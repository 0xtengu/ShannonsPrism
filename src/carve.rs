// Copyright (C) 2026 0xtengu
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

//! Embedded-stream carving: walk the high-entropy regions the entropy map
//! flags, decode known stream formats found there, and report each payload,
//! recursing one level down. Codecs with a magic number (zlib, gzip, xz, zstd,
//! lz4, bzip2) are tried at every offset of a hot region; headerless codecs
//! (LZMA-alone, raw deflate) only around region starts, where a stream would
//! have to begin for the map to have noticed it.

use crate::emap::{self, Band, EntropyMap};
use crate::format;
use crate::score::Metrics;
use std::io::{BufRead, Read};

/// One stream found and decoded.
#[derive(Debug, Clone)]
pub struct Found
{
    pub offset: usize,
    pub codec: &'static str,
    /// Whether the stream was found by a magic number or by blind probing.
    pub by_magic: bool,
    /// Bytes of input the decoder consumed: the stream's length in the file.
    pub stream_len: usize,
    pub decoded_len: usize,
    /// Output was cut at the decode cap; `decoded_len` is a lower bound.
    pub capped: bool,
    /// The decoder failed part-way; what it produced before that is reported.
    pub partial: Option<String>,
    pub entropy: f64,
    pub band: Option<Band>,
    pub uniformity: Option<emap::Uniformity>,
    /// What `format::identify` calls the payload.
    pub identity: String,
    /// "ELF aarch64 executable", when the payload parses as a container.
    pub container: Option<String>,
    /// Streams found inside the payload, one level down.
    pub children: Vec<Found>,
    /// The decoded bytes, for a caller to write out. Empty for children past the recursion limit.
    pub payload: Vec<u8>,
}

pub struct Options
{
    /// Stop decoding one stream past this many output bytes; bounds memory against decompression bombs.
    pub max_decoded: usize,
    /// How many levels of payload to carve in turn.
    pub depth: usize,
    /// Try the headerless probes (LZMA-alone, raw deflate) at region starts.
    pub probe_headerless: bool,
}

impl Default for Options
{
    fn default() -> Self
    {
        Options {
            max_decoded: 256 << 20,
            depth: 1,
            probe_headerless: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Signatures
// ---------------------------------------------------------------------------

/// A magic-number check; the decode attempt is the real test.
struct Sig
{
    codec: &'static str,
    first: u8,
    matches: fn(&[u8]) -> bool,
}

fn isZlib(b: &[u8]) -> bool
{
    // RFC 1950: CM = 8, CINFO <= 7, no preset dictionary, header checksum.
    b.len() >= 2
        && b[0] & 0x0f == 8
        && b[0] >> 4 <= 7
        && b[1] & 0x20 == 0
        && (b[0] as u32 * 256 + b[1] as u32) % 31 == 0
}

fn isGzip(b: &[u8]) -> bool
{
    b.len() >= 4 && b[0] == 0x1f && b[1] == 0x8b && b[2] == 8 && b[3] & 0xe0 == 0
}

fn isXz(b: &[u8]) -> bool
{
    b.len() >= 6 && b[..6] == [0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00]
}

fn isZstd(b: &[u8]) -> bool
{
    b.len() >= 4 && b[..4] == [0x28, 0xb5, 0x2f, 0xfd]
}

fn isLz4(b: &[u8]) -> bool
{
    // Frame magic, then FLG with version bits == 01.
    b.len() >= 5 && b[..4] == [0x04, 0x22, 0x4d, 0x18] && b[4] & 0xc0 == 0x40
}

fn isBzip2(b: &[u8]) -> bool
{
    // "BZh" + level digit + the first block's magic (pi, in BCD).
    b.len() >= 10
        && &b[..3] == b"BZh"
        && (b'1'..=b'9').contains(&b[3])
        && b[4..10] == [0x31, 0x41, 0x59, 0x26, 0x53, 0x59]
}

const SIGS: &[Sig] = &[
    Sig {
        codec: "gzip",
        first: 0x1f,
        matches: isGzip,
    },
    Sig {
        codec: "xz",
        first: 0xfd,
        matches: isXz,
    },
    Sig {
        codec: "zstd",
        first: 0x28,
        matches: isZstd,
    },
    Sig {
        codec: "lz4",
        first: 0x04,
        matches: isLz4,
    },
    Sig {
        codec: "bzip2",
        first: b'B',
        matches: isBzip2,
    },
    // zlib last: its two-byte header is the noisiest signature.
    Sig {
        codec: "zlib",
        first: 0x08,
        matches: isZlib,
    },
];

/// Screen LZMA-alone by its 13-byte header: props < 225, a power-of-two (or 1.5x) dict, plausible unpacked size.
fn plausibleLzmaAlone(b: &[u8]) -> bool
{
    if b.len() < 13 || b[0] >= 225
    {
        return false;
    }
    let dict = u32::from_le_bytes([b[1], b[2], b[3], b[4]]);
    let pow2 = dict.is_power_of_two();
    let pow2_and_half = dict >= 3 && (dict / 3).is_power_of_two() && dict % 3 == 0;
    if !(dict >= 4096 && (pow2 || pow2_and_half))
    {
        return false;
    }
    let size = u64::from_le_bytes(b[5..13].try_into().unwrap());
    size == u64::MAX || size < (1u64 << 36)
}

// ---------------------------------------------------------------------------
// Decoding with exact input accounting
// ---------------------------------------------------------------------------

/// A BufRead over a slice whose `consume` is the only way forward, so after a
/// decode `pos` is exactly the bytes the stream used. The `bufread::*` decoder
/// variants must be used over this; `read::*` buffer ahead and over-report.
struct Slice<'a>
{
    d: &'a [u8],
    pos: usize,
}

impl<'a> Read for Slice<'a>
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>
    {
        let n = buf.len().min(self.d.len() - self.pos);
        buf[..n].copy_from_slice(&self.d[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl<'a> BufRead for Slice<'a>
{
    fn fill_buf(&mut self) -> std::io::Result<&[u8]>
    {
        Ok(&self.d[self.pos..])
    }
    fn consume(&mut self, n: usize)
    {
        self.pos = (self.pos + n).min(self.d.len());
    }
}

/// Outcome of one decode attempt.
struct Probe
{
    consumed: usize,
    out: Vec<u8>,
    capped: bool,
    error: Option<String>,
}

/// Pull from a decoder until it ends, errors, or the cap is reached; output produced before an error is kept.
fn drain<R: Read>(r: &mut R, cap: usize) -> (Vec<u8>, bool, Option<String>)
{
    let mut out = Vec::new();
    let err = r
        .take(cap as u64)
        .read_to_end(&mut out)
        .err()
        .map(|e| e.to_string());
    let capped = err.is_none() && out.len() >= cap;
    (out, capped, err)
}

/// Decode `codec` starting at `data[0]`.
fn probe(codec: &str, data: &[u8], cap: usize) -> Probe
{
    let src = Slice { d: data, pos: 0 };
    let (out, capped, error, consumed) = match codec
    {
        "zlib" =>
        {
            let mut dec = flate2::bufread::ZlibDecoder::new(src);
            let (o, c, e) = drain(&mut dec, cap);
            (o, c, e, dec.get_ref().pos)
        }
        "gzip" =>
        {
            let mut dec = flate2::bufread::GzDecoder::new(src);
            let (o, c, e) = drain(&mut dec, cap);
            (o, c, e, dec.get_ref().pos)
        }
        "deflate" =>
        {
            let mut dec = flate2::bufread::DeflateDecoder::new(src);
            let (o, c, e) = drain(&mut dec, cap);
            (o, c, e, dec.get_ref().pos)
        }
        "xz" =>
        {
            return match xz2::stream::Stream::new_stream_decoder(u64::MAX, 0)
            {
                Ok(st) => runLzma(st, data, cap),
                Err(e) => Probe {
                    consumed: 0,
                    out: Vec::new(),
                    capped: false,
                    error: Some(e.to_string()),
                },
            };
        }
        "lzma" =>
        {
            // The memory limit bounds the dictionary the header may ask for.
            return match xz2::stream::Stream::new_lzma_decoder(512 << 20)
            {
                Ok(st) => runLzma(st, data, cap),
                Err(e) => Probe {
                    consumed: 0,
                    out: Vec::new(),
                    capped: false,
                    error: Some(e.to_string()),
                },
            };
        }
        "bzip2" =>
        {
            let mut dec = bzip2::bufread::BzDecoder::new(src);
            let (o, c, e) = drain(&mut dec, cap);
            (o, c, e, dec.get_ref().pos)
        }
        "zstd" => match zstd::stream::read::Decoder::with_buffer(src)
        {
            Ok(dec) =>
            {
                let mut dec = dec.single_frame();
                let (o, c, e) = drain(&mut dec, cap);
                (o, c, e, dec.get_ref().pos)
            }
            Err(e) =>
            {
                return Probe {
                    consumed: 0,
                    out: Vec::new(),
                    capped: false,
                    error: Some(e.to_string()),
                }
            }
        },
        "lz4" =>
        {
            // The frame reader would treat bytes after the end mark as another
            // frame, so hand it exactly one frame, measured up front.
            let Some(n) = lz4FrameLen(data)
            else
            {
                return Probe {
                    consumed: 0,
                    out: Vec::new(),
                    capped: false,
                    error: Some("lz4: frame does not terminate".into()),
                };
            };
            let mut dec = lz4_flex::frame::FrameDecoder::new(Slice {
                d: &data[..n],
                pos: 0,
            });
            let (o, c, e) = drain(&mut dec, cap);
            (o, c, e, dec.get_ref().pos)
        }
        other =>
        {
            return Probe {
                consumed: 0,
                out: Vec::new(),
                capped: false,
                error: Some(format!("no prober for {}", other)),
            };
        }
    };
    Probe {
        consumed,
        out,
        capped,
        error,
    }
}

/// Drive an xz or LZMA-alone decoder by hand: `process` reports exactly when the
/// stream ended and how much input it took, avoiding the reader wrappers' false
/// corruption on bytes that follow a finished stream.
fn runLzma(mut st: xz2::stream::Stream, data: &[u8], cap: usize) -> Probe
{
    use xz2::stream::{Action, Status};
    let mut out: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 64 << 10];
    loop
    {
        let consumed = st.total_in() as usize;
        let produced = st.total_out() as usize;
        let want = buf.len().min(cap - out.len());
        if want == 0
        {
            return Probe {
                consumed,
                out,
                capped: true,
                error: None,
            };
        }
        let input = &data[consumed.min(data.len())..];
        let action = if input.is_empty()
        {
            Action::Finish
        }
        else
        {
            Action::Run
        };
        let status = st.process(input, &mut buf[..want], action);
        let got = st.total_out() as usize - produced;
        out.extend_from_slice(&buf[..got]);
        match status
        {
            Ok(Status::StreamEnd) =>
            {
                return Probe {
                    consumed: st.total_in() as usize,
                    out,
                    capped: false,
                    error: None,
                };
            }
            Ok(_) =>
            {
                if input.is_empty() && got == 0
                {
                    return Probe {
                        consumed: st.total_in() as usize,
                        out,
                        capped: false,
                        error: Some("stream truncated at end of data".into()),
                    };
                }
            }
            Err(e) =>
            {
                return Probe {
                    consumed: st.total_in() as usize,
                    out,
                    capped: false,
                    error: Some(e.to_string()),
                };
            }
        }
    }
}

/// Length of the lz4 frame at `b[0]` by walking its block headers, or None if it does not terminate in the buffer.
fn lz4FrameLen(b: &[u8]) -> Option<usize>
{
    if b.len() < 7
    {
        return None;
    }
    let flg = b[4];
    let mut p = 6;
    if flg & 0x08 != 0
    {
        p += 8;
    }
    if flg & 0x01 != 0
    {
        p += 4;
    }
    p += 1; // header checksum
    let block_checksum = flg & 0x10 != 0;
    loop
    {
        if p + 4 > b.len()
        {
            return None;
        }
        let size = u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]);
        p += 4;
        if size == 0
        {
            break;
        }
        let len = (size & 0x7fff_ffff) as usize;
        if len > 4 << 20
        {
            return None; // larger than any block size the format allows
        }
        p = p.checked_add(len)?;
        if block_checksum
        {
            p += 4;
        }
        if p > b.len()
        {
            return None;
        }
    }
    if flg & 0x04 != 0
    {
        p += 4;
    }
    if p > b.len()
    {
        None
    }
    else
    {
        Some(p)
    }
}

/// Only blind-probe raw deflate that starts with a dynamic-Huffman block: fixed and stored blocks decode from random bytes too readily.
fn plausibleRawDeflate(b: &[u8]) -> bool
{
    !b.is_empty() && (b[0] >> 1) & 3 == 2
}

// ---------------------------------------------------------------------------
// Acceptance
// ---------------------------------------------------------------------------

/// A blind probe is believed only when it decodes at least this much and finishes cleanly.
const MIN_HEADERLESS_CLEAN: usize = 4096;
/// A stream that errored part-way is reported only when at least this much came out.
const MIN_PARTIAL: usize = 64 << 10;

fn accept(p: &Probe, by_magic: bool) -> bool
{
    match &p.error
    {
        None =>
        {
            if p.capped
            {
                return true;
            }
            if by_magic
            {
                p.consumed >= 8 && (p.out.len() > 0 || p.consumed >= 16)
            }
            else
            {
                p.out.len() >= MIN_HEADERLESS_CLEAN && p.consumed >= 256
            }
        }
        Some(_) => p.out.len() >= MIN_PARTIAL,
    }
}

fn describePayload(
    out: &[u8],
) -> (
    f64,
    Option<Band>,
    Option<emap::Uniformity>,
    String,
    Option<String>,
)
{
    let m = Metrics::of(out);
    let band = Band::classifySection(m.entropy, out.len());
    let uni = if band == Some(Band::Random) && out.len() >= emap::MIN_CHI2_BYTES
    {
        Some(emap::uniformity(out))
    }
    else
    {
        None
    };
    let ident = format::identify(out);
    let container = format::parse(out).map(|c| format!("{} {} {}", c.format, c.arch, c.kind));
    (m.entropy, band, uni, ident.label, container)
}

// ---------------------------------------------------------------------------
// Walking the map
// ---------------------------------------------------------------------------

/// Offset ranges worth a blind probe: every Compressed or Random region, widened one window left for the stream header.
pub fn scanRanges(map: &EntropyMap, len: usize) -> Vec<(usize, usize)>
{
    let mut out: Vec<(usize, usize)> = Vec::new();
    for r in &map.regions
    {
        if !matches!(r.band, Band::Compressed | Band::Random)
        {
            continue;
        }
        let start = r.start.saturating_sub(map.window);
        let end = r.end.min(len);
        match out.last_mut()
        {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => out.push((start, end)),
        }
    }
    out
}

/// Per-block flag: is this block code or denser? Used to gate the noisy zlib signature to plausible locations.
fn denseBlocks(map: &EntropyMap) -> Vec<bool>
{
    map.blocks
        .iter()
        .map(|b| Band::ofWindowed(b.entropy, map.window) >= Band::Code)
        .collect()
}

/// Carve one buffer. `map` must have been computed over `data`.
pub fn carve(data: &[u8], map: &EntropyMap, opts: &Options) -> Vec<Found>
{
    carveLevel(data, map, opts, 0)
}

fn carveLevel(data: &[u8], map: &EntropyMap, opts: &Options, level: usize) -> Vec<Found>
{
    let mut found: Vec<Found> = Vec::new();
    let ranges = scanRanges(map, data.len());

    let covered = |found: &[Found], off: usize| {
        found
            .iter()
            .find(|f| off >= f.offset && off < f.offset + f.stream_len)
            .map(|f| f.offset + f.stream_len)
    };

    // Magic numbers first; anything they cover is off-limits to the blind probes.
    let dense = denseBlocks(map);
    let step = map.step.max(1);
    let is_dense = |off: usize| dense.get(off / step).copied().unwrap_or(false);
    let mut off = 0usize;
    while off < data.len()
    {
        if let Some(next) = covered(&found, off)
        {
            off = next;
            continue;
        }
        let b = data[off];
        let mut hit = false;
        for sig in SIGS
        {
            // zlib's first byte varies with the window size; the others are fixed.
            let first_ok = if sig.codec == "zlib"
            {
                b & 0x0f == 8 && is_dense(off)
            }
            else
            {
                b == sig.first
            };
            if !first_ok || !(sig.matches)(&data[off..])
            {
                continue;
            }
            let p = probe(sig.codec, &data[off..], opts.max_decoded);
            if accept(&p, true)
            {
                let f = makeFound(off, sig.codec, true, p);
                off = f.offset + f.stream_len.max(1);
                found.push(f);
                hit = true;
                break;
            }
        }
        if !hit
        {
            off += 1;
        }
    }

    // Then the headerless codecs, only around where the entropy steps up: the
    // start of each hot range, and any region whose band rises above its
    // predecessor's (where a payload following a header or 0xff run begins).
    if opts.probe_headerless
    {
        let mut edges: Vec<usize> = ranges.iter().map(|&(lo, _)| lo + map.window).collect();
        let mut prev = Band::Padding;
        for r in &map.regions
        {
            if r.band > prev && r.band >= Band::Structured
            {
                edges.push(r.start);
            }
            prev = r.band;
        }
        edges.sort_unstable();
        edges.dedup();
        for start in edges
        {
            let a = start.saturating_sub(map.window);
            let z = (start + map.window).min(data.len());
            // Every offset in a small window; on wide windows only aligned ones.
            let step = if map.window > 4096 { 4 } else { 1 };
            let mut off = a;
            while off < z
            {
                if let Some(next) = covered(&found, off)
                {
                    off = next;
                    continue;
                }
                for codec in ["lzma", "deflate"]
                {
                    let plausible = match codec
                    {
                        "lzma" => plausibleLzmaAlone(&data[off..]),
                        _ => plausibleRawDeflate(&data[off..]),
                    };
                    if !plausible
                    {
                        continue;
                    }
                    let p = probe(codec, &data[off..], opts.max_decoded);
                    if accept(&p, false)
                    {
                        found.push(makeFound(off, codec, false, p));
                        break;
                    }
                }
                off += step;
            }
        }
    }

    found.sort_by_key(|f| f.offset);

    if level < opts.depth
    {
        for f in found.iter_mut()
        {
            if f.payload.len() < 64
            {
                continue;
            }
            let w = (f.payload.len() / 2048).clamp(64, 65536);
            let sub = emap::compute(&f.payload, w, w);
            let mut kids = carveLevel(&f.payload, &sub, opts, level + 1);
            // Children drop their payloads to bound memory; the caller can re-derive them.
            for k in kids.iter_mut()
            {
                k.payload = Vec::new();
            }
            f.children = kids;
        }
    }
    found
}

fn makeFound(offset: usize, codec: &'static str, by_magic: bool, p: Probe) -> Found
{
    let (entropy, band, uniformity, identity, container) = describePayload(&p.out);
    Found {
        offset,
        codec,
        by_magic,
        stream_len: p.consumed,
        decoded_len: p.out.len(),
        capped: p.capped,
        partial: p.error,
        entropy,
        band,
        uniformity,
        identity,
        container,
        children: Vec::new(),
        payload: p.out,
    }
}

/// Try one codec at one offset, for `--at`-style drilling and for tests.
pub fn probeAt(data: &[u8], offset: usize, codec: &'static str, max_decoded: usize)
    -> Option<Found>
{
    if offset >= data.len()
    {
        return None;
    }
    let p = probe(codec, &data[offset..], max_decoded);
    let by_magic = codec != "lzma" && codec != "deflate";
    if accept(&p, by_magic)
    {
        Some(makeFound(offset, codec, by_magic, p))
    }
    else
    {
        None
    }
}

/// Codec names the prober understands.
pub const CODECS: &[&str] = &[
    "zlib", "gzip", "deflate", "xz", "lzma", "bzip2", "zstd", "lz4",
];

pub fn human(n: usize) -> String
{
    if n >= 1 << 30
    {
        format!("{:.1} GB", n as f64 / (1u64 << 30) as f64)
    }
    else if n >= 1 << 20
    {
        format!("{:.1} MB", n as f64 / (1u64 << 20) as f64)
    }
    else if n >= 1 << 10
    {
        format!("{:.1} KB", n as f64 / 1024.0)
    }
    else
    {
        format!("{} B", n)
    }
}
