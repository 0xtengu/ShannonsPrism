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

//! Packing and decompressor emission: turn a codec choice into a packed
//! container, and optionally a dependency-free C file that unpacks it. The
//! built-in codecs each have a decoder small enough to inline into that C,
//! which library codecs like zstd and xz cannot offer.

use crate::codecs;
use crate::libs;
use crate::registry::{self, CDecoder};

/// Decode with whichever implementation owns this codec: built-in or library.
pub fn decodeAny(name: &str, data: &[u8], params: &codecs::Params) -> Result<Vec<u8>, String>
{
    if registry::find(name).map(|c| c.backend) == Some(registry::Backend::Native)
    {
        return codecs::decodeOne(name, data, params);
    }
    match libs::find(name)
    {
        Some(c) => libs::decodeSafe(c, data),
        None => Err(format!("unknown codec: {}", name)),
    }
}

/// Which library or C template turns this codec's chunks back into bytes, for the packing recipe.
pub fn decoderHint(name: &str) -> String
{
    if let Some((_, f)) = templateFor(name)
    {
        return format!("C template {}()", f);
    }
    registry::find(name)
        .map(registry::decoderHint)
        .unwrap_or("unknown")
        .into()
}

/// A codec is portable when a dependency-free C decoder exists for it.
pub fn isPortable(name: &str) -> bool
{
    templateFor(name).is_some()
}

/// One packed piece of the original: a whole file, or one section.
pub struct Chunk
{
    pub label: String,
    pub codec: String,
    pub offset: usize,
    pub original_len: usize,
    pub data: Vec<u8>,
}

pub struct Package
{
    pub chunks: Vec<Chunk>,
    pub original_len: usize,
    /// Bytes of the original not covered by any chunk (gaps between sections,
    /// headers). Stored verbatim so the output reconstructs exactly.
    pub gaps: Vec<(usize, Vec<u8>)>,
}

impl Package
{
    pub fn packedLen(&self) -> usize
    {
        self.chunks.iter().map(|c| c.data.len()).sum::<usize>()
            + self.gaps.iter().map(|(_, g)| g.len()).sum::<usize>()
    }

    /// Byte offset of each chunk's payload inside the serialised .epk, in chunk order.
    pub fn layout(&self) -> Vec<usize>
    {
        let mut pos = HEADER_LEN;
        let mut out = Vec::with_capacity(self.chunks.len());
        for c in &self.chunks
        {
            pos += 1 + c.codec.len() + 12;
            out.push(pos);
            pos += c.data.len();
        }
        out
    }
}

/// ENPK2 magic plus the three u32 counts.
const HEADER_LEN: usize = 5 + 4 + 4 + 4;

/// Reconstruct the original from a package to verify packing before writing to disk.
pub fn verify(pkg: &Package, original: &[u8], params: &codecs::Params) -> Result<(), String>
{
    let mut out = vec![0u8; pkg.original_len];
    for c in &pkg.chunks
    {
        let raw =
            decodeAny(&c.codec, &c.data, params).map_err(|e| format!("{}: {}", c.label, e))?;
        if raw.len() != c.original_len
        {
            return Err(format!(
                "{}: decoded {} bytes, expected {}",
                c.label,
                raw.len(),
                c.original_len
            ));
        }
        out[c.offset..c.offset + raw.len()].copy_from_slice(&raw);
    }
    for (off, g) in &pkg.gaps
    {
        out[*off..*off + g.len()].copy_from_slice(g);
    }
    if out != original
    {
        return Err("reconstruction does not match the original".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Binary container
// ---------------------------------------------------------------------------
//
// ENPK2 | u32 original_len | u32 chunk_count | u32 gap_count
//       | chunks: u8 name_len, name, u32 offset, u32 original_len, u32 packed_len, bytes
//       | gaps:   u32 offset, u32 len, bytes
//
// Codec names, not numeric ids, so the list can change without breaking .epks.

pub fn serialise(pkg: &Package) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    out.extend_from_slice(b"ENPK2");
    out.extend_from_slice(&(pkg.original_len as u32).to_le_bytes());
    out.extend_from_slice(&(pkg.chunks.len() as u32).to_le_bytes());
    out.extend_from_slice(&(pkg.gaps.len() as u32).to_le_bytes());
    for c in &pkg.chunks
    {
        let name = c.codec.as_bytes();
        if name.len() > 255
        {
            return Err(format!("codec name too long: {}", c.codec));
        }
        out.push(name.len() as u8);
        out.extend_from_slice(name);
        out.extend_from_slice(&(c.offset as u32).to_le_bytes());
        out.extend_from_slice(&(c.original_len as u32).to_le_bytes());
        out.extend_from_slice(&(c.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&c.data);
    }
    for (off, g) in &pkg.gaps
    {
        out.extend_from_slice(&(*off as u32).to_le_bytes());
        out.extend_from_slice(&(g.len() as u32).to_le_bytes());
        out.extend_from_slice(g);
    }
    Ok(out)
}

/// Parse a serialised container back into a Package, checking every length against the bytes present.
pub fn parse(bytes: &[u8]) -> Result<Package, String>
{
    struct Rd<'a>
    {
        b: &'a [u8],
        p: usize,
    }
    impl<'a> Rd<'a>
    {
        fn take(&mut self, n: usize) -> Result<&'a [u8], String>
        {
            if n > self.b.len() - self.p
            {
                return Err(format!("truncated container at byte {}", self.p));
            }
            let s = &self.b[self.p..self.p + n];
            self.p += n;
            Ok(s)
        }
        fn u32(&mut self) -> Result<usize, String>
        {
            let s = self.take(4)?;
            Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
        }
    }
    if bytes.len() < HEADER_LEN || &bytes[..5] != b"ENPK2"
    {
        return Err("not an ENPK2 container".into());
    }
    let mut r = Rd { b: bytes, p: 5 };
    let original_len = r.u32()?;
    let nchunks = r.u32()?;
    let ngaps = r.u32()?;

    let mut chunks = Vec::new();
    for _ in 0..nchunks
    {
        let nlen = r.take(1)?[0] as usize;
        let codec = String::from_utf8(r.take(nlen)?.to_vec())
            .map_err(|_| "codec name is not UTF-8".to_string())?;
        let offset = r.u32()?;
        let olen = r.u32()?;
        let plen = r.u32()?;
        let data = r.take(plen)?.to_vec();
        if offset
            .checked_add(olen)
            .map(|e| e > original_len)
            .unwrap_or(true)
        {
            return Err(format!(
                "chunk {} at 0x{:x} runs past the declared length",
                codec, offset
            ));
        }
        chunks.push(Chunk {
            label: codec.clone(),
            codec,
            offset,
            original_len: olen,
            data,
        });
    }
    let mut gaps = Vec::new();
    for _ in 0..ngaps
    {
        let offset = r.u32()?;
        let len = r.u32()?;
        let g = r.take(len)?.to_vec();
        if offset
            .checked_add(len)
            .map(|e| e > original_len)
            .unwrap_or(true)
        {
            return Err(format!(
                "gap at 0x{:x} runs past the declared length",
                offset
            ));
        }
        gaps.push((offset, g));
    }
    // Trust the declared length only once the pieces account for it, so a
    // small file cannot ask for a huge output buffer.
    let accounted: usize = chunks.iter().map(|c| c.original_len).sum::<usize>()
        + gaps.iter().map(|(_, g)| g.len()).sum::<usize>();
    if accounted != original_len
    {
        return Err(format!(
            "chunks and gaps cover {} bytes but the header declares {}",
            accounted, original_len
        ));
    }
    Ok(Package {
        chunks,
        original_len,
        gaps,
    })
}

/// Unpack a serialised container back to the original bytes.
pub fn deserialise(bytes: &[u8], params: &codecs::Params) -> Result<Vec<u8>, String>
{
    let pkg = parse(bytes)?;
    let mut out = vec![0u8; pkg.original_len];
    for c in &pkg.chunks
    {
        let raw = decodeAny(&c.codec, &c.data, params)
            .map_err(|e| format!("{} at 0x{:x}: {}", c.codec, c.offset, e))?;
        if raw.len() != c.original_len
        {
            return Err(format!(
                "{} at 0x{:x}: decoded {} bytes, expected {}",
                c.codec,
                c.offset,
                raw.len(),
                c.original_len
            ));
        }
        out[c.offset..c.offset + raw.len()].copy_from_slice(&raw);
    }
    for (off, g) in &pkg.gaps
    {
        out[*off..*off + g.len()].copy_from_slice(g);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// C decoder templates
// ---------------------------------------------------------------------------
//
// Each mirrors the matching Rust encoder in codecs.rs, with no allocation and
// no libc, so they drop into freestanding builds.

const C_LZSS: &str = r#"
/* LZSS: u32le original length, then flag byte per 8 tokens.
   flag bit set -> u16be (dist << 4 | len - MIN_MATCH); clear -> literal. */
static unsigned lzss_decode(const unsigned char *in, unsigned in_len,
                            unsigned char *out, unsigned out_cap) {
    if (in_len < 4) return 0;
    unsigned n = (unsigned)in[0] | ((unsigned)in[1]<<8) | ((unsigned)in[2]<<16) | ((unsigned)in[3]<<24);
    if (n > out_cap) return 0;
    unsigned i = 4, o = 0;
    while (o < n) {
        if (i >= in_len) return 0;
        unsigned char flags = in[i++];
        for (int b = 0; b < 8 && o < n; b++) {
            if (flags & (1u << b)) {
                if (i + 1 >= in_len) return 0;
                unsigned v = ((unsigned)in[i] << 8) | in[i+1]; i += 2;
                unsigned dist = v >> 4, len = (v & 0xF) + MIN_MATCH;
                if (dist == 0 || dist > o || o + len > n) return 0;
                for (unsigned k = 0; k < len; k++) { out[o] = out[o - dist]; o++; }
            } else {
                if (i >= in_len) return 0;
                out[o++] = in[i++];
            }
        }
    }
    return o;
}
"#;

const C_LZ4ISH: &str = r#"
/* LZ4-like: token (litlen<<4 | matchlen), 255-extended lengths,
   literals, u16le offset, match. Final sequence is literals only. */
static unsigned lz4ish_decode(const unsigned char *in, unsigned in_len,
                              unsigned char *out, unsigned out_cap) {
    unsigned i = 0, o = 0;
    while (i < in_len) {
        unsigned char tok = in[i++];
        unsigned lit = tok >> 4;
        if (lit == 15) { unsigned char e; do { if (i>=in_len) return 0; e = in[i++]; lit += e; } while (e == 255); }
        if (i + lit > in_len || o + lit > out_cap) return 0;
        for (unsigned k = 0; k < lit; k++) out[o++] = in[i++];
        if (i >= in_len) break;
        if (i + 1 >= in_len) return 0;
        unsigned dist = (unsigned)in[i] | ((unsigned)in[i+1] << 8); i += 2;
        unsigned ml = tok & 0xF;
        if (ml == 15) { unsigned char e; do { if (i>=in_len) return 0; e = in[i++]; ml += e; } while (e == 255); }
        ml += 4;
        if (dist == 0 || dist > o || o + ml > out_cap) return 0;
        for (unsigned k = 0; k < ml; k++) { out[o] = out[o - dist]; o++; }
    }
    return o;
}
"#;

const C_LZ4: &str = r#"
/* u32le output length followed by a raw LZ4 block. */
static unsigned lz4_decode(const unsigned char *in, unsigned in_len,
                           unsigned char *out, unsigned out_cap) {
    if (in_len < 4) return 0;
    unsigned want = (unsigned)in[0] | ((unsigned)in[1]<<8) |
                    ((unsigned)in[2]<<16) | ((unsigned)in[3]<<24);
    if (want > out_cap) return 0;
    unsigned i = 4, o = 0;
    while (i < in_len) {
        unsigned token = in[i++], lit = token >> 4, len;
        if (lit == 15) {
            unsigned b;
            do { if (i >= in_len) return 0; b = in[i++]; lit += b; } while (b == 255);
        }
        if (lit > in_len - i || lit > want - o) return 0;
        while (lit--) out[o++] = in[i++];
        if (i == in_len) break;
        if (i + 2 > in_len) return 0;
        unsigned dist = (unsigned)in[i] | ((unsigned)in[i + 1] << 8); i += 2;
        len = (token & 15u) + 4u;
        if ((token & 15u) == 15u) {
            unsigned b;
            do { if (i >= in_len) return 0; b = in[i++]; len += b; } while (b == 255);
        }
        if (!dist || dist > o || len > want - o) return 0;
        while (len--) { out[o] = out[o - dist]; o++; }
    }
    return o == want ? o : 0;
}
"#;

const C_SNAPPY: &str = r#"
/* Snappy raw stream: varint output length followed by literal/copy tags. */
static unsigned snappy_decode(const unsigned char *in, unsigned in_len,
                              unsigned char *out, unsigned out_cap) {
    unsigned i = 0, want = 0, shift = 0;
    for (;;) {
        if (i >= in_len || shift >= 32) return 0;
        unsigned b = in[i++]; want |= (b & 127u) << shift;
        if (!(b & 128u)) break;
        shift += 7;
    }
    if (want > out_cap) return 0;
    unsigned o = 0;
    while (o < want) {
        if (i >= in_len) return 0;
        unsigned tag = in[i++], kind = tag & 3u, len, dist = 0;
        if (kind == 0) {
            len = tag >> 2;
            if (len < 60) len++;
            else {
                unsigned n = len - 59u; len = 0;
                if (n > 4 || n > in_len - i) return 0;
                for (unsigned k = 0; k < n; k++) len |= (unsigned)in[i++] << (8u * k);
                len++;
            }
            if (len > in_len - i || len > want - o) return 0;
            while (len--) out[o++] = in[i++];
            continue;
        }
        if (kind == 1) {
            if (i >= in_len) return 0;
            len = 4u + ((tag >> 2) & 7u);
            dist = ((tag & 224u) << 3) | in[i++];
        } else if (kind == 2) {
            if (i + 2 > in_len) return 0;
            len = 1u + (tag >> 2);
            dist = (unsigned)in[i] | ((unsigned)in[i + 1] << 8); i += 2;
        } else {
            if (i + 4 > in_len) return 0;
            len = 1u + (tag >> 2);
            dist = (unsigned)in[i] | ((unsigned)in[i + 1] << 8) |
                   ((unsigned)in[i + 2] << 16) | ((unsigned)in[i + 3] << 24); i += 4;
        }
        if (!dist || dist > o || len > want - o) return 0;
        while (len--) { out[o] = out[o - dist]; o++; }
    }
    return o;
}
"#;

const C_LZF: &str = r#"
/* "LZF1", u32le output length, then a liblzf-compatible stream. */
static unsigned lzf_decode(const unsigned char *in, unsigned in_len,
                           unsigned char *out, unsigned out_cap) {
    if (in_len < 8 || in[0]!='L' || in[1]!='Z' || in[2]!='F' || in[3]!='1') return 0;
    unsigned want = (unsigned)in[4] | ((unsigned)in[5]<<8) |
                    ((unsigned)in[6]<<16) | ((unsigned)in[7]<<24);
    if (want > out_cap) return 0;
    unsigned i = 8, o = 0;
    while (i < in_len) {
        unsigned ctrl = in[i++];
        if (ctrl < 32) {
            unsigned n = ctrl + 1u;
            if (n > in_len - i || n > want - o) return 0;
            while (n--) out[o++] = in[i++];
        } else {
            unsigned len = ctrl >> 5, dist = (ctrl & 31u) << 8;
            if (len == 7) { if (i >= in_len) return 0; len += in[i++]; }
            if (i >= in_len) return 0;
            dist += in[i++] + 1u; len += 2u;
            if (dist > o || len > want - o) return 0;
            while (len--) { out[o] = out[o - dist]; o++; }
        }
    }
    return o == want ? o : 0;
}
"#;

const C_LZJB: &str = r#"
/* Base-128 big-endian size header followed by the ZFS LZJB token stream. */
static unsigned lzjb_decode(const unsigned char *in, unsigned in_len,
                            unsigned char *out, unsigned out_cap) {
    unsigned i = 0, encoded = 0;
    for (;;) {
        if (i >= in_len || encoded > 0x01ffffffu) return 0;
        unsigned c = in[i++];
        if (c & 128u) { encoded |= c & 127u; break; }
        encoded = (encoded | c) * 128u;
    }
    if (!encoded) return 0;
    unsigned want = encoded - 1u, o = 0, mask = 128u, map = 0, windowpos = 0;
    unsigned char window[1024];
    if (want > out_cap) return 0;
    while (o < want) {
        unsigned c;
        mask <<= 1;
        if (mask == 256u) {
            if (i >= in_len) return 0;
            map = in[i++]; mask = 1;
        }
        if (i >= in_len) return 0;
        c = in[i++];
        if (map & mask) {
            if (i >= in_len) return 0;
            unsigned pair = (c << 8) | in[i++];
            unsigned len = (c >> 2) + 3u, off = pair & 1023u;
            unsigned dist = off ? off : 1024u;
            unsigned cpy = (windowpos + 1024u - off) & 1023u;
            if (dist > o || len > want - o) return 0;
            while (len--) {
                unsigned char b = window[cpy]; cpy = (cpy + 1u) & 1023u;
                out[o++] = b; window[windowpos] = b; windowpos = (windowpos + 1u) & 1023u;
            }
        } else {
            out[o++] = (unsigned char)c;
            window[windowpos] = (unsigned char)c; windowpos = (windowpos + 1u) & 1023u;
        }
    }
    return o;
}
"#;

const C_BRIEFLZ: &str = r#"
typedef struct {
    const unsigned char *in; unsigned in_len, pos, tag; int bits;
} blz_state;
static int blz_bit(blz_state *s, unsigned *v) {
    int was = s->bits--;
    if (was == 0) {
        if (s->pos + 2 > s->in_len) return 0;
        s->tag = (unsigned)s->in[s->pos] | ((unsigned)s->in[s->pos + 1] << 8);
        s->pos += 2; s->bits = 15;
    }
    *v = (s->tag & 0x8000u) != 0; s->tag <<= 1;
    return 1;
}
static int blz_gamma(blz_state *s, unsigned *v) {
    unsigned n = 1, bit, more;
    do {
        if (n & 0x80000000u || !blz_bit(s, &bit) || !blz_bit(s, &more)) return 0;
        n = (n << 1) + bit;
    } while (more);
    *v = n; return 1;
}
/* "BLZ1", u32le output length, then BriefLZ's split tag/data stream. */
static unsigned brieflz_decode(const unsigned char *in, unsigned in_len,
                               unsigned char *out, unsigned out_cap) {
    if (in_len < 8 || in[0]!='B' || in[1]!='L' || in[2]!='Z' || in[3]!='1') return 0;
    unsigned want = (unsigned)in[4] | ((unsigned)in[5]<<8) |
                    ((unsigned)in[6]<<16) | ((unsigned)in[7]<<24);
    if (want > out_cap) return 0;
    blz_state s = { in + 8, in_len - 8, 0, 0, 1 };
    unsigned o = 0;
    while (o < want) {
        unsigned match;
        if (!blz_bit(&s, &match)) return 0;
        if (!match) {
            if (s.pos >= s.in_len) return 0;
            out[o++] = s.in[s.pos++];
        } else {
            unsigned len, hi;
            if (!blz_gamma(&s, &len) || !blz_gamma(&s, &hi) || hi < 2 || s.pos >= s.in_len) return 0;
            len += 2; unsigned dist = ((hi - 2u) << 8) + s.in[s.pos++] + 1u;
            if (dist > o || len > want - o) return 0;
            while (len--) { out[o] = out[o - dist]; o++; }
        }
    }
    return o;
}
"#;

const C_LZO1X: &str = r#"
/* u64le output length followed by an LZO1X stream. This is a bounded port of
   the lzo1x crate decoder; state is 0=A, 1=B, 2=C. */
static unsigned lzo1x_decode(const unsigned char *in, unsigned in_len,
                             unsigned char *out, unsigned out_cap) {
    unsigned i = 8, o = 0, state, insn, len, dist, tail;
    if (in_len < 8 || in[4] || in[5] || in[6] || in[7]) return 0;
    unsigned want = (unsigned)in[0] | ((unsigned)in[1]<<8) |
                    ((unsigned)in[2]<<16) | ((unsigned)in[3]<<24);
    if (want > out_cap || i >= in_len) return 0;
    insn = in[i];
    if (insn <= 17) state = 0;
    else {
        i++; len = insn - 17u;
        if (len > in_len - i || len > want - o) return 0;
        while (len--) out[o++] = in[i++];
        state = insn <= 20 ? 1u : 2u;
    }
    for (;;) {
        if (i >= in_len) return 0;
        insn = in[i++];
        if (insn <= 15) {
            if (state == 0) {
                if (insn == 0) {
                    len = 18;
                    while (i < in_len && in[i] == 0) {
                        if (len > 0xffffffffu - 255u) return 0;
                        len += 255; i++;
                    }
                    if (i >= in_len || len > 0xffffffffu - in[i]) return 0;
                    len += in[i++];
                } else len = insn + 3u;
                if (len > in_len - i || len > want - o) return 0;
                while (len--) out[o++] = in[i++];
                state = 2; continue;
            }
            len = state == 1 ? 2u : 3u;
            if (i >= in_len) return 0;
            dist = ((unsigned)in[i++] << 2) + (insn >> 2) + (state == 1 ? 1u : 2049u);
            tail = insn;
        } else if (insn <= 31) {
            len = insn & 7u;
            if (!len) {
                len = 9;
                while (i < in_len && in[i] == 0) {
                    if (len > 0xffffffffu - 255u) return 0;
                    len += 255; i++;
                }
                if (i >= in_len || len > 0xffffffffu - in[i]) return 0;
                len += in[i++];
            } else len += 2;
            if (i + 2 > in_len) return 0;
            tail = in[i];
            dist = (((insn & 8u) >> 3) << 14) + ((unsigned)in[i+1] << 6) +
                   (in[i] >> 2) + 16384u;
            i += 2;
            if (dist == 16384u)
                return i == in_len && o == want ? o : 0;
        } else if (insn <= 63) {
            len = insn & 31u;
            if (!len) {
                len = 33;
                while (i < in_len && in[i] == 0) {
                    if (len > 0xffffffffu - 255u) return 0;
                    len += 255; i++;
                }
                if (i >= in_len || len > 0xffffffffu - in[i]) return 0;
                len += in[i++];
            } else len += 2;
            if (i + 2 > in_len) return 0;
            tail = in[i]; dist = ((unsigned)in[i+1] << 6) + (in[i] >> 2) + 1u; i += 2;
        } else {
            len = insn < 128 ? ((insn & 32u) ? 4u : 3u) : ((insn & 96u) >> 5) + 5u;
            if (i >= in_len) return 0;
            dist = ((unsigned)in[i++] << 3) + ((insn & 28u) >> 2) + 1u;
            tail = insn;
        }
        if (!dist || dist > o || len > want - o) return 0;
        while (len--) { out[o] = out[o - dist]; o++; }
        len = tail & 3u;
        if (len > in_len - i || len > want - o) return 0;
        if (!len) state = 0;
        else {
            while (len--) out[o++] = in[i++];
            state = 1;
        }
    }
}
"#;

const C_NRV: &str = r#"
typedef struct {
    const unsigned char *in; unsigned len, pos, bits;
} nrv_bits;
static int nrv_bit(nrv_bits *s, unsigned *v) {
    if ((s->bits & 127u) == 0) {
        if (s->pos >= s->len) return 0;
        s->bits = (unsigned)s->in[s->pos++] * 2u + 1u;
    } else s->bits *= 2u;
    *v = (s->bits >> 8) & 1u;
    return 1;
}
/* kind 0/1/2 selects NRV2B/2D/2E. Input has magic + u32le size. */
static unsigned nrv_decode(const unsigned char *in, unsigned in_len,
                           unsigned char *out, unsigned out_cap, unsigned kind) {
    unsigned want, o = 0, last = 1;
    nrv_bits s;
    if (in_len < 8) return 0;
    want = (unsigned)in[4] | ((unsigned)in[5]<<8) |
           ((unsigned)in[6]<<16) | ((unsigned)in[7]<<24);
    if (want > out_cap) return 0;
    s.in = in + 8; s.len = in_len - 8; s.pos = 0; s.bits = 0;
    for (;;) {
        unsigned b, off = 1, ml, total;
        for (;;) {
            if (!nrv_bit(&s, &b)) return 0;
            if (!b) break;
            if (s.pos >= s.len || o >= want) return 0;
            out[o++] = s.in[s.pos++];
        }
        if (kind == 0) {
            do {
                if (!nrv_bit(&s, &b) || off > 0x7fffffffu) return 0;
                off = off * 2u + b;
                if (!nrv_bit(&s, &b) || off > 0x01000002u) return 0;
            } while (!b);
        } else {
            for (;;) {
                if (!nrv_bit(&s, &b) || off > 0x7fffffffu) return 0;
                off = off * 2u + b;
                if (!nrv_bit(&s, &b) || off > 0x01000002u) return 0;
                if (b) break;
                if (!nrv_bit(&s, &b) || off < 1 || off - 1u > 0x7fffffffu) return 0;
                off = (off - 1u) * 2u + b;
            }
        }
        if (off == 2) {
            off = last;
            if (kind && !nrv_bit(&s, &ml)) return 0;
        } else {
            unsigned x;
            if (s.pos >= s.len) return 0;
            x = (off - 3u) * 256u + s.in[s.pos++];
            if (x == 0xffffffffu)
                return s.pos == s.len && o == want ? o : 0;
            if (kind) { ml = (~x) & 1u; x >>= 1; }
            off = x + 1u; last = off;
        }
        if (kind == 0) {
            if (!nrv_bit(&s, &ml) || !nrv_bit(&s, &b)) return 0;
            ml = ml * 2u + b;
            if (!ml) {
                ml = 1;
                do {
                    if (!nrv_bit(&s, &b) || ml > 0x7fffffffu) return 0;
                    ml = ml * 2u + b;
                    if (!nrv_bit(&s, &b)) return 0;
                } while (!b);
                ml += 2;
            }
            ml += off > 0xd00u;
        } else if (kind == 1) {
            if (!nrv_bit(&s, &b)) return 0;
            ml = ml * 2u + b;
            if (!ml) {
                ml = 1;
                do {
                    if (!nrv_bit(&s, &b) || ml > 0x7fffffffu) return 0;
                    ml = ml * 2u + b;
                    if (!nrv_bit(&s, &b)) return 0;
                } while (!b);
                ml += 2;
            }
            ml += off > 0x500u;
        } else {
            if (ml) {
                if (!nrv_bit(&s, &b)) return 0;
                ml = 1u + b;
            } else {
                if (!nrv_bit(&s, &b)) return 0;
                if (b) {
                    if (!nrv_bit(&s, &b)) return 0;
                    ml = 3u + b;
                } else {
                    ml = 1;
                    do {
                        if (!nrv_bit(&s, &b) || ml > 0x7fffffffu) return 0;
                        ml = ml * 2u + b;
                        if (!nrv_bit(&s, &b)) return 0;
                    } while (!b);
                    ml += 3;
                }
            }
            ml += off > 0x500u;
        }
        total = ml + 1u;
        if (!off || off > o || total > want - o) return 0;
        while (total--) { out[o] = out[o - off]; o++; }
    }
}
#define nrv2b_decode(in,n,out,cap) \
    ((n)>=4 && (in)[0]=='N' && (in)[1]=='R' && (in)[2]=='V' && (in)[3]=='B' ? \
     nrv_decode((in),(n),(out),(cap),0) : 0)
#define nrv2d_decode(in,n,out,cap) \
    ((n)>=4 && (in)[0]=='N' && (in)[1]=='R' && (in)[2]=='V' && (in)[3]=='D' ? \
     nrv_decode((in),(n),(out),(cap),1) : 0)
#define nrv2e_decode(in,n,out,cap) \
    ((n)>=4 && (in)[0]=='N' && (in)[1]=='R' && (in)[2]=='V' && (in)[3]=='E' ? \
     nrv_decode((in),(n),(out),(cap),2) : 0)
"#;

const C_LZSA2: &str = r#"
typedef struct {
    const unsigned char *in; unsigned end, pos, pending; unsigned char nibbles;
} lzsa2_state;
static int lzsa2_nibble(lzsa2_state *s, unsigned *v) {
    s->pending ^= 1u;
    if (s->pending) {
        if (s->pos >= s->end) return 0;
        s->nibbles = s->in[s->pos++]; *v = s->nibbles >> 4;
    } else *v = s->nibbles & 15u;
    return 1;
}
static int lzsa2_len(lzsa2_state *s, unsigned *len) {
    unsigned v;
    if (!lzsa2_nibble(s, &v) || *len > 0xffffffffu - v) return 0;
    *len += v;
    if (v == 15) {
        if (s->pos >= s->end || *len > 0xffffffffu - s->in[s->pos]) return 0;
        *len += s->in[s->pos++];
        if (*len == 257) {
            if (s->end - s->pos < 2) return 0;
            *len = (unsigned)s->in[s->pos] | ((unsigned)s->in[s->pos+1] << 8);
            s->pos += 2;
        } else if (*len == 256) *len = 0;
    }
    return 1;
}
/* Expand one framed LZSA2 block. base is the start of its available dictionary. */
static int lzsa2_block(const unsigned char *in, unsigned in_len,
                       unsigned char *out, unsigned base, unsigned start,
                       unsigned cap, unsigned *written) {
    lzsa2_state s = { in, in_len, 0, 0, 0 };
    unsigned o = start, last = 0;
    while (s.pos < s.end) {
        unsigned token = s.in[s.pos++], lit = (token >> 3) & 3u;
        unsigned mode, v, dist, ml;
        if (lit == 3 && !lzsa2_len(&s, &lit)) return 0;
        if (lit > s.end - s.pos || lit > cap - o) return 0;
        while (lit--) out[base + o++] = s.in[s.pos++];
        if (s.pos == s.end) break;
        mode = token & 0xc0u;
        if (mode == 0) {
            if (!lzsa2_nibble(&s, &v)) return 0;
            dist = ((v << 1) | ((token & 32u) >> 5)) ^ 30u;
            dist++;
        } else if (mode == 0x40u) {
            if (s.pos >= s.end) return 0;
            dist = ((unsigned)s.in[s.pos++] | ((token & 32u) << 3)) ^ 255u;
            dist++;
        } else if (mode == 0x80u) {
            if (!lzsa2_nibble(&s, &v) || s.pos >= s.end) return 0;
            dist = ((unsigned)s.in[s.pos++] | (v << 9) | ((token & 32u) << 3)) ^ 0x1effu;
            dist += 513u;
        } else if (!(token & 32u)) {
            if (s.end - s.pos < 2) return 0;
            dist = (((unsigned)s.in[s.pos] << 8) | s.in[s.pos+1]) ^ 0xffffu;
            s.pos += 2; dist++;
        } else dist = last;
        last = dist;
        ml = token & 7u;
        if (ml == 7) {
            ml += 2;
            if (!lzsa2_len(&s, &ml)) return 0;
            if (!ml) break;
        } else ml += 2;
        if (!dist || dist > o || ml > cap - o) return 0;
        while (ml--) { out[base + o] = out[base + o - dist]; o++; }
    }
    *written = o - start;
    return 1;
}
/* "LSA2", u32le output size, then an LZSA2 framed stream. */
static unsigned lzsa2_decode(const unsigned char *in, unsigned in_len,
                             unsigned char *out, unsigned out_cap) {
    unsigned want, i = 11, o = 0, previous = 0;
    if (in_len < 11 || in[0]!='L' || in[1]!='S' || in[2]!='A' || in[3]!='2')
        return 0;
    want = (unsigned)in[4] | ((unsigned)in[5]<<8) |
           ((unsigned)in[6]<<16) | ((unsigned)in[7]<<24);
    if (want > out_cap || in[8] != 0x7b || in[9] != 0x9e || in[10] != 0x20)
        return 0;
    while (i < in_len) {
        unsigned n, raw, got, base;
        if (in_len - i < 3) return 0;
        n = (unsigned)in[i] | ((unsigned)in[i+1]<<8) | ((unsigned)(in[i+2]&127u)<<16);
        raw = in[i+2] & 128u; i += 3;
        if (!n) return i == in_len && o == want ? o : 0;
        if (n > in_len - i) return 0;
        if (raw) {
            if (n > want - o) return 0;
            for (unsigned k = 0; k < n; k++) out[o++] = in[i+k];
        } else {
            if (previous > o) return 0;
            base = o - previous;
            if (!lzsa2_block(in + i, n, out, base, previous, want - base, &got))
                return 0;
            o += got; previous = got;
        }
        i += n;
    }
    return 0;
}
"#;

const C_LITRUN: &str = r#"
/* Literal-run tokens: [0][len][bytes...] | [1][dist u16be][len u8] */
static unsigned litrun_decode(const unsigned char *in, unsigned in_len,
                              unsigned char *out, unsigned out_cap) {
    unsigned i = 0, o = 0;
    while (i < in_len) {
        unsigned char tag = in[i++];
        if (tag == 0) {
            if (i >= in_len) return 0;
            unsigned len = in[i++];
            if (i + len > in_len || o + len > out_cap) return 0;
            for (unsigned k = 0; k < len; k++) out[o++] = in[i++];
        } else if (tag == 1) {
            if (i + 2 >= in_len) return 0;
            unsigned dist = ((unsigned)in[i] << 8) | in[i+1];
            unsigned len = in[i+2]; i += 3;
            if (dist == 0 || dist > o || o + len > out_cap) return 0;
            for (unsigned k = 0; k < len; k++) { out[o] = out[o - dist]; o++; }
        } else return 0;
    }
    return o;
}
"#;

const C_TOKENS: &str = r#"
/* Verbose tokens: [0][byte] | [1][dist u16be][len u8] */
static unsigned tokens_decode(const unsigned char *in, unsigned in_len,
                              unsigned char *out, unsigned out_cap) {
    unsigned i = 0, o = 0;
    while (i < in_len) {
        unsigned char tag = in[i++];
        if (tag == 0) {
            if (i >= in_len || o >= out_cap) return 0;
            out[o++] = in[i++];
        } else if (tag == 1) {
            if (i + 2 >= in_len) return 0;
            unsigned dist = ((unsigned)in[i] << 8) | in[i+1];
            unsigned len = in[i+2]; i += 3;
            if (dist == 0 || dist > o || o + len > out_cap) return 0;
            for (unsigned k = 0; k < len; k++) { out[o] = out[o - dist]; o++; }
        } else return 0;
    }
    return o;
}
"#;

const C_SHORTDIST: &str = r#"
/* Short-distance tokens: [0][byte] | [2][dist u8][len u8] | [1][dist u16be][len u8] */
static unsigned shortdist_decode(const unsigned char *in, unsigned in_len,
                                 unsigned char *out, unsigned out_cap) {
    unsigned i = 0, o = 0;
    while (i < in_len) {
        unsigned char tag = in[i++];
        unsigned dist, len;
        if (tag == 0) {
            if (i >= in_len || o >= out_cap) return 0;
            out[o++] = in[i++];
            continue;
        } else if (tag == 2) {
            if (i + 1 >= in_len) return 0;
            dist = in[i]; len = in[i+1]; i += 2;
        } else if (tag == 1) {
            if (i + 2 >= in_len) return 0;
            dist = ((unsigned)in[i] << 8) | in[i+1]; len = in[i+2]; i += 3;
        } else return 0;
        if (dist == 0 || dist > o || o + len > out_cap) return 0;
        for (unsigned k = 0; k < len; k++) { out[o] = out[o - dist]; o++; }
    }
    return o;
}
"#;

const C_ZERO_RLE: &str = r#"
/* Zero-run RLE: ESC count -> count zeros; ESC 0 -> literal ESC. */
static unsigned zero_rle_decode(const unsigned char *in, unsigned in_len,
                                unsigned char *out, unsigned out_cap) {
    unsigned i = 0, o = 0;
    while (i < in_len) {
        if (in[i] == ESC_BYTE) {
            if (i + 1 >= in_len) return 0;
            unsigned c = in[i+1]; i += 2;
            if (c == 0) { if (o >= out_cap) return 0; out[o++] = ESC_BYTE; }
            else { if (o + c > out_cap) return 0; for (unsigned k = 0; k < c; k++) out[o++] = 0; }
        } else {
            if (o >= out_cap) return 0;
            out[o++] = in[i++];
        }
    }
    return o;
}
"#;

const C_RLE_ESCAPE: &str = r#"
/* Escape RLE: ESC 0 -> literal ESC; ESC count byte -> count copies. */
static unsigned rle_escape_decode(const unsigned char *in, unsigned in_len,
                                  unsigned char *out, unsigned out_cap) {
    unsigned i = 0, o = 0;
    while (i < in_len) {
        if (in[i] == ESC_BYTE) {
            if (i + 1 >= in_len) return 0;
            unsigned c = in[i+1];
            if (c == 0) { if (o >= out_cap) return 0; out[o++] = ESC_BYTE; i += 2; }
            else {
                if (i + 2 >= in_len || o + c > out_cap) return 0;
                unsigned char v = in[i+2]; i += 3;
                for (unsigned k = 0; k < c; k++) out[o++] = v;
            }
        } else {
            if (o >= out_cap) return 0;
            out[o++] = in[i++];
        }
    }
    return o;
}
"#;

const C_PACKBITS: &str = r#"
/* PackBits: n<128 -> n+1 literals; n>128 -> 257-n copies; 128 -> skip. */
static unsigned packbits_decode(const unsigned char *in, unsigned in_len,
                                unsigned char *out, unsigned out_cap) {
    unsigned i = 0, o = 0;
    while (i < in_len) {
        int t = in[i++];
        if (t == 128) continue;
        if (t < 128) {
            unsigned len = (unsigned)t + 1;
            if (i + len > in_len || o + len > out_cap) return 0;
            for (unsigned k = 0; k < len; k++) out[o++] = in[i++];
        } else {
            unsigned len = 257u - (unsigned)t;
            if (i >= in_len || o + len > out_cap) return 0;
            unsigned char v = in[i++];
            for (unsigned k = 0; k < len; k++) out[o++] = v;
        }
    }
    return o;
}
"#;

const C_DPCM: &str = r#"
/* DPCM: first byte literal, each later byte a delta from the previous. */
static unsigned dpcm_decode(const unsigned char *in, unsigned in_len,
                            unsigned char *out, unsigned out_cap) {
    if (in_len > out_cap) return 0;
    unsigned char prev = 0;
    for (unsigned i = 0; i < in_len; i++) {
        unsigned char v = (i == 0) ? in[i] : (unsigned char)(in[i] + prev);
        out[i] = v; prev = v;
    }
    return in_len;
}
"#;

const C_XOR: &str = r#"
/* XOR-with-previous: first byte literal, each later byte XORed with the previous. */
static unsigned xor_prev_decode(const unsigned char *in, unsigned in_len,
                                unsigned char *out, unsigned out_cap) {
    if (in_len > out_cap) return 0;
    unsigned char prev = 0;
    for (unsigned i = 0; i < in_len; i++) {
        unsigned char v = (i == 0) ? in[i] : (unsigned char)(in[i] ^ prev);
        out[i] = v; prev = v;
    }
    return in_len;
}
"#;

const C_DELTA16: &str = r#"
/* Little-endian 16-bit delta transform; a trailing odd byte is literal. */
static unsigned delta16_decode(const unsigned char *in, unsigned in_len,
                               unsigned char *out, unsigned out_cap) {
    if (in_len > out_cap) return 0;
    if (in_len < 2) {
        for (unsigned i = 0; i < in_len; i++) out[i] = in[i];
        return in_len;
    }
    out[0] = in[0]; out[1] = in[1];
    unsigned prev = (unsigned)in[0] | ((unsigned)in[1] << 8), i = 2;
    while (i + 1 < in_len) {
        unsigned delta = (unsigned)in[i] | ((unsigned)in[i + 1] << 8);
        prev = (prev + delta) & 0xffffu;
        out[i] = (unsigned char)prev; out[i + 1] = (unsigned char)(prev >> 8);
        i += 2;
    }
    if (i < in_len) out[i] = in[i];
    return in_len;
}
"#;

const C_RLE_PAIR: &str = r#"
/* Byte pairs [count, value]. */
static unsigned rle_pair_decode(const unsigned char *in, unsigned in_len,
                                unsigned char *out, unsigned out_cap) {
    if (in_len & 1u) return 0;
    unsigned o = 0;
    for (unsigned i = 0; i < in_len; i += 2) {
        unsigned n = in[i];
        if (n == 0 || o + n > out_cap) return 0;
        while (n--) out[o++] = in[i + 1];
    }
    return o;
}
"#;

const C_MTF: &str = r#"
/* Move-to-front indices over the initial byte alphabet 0..255. */
static unsigned mtf_decode(const unsigned char *in, unsigned in_len,
                           unsigned char *out, unsigned out_cap) {
    if (in_len > out_cap) return 0;
    unsigned char table[256];
    for (unsigned i = 0; i < 256; i++) table[i] = (unsigned char)i;
    for (unsigned i = 0; i < in_len; i++) {
        unsigned idx = in[i]; unsigned char b = table[idx];
        out[i] = b;
        while (idx) { table[idx] = table[idx - 1]; idx--; }
        table[0] = b;
    }
    return in_len;
}
"#;

const C_BITPLANE: &str = r#"
/* "BP8\0", u32be original length, then eight MSB-first bit planes. */
static unsigned bitplane_decode(const unsigned char *in, unsigned in_len,
                                unsigned char *out, unsigned out_cap) {
    if (in_len < 8 || in[0]!='B' || in[1]!='P' || in[2]!='8' || in[3]!=0) return 0;
    unsigned n = ((unsigned)in[4]<<24) | ((unsigned)in[5]<<16) |
                 ((unsigned)in[6]<<8) | in[7];
    unsigned plane = (n + 7u) / 8u;
    if (n > out_cap || plane > (in_len - 8u) / 8u) return 0;
    for (unsigned i = 0; i < n; i++) out[i] = 0;
    for (unsigned bit = 0; bit < 8; bit++) {
        unsigned base = 8u + bit * plane;
        for (unsigned i = 0; i < n; i++)
            out[i] |= (unsigned char)(((in[base + i / 8u] >> (7u - (i & 7u))) & 1u) << bit);
    }
    return n;
}
"#;

const C_BPE: &str = r#"
/* "BPE1", rule count, [symbol,left,right] rules, then the substituted stream. */
static unsigned bpe_decode(const unsigned char *in, unsigned in_len,
                           unsigned char *out, unsigned out_cap) {
    if (in_len < 5 || in[0]!='B' || in[1]!='P' || in[2]!='E' || in[3]!='1') return 0;
    unsigned count = in[4], body = 5u + count * 3u;
    if (body > in_len) return 0;
    unsigned char is_rule[256] = {0}, left[256], right[256];
    for (unsigned r = 0; r < count; r++) {
        unsigned p = 5u + r * 3u, s = in[p];
        is_rule[s] = 1; left[s] = in[p + 1]; right[s] = in[p + 2];
    }
    unsigned o = 0;
    for (unsigned i = body; i < in_len; i++) {
        unsigned char stack[256]; unsigned sp = 0;
        stack[sp++] = in[i];
        while (sp) {
            unsigned char c = stack[--sp];
            if (is_rule[c]) {
                if (sp > 253) return 0;
                stack[sp++] = right[c]; stack[sp++] = left[c];
            } else {
                if (o >= out_cap) return 0;
                out[o++] = c;
            }
        }
    }
    return o;
}
"#;

const C_VARINT: &str = r#"
static int sprism_varint(const unsigned char *in, unsigned in_len,
                         unsigned *pos, unsigned *value) {
    unsigned v = 0, shift = 0;
    for (;;) {
        if (*pos >= in_len || shift >= 32) return 0;
        unsigned b = in[(*pos)++];
        v |= (b & 127u) << shift;
        if (!(b & 128u)) { *value = v; return 1; }
        shift += 7;
    }
}
/* [0,literal] or [1,varint distance,varint length]. */
static unsigned varint_decode(const unsigned char *in, unsigned in_len,
                              unsigned char *out, unsigned out_cap) {
    unsigned i = 0, o = 0;
    while (i < in_len) {
        unsigned tag = in[i++];
        if (tag == 0) {
            if (i >= in_len || o >= out_cap) return 0;
            out[o++] = in[i++];
        } else if (tag == 1) {
            unsigned dist, len;
            if (!sprism_varint(in, in_len, &i, &dist) ||
                !sprism_varint(in, in_len, &i, &len) ||
                dist == 0 || dist > o || o + len > out_cap) return 0;
            while (len--) { out[o] = out[o - dist]; o++; }
        } else return 0;
    }
    return o;
}
"#;

const C_ASCII_TOKENS: &str = r#"
static int sprism_hex(unsigned char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}
/* Printable Lhh literals and Mddddll matches. */
static unsigned ascii_tokens_decode(const unsigned char *in, unsigned in_len,
                                    unsigned char *out, unsigned out_cap) {
    unsigned i = 0, o = 0;
    while (i < in_len) {
        if (in[i] == 'L') {
            if (i + 2 >= in_len || o >= out_cap) return 0;
            int a = sprism_hex(in[i + 1]), b = sprism_hex(in[i + 2]);
            if (a < 0 || b < 0) return 0;
            out[o++] = (unsigned char)((a << 4) | b); i += 3;
        } else if (in[i] == 'M') {
            if (i + 6 >= in_len) return 0;
            unsigned dist = 0, len = 0;
            for (unsigned k = 1; k <= 4; k++) {
                int h = sprism_hex(in[i + k]); if (h < 0) return 0;
                dist = (dist << 4) | (unsigned)h;
            }
            for (unsigned k = 5; k <= 6; k++) {
                int h = sprism_hex(in[i + k]); if (h < 0) return 0;
                len = (len << 4) | (unsigned)h;
            }
            if (dist == 0 || dist > o || o + len > out_cap) return 0;
            while (len--) { out[o] = out[o - dist]; o++; }
            i += 7;
        } else return 0;
    }
    return o;
}
"#;

const C_RANS: &str = r#"
#define SPRISM_RANS_TOTAL 4096u
#define SPRISM_RANS_L (1u << 23)
typedef struct {
    const unsigned char *in; unsigned in_len, pos, remaining, state;
    unsigned short freq[256], cum[257]; unsigned char slot[SPRISM_RANS_TOTAL];
} sprism_rans;

static int sprism_rans_init(sprism_rans *r, const unsigned char *in, unsigned in_len) {
    r->in = in; r->in_len = in_len; r->pos = 0; r->remaining = 0; r->state = 0;
    if (in_len == 4 && !(in[0] | in[1] | in[2] | in[3])) return 1;
    if (in_len < 520) return 0;
    r->remaining = (unsigned)in[0] | ((unsigned)in[1]<<8) |
                   ((unsigned)in[2]<<16) | ((unsigned)in[3]<<24);
    r->cum[0] = 0;
    for (unsigned s = 0; s < 256; s++) {
        unsigned p = 4u + s * 2u;
        r->freq[s] = (unsigned short)((unsigned)in[p] | ((unsigned)in[p + 1]<<8));
        r->cum[s + 1] = (unsigned short)(r->cum[s] + r->freq[s]);
    }
    if (r->cum[256] != SPRISM_RANS_TOTAL) return 0;
    for (unsigned s = 0; s < 256; s++)
        for (unsigned q = r->cum[s]; q < r->cum[s + 1]; q++) r->slot[q] = (unsigned char)s;
    r->state = (unsigned)in[516] | ((unsigned)in[517]<<8) |
               ((unsigned)in[518]<<16) | ((unsigned)in[519]<<24);
    r->pos = 520;
    return 1;
}

static int sprism_rans_byte(sprism_rans *r, unsigned char *value) {
    if (!r->remaining) return 0;
    unsigned q = r->state & (SPRISM_RANS_TOTAL - 1u), sym = r->slot[q];
    *value = (unsigned char)sym;
    r->state = (unsigned)r->freq[sym] * (r->state >> 12) + q - r->cum[sym];
    while (r->state < SPRISM_RANS_L) {
        if (r->pos >= r->in_len) return 0;
        r->state = (r->state << 8) | r->in[r->pos++];
    }
    r->remaining--;
    return 1;
}

static unsigned rans_decode(const unsigned char *in, unsigned in_len,
                            unsigned char *out, unsigned out_cap) {
    sprism_rans r;
    if (!sprism_rans_init(&r, in, in_len) || r.remaining > out_cap) return 0;
    unsigned n = r.remaining;
    for (unsigned i = 0; i < n; i++) if (!sprism_rans_byte(&r, &out[i])) return 0;
    return n;
}

static unsigned dpcm_rans_decode(const unsigned char *in, unsigned in_len,
                                 unsigned char *out, unsigned out_cap) {
    unsigned n = rans_decode(in, in_len, out, out_cap);
    if (!n && out_cap) return 0;
    for (unsigned i = 1; i < n; i++) out[i] = (unsigned char)(out[i] + out[i - 1]);
    return n;
}

static unsigned mtf_rans_decode(const unsigned char *in, unsigned in_len,
                                unsigned char *out, unsigned out_cap) {
    unsigned n = rans_decode(in, in_len, out, out_cap);
    if (!n && out_cap) return 0;
    unsigned char table[256];
    for (unsigned i = 0; i < 256; i++) table[i] = (unsigned char)i;
    for (unsigned i = 0; i < n; i++) {
        unsigned idx = out[i]; unsigned char b = table[idx]; out[i] = b;
        while (idx) { table[idx] = table[idx - 1]; idx--; }
        table[0] = b;
    }
    return n;
}

static unsigned bitplane_rans_decode(const unsigned char *in, unsigned in_len,
                                     unsigned char *out, unsigned out_cap) {
    sprism_rans r; unsigned char h[8];
    if (!sprism_rans_init(&r, in, in_len) || r.remaining < 8) return 0;
    for (unsigned i = 0; i < 8; i++) if (!sprism_rans_byte(&r, &h[i])) return 0;
    if (h[0]!='B' || h[1]!='P' || h[2]!='8' || h[3]!=0) return 0;
    unsigned n = ((unsigned)h[4]<<24) | ((unsigned)h[5]<<16) | ((unsigned)h[6]<<8) | h[7];
    unsigned plane = (n + 7u) / 8u;
    if (n > out_cap || r.remaining != plane * 8u) return 0;
    for (unsigned i = 0; i < n; i++) out[i] = 0;
    for (unsigned bit = 0; bit < 8; bit++) {
        for (unsigned p = 0; p < plane; p++) {
            unsigned char v; if (!sprism_rans_byte(&r, &v)) return 0;
            for (unsigned k = 0; k < 8; k++) {
                unsigned i = p * 8u + k;
                if (i < n) out[i] |= (unsigned char)(((v >> (7u - k)) & 1u) << bit);
            }
        }
    }
    return n;
}
#undef SPRISM_RANS_TOTAL
#undef SPRISM_RANS_L
"#;

const C_HEATSHRINK: &str = r#"
/* Heatshrink-compatible stream: fixed window=8 and lookahead=4, wrapped by
   "HSK1" and a u32le output length. */
static unsigned heatshrink_decode(const unsigned char *in, unsigned in_len,
                                  unsigned char *out, unsigned out_cap) {
    if (in_len < 8 || in[0]!='H' || in[1]!='S' || in[2]!='K' || in[3]!='1') return 0;
    unsigned want = (unsigned)in[4] | ((unsigned)in[5]<<8) |
                    ((unsigned)in[6]<<16) | ((unsigned)in[7]<<24);
    if (want > out_cap) return 0;
    unsigned bit = 64, o = 0;
#define HS_BITS(dst, count) do { \
    (dst) = 0; \
    for (unsigned hs_i = 0; hs_i < (count); hs_i++) { \
        if (bit >= in_len * 8u) return 0; \
        (dst) = ((dst) << 1) | ((in[bit >> 3] >> (7u - (bit & 7u))) & 1u); \
        bit++; \
    } \
} while (0)
    while (o < want) {
        unsigned tag, value, len;
        HS_BITS(tag, 1);
        if (tag) {
            HS_BITS(value, 8);
            out[o++] = (unsigned char)value;
        } else {
            HS_BITS(value, 8);
            HS_BITS(len, 4);
            value++; len++;
            if (value > o || o + len > want) return 0;
            while (len--) { out[o] = out[o - value]; o++; }
        }
    }
#undef HS_BITS
    return o;
}
"#;

const C_INFLATE: &str = r#"
/* Inflate (RFC 1951) inside a zlib wrapper (RFC 1950), the format every
   deflate-family codec here emits. Bit-serial Huffman as in zlib's puff.c.
   The Adler-32 trailer is not checked; the FNV checksum over the output covers it. */
typedef struct {
    const unsigned char *in; unsigned in_len, pos, bitbuf; int bitcnt, err;
} inf_state;
typedef struct { unsigned short count[16], symbol[288]; } inf_huff;

static unsigned inf_bits(inf_state *s, int n) {
    unsigned v = 0;
    for (int i = 0; i < n; i++) {
        if (s->bitcnt == 0) {
            if (s->pos >= s->in_len) { s->err = 1; return 0; }
            s->bitbuf = s->in[s->pos++]; s->bitcnt = 8;
        }
        v |= (s->bitbuf & 1u) << i; s->bitbuf >>= 1; s->bitcnt--;
    }
    return v;
}

/* Canonical code from lengths; returns nonzero on an over-subscribed set. */
static int inf_build(inf_huff *h, const unsigned char *len, int n) {
    unsigned short offs[16]; int i, left = 1;
    for (i = 0; i < 16; i++) h->count[i] = 0;
    for (i = 0; i < n; i++) h->count[len[i]]++;
    h->count[0] = 0;
    for (i = 1; i < 16; i++) { left = (left << 1) - h->count[i]; if (left < 0) return 1; }
    offs[1] = 0;
    for (i = 1; i < 15; i++) offs[i + 1] = offs[i] + h->count[i];
    for (i = 0; i < n; i++) if (len[i]) h->symbol[offs[len[i]]++] = (unsigned short)i;
    return 0;
}

static int inf_decode(inf_state *s, const inf_huff *h) {
    int code = 0, first = 0, index = 0;
    for (int l = 1; l < 16; l++) {
        code |= (int)inf_bits(s, 1);
        int count = h->count[l];
        if (code - count < first) return h->symbol[index + (code - first)];
        index += count; first += count; first <<= 1; code <<= 1;
    }
    s->err = 1; return 0;
}

static unsigned zlib_decode(const unsigned char *in, unsigned in_len,
                            unsigned char *out, unsigned out_cap) {
    static const unsigned short lbase[29] = {3,4,5,6,7,8,9,10,11,13,15,17,19,23,27,31,
        35,43,51,59,67,83,99,115,131,163,195,227,258};
    static const unsigned char  lext[29]  = {0,0,0,0,0,0,0,0,1,1,1,1,2,2,2,2,3,3,3,3,4,4,4,4,5,5,5,5,0};
    static const unsigned short dbase[30] = {1,2,3,4,5,7,9,13,17,25,33,49,65,97,129,193,257,385,
        513,769,1025,1537,2049,3073,4097,6145,8193,12289,16385,24577};
    static const unsigned char  dext[30]  = {0,0,0,0,1,1,2,2,3,3,4,4,5,5,6,6,7,7,8,8,9,9,10,10,11,11,12,12,13,13};
    static const unsigned char  order[19] = {16,17,18,0,8,7,9,6,10,5,11,4,12,3,13,2,14,1,15};
    inf_state s = { in, in_len, 2, 0, 0, 0 };   /* skip CMF/FLG */
    inf_huff lc, dc;
    unsigned char lens[320];
    unsigned o = 0;
    int last;
    if (in_len < 2 || (in[0] & 0x0f) != 8 || (in[1] & 0x20)) return 0;
    do {
        last = (int)inf_bits(&s, 1);
        int type = (int)inf_bits(&s, 2);
        if (type == 0) {
            s.bitcnt = 0;
            if (s.pos + 4 > in_len) return 0;
            unsigned n = in[s.pos] | ((unsigned)in[s.pos + 1] << 8);
            unsigned nn = in[s.pos + 2] | ((unsigned)in[s.pos + 3] << 8);
            if (nn != (~n & 0xffffu)) return 0;
            s.pos += 4;
            if (s.pos + n > in_len || o + n > out_cap) return 0;
            for (unsigned k = 0; k < n; k++) out[o++] = in[s.pos++];
            continue;
        }
        if (type == 1) {
            int i = 0;
            for (; i < 144; i++) lens[i] = 8;
            for (; i < 256; i++) lens[i] = 9;
            for (; i < 280; i++) lens[i] = 7;
            for (; i < 288; i++) lens[i] = 8;
            inf_build(&lc, lens, 288);
            for (i = 0; i < 30; i++) lens[i] = 5;
            inf_build(&dc, lens, 30);
        } else if (type == 2) {
            int nlen = (int)inf_bits(&s, 5) + 257, ndist = (int)inf_bits(&s, 5) + 1;
            int ncode = (int)inf_bits(&s, 4) + 4, i;
            if (nlen > 286 || ndist > 30) return 0;
            for (i = 0; i < 19; i++) lens[i] = 0;
            for (i = 0; i < ncode; i++) lens[order[i]] = (unsigned char)inf_bits(&s, 3);
            if (inf_build(&lc, lens, 19)) return 0;
            for (i = 0; i < nlen + ndist;) {
                int sym = inf_decode(&s, &lc);
                if (s.err) return 0;
                if (sym < 16) { lens[i++] = (unsigned char)sym; continue; }
                int rep, val = 0;
                if (sym == 16) { if (i == 0) return 0; val = lens[i - 1]; rep = 3 + (int)inf_bits(&s, 2); }
                else if (sym == 17) rep = 3 + (int)inf_bits(&s, 3);
                else rep = 11 + (int)inf_bits(&s, 7);
                if (i + rep > nlen + ndist) return 0;
                while (rep--) lens[i++] = (unsigned char)val;
            }
            if (lens[256] == 0 || inf_build(&lc, lens, nlen) || inf_build(&dc, lens + nlen, ndist)) return 0;
        } else {
            return 0;
        }
        for (;;) {
            int sym = inf_decode(&s, &lc);
            if (s.err) return 0;
            if (sym < 256) { if (o >= out_cap) return 0; out[o++] = (unsigned char)sym; continue; }
            if (sym == 256) break;
            sym -= 257;
            if (sym >= 29) return 0;
            unsigned len = lbase[sym] + inf_bits(&s, lext[sym]);
            int dsym = inf_decode(&s, &dc);
            if (s.err || dsym >= 30) return 0;
            unsigned dist = dbase[dsym] + inf_bits(&s, dext[dsym]);
            if (s.err || dist > o || o + len > out_cap) return 0;
            for (unsigned k = 0; k < len; k++) { out[o] = out[o - dist]; o++; }
        }
    } while (!last);
    return s.err ? 0 : o;
}
"#;

/// The C template and decoder function for a codec, or None when it needs a library; deflate-family codecs share one.
fn templateFor(codec: &str) -> Option<(&'static str, &'static str)>
{
    Some(match registry::find(codec)?.c_decoder?
    {
        CDecoder::Lzss => (C_LZSS, "lzss_decode"),
        CDecoder::Lz4ish => (C_LZ4ISH, "lz4ish_decode"),
        CDecoder::Litrun => (C_LITRUN, "litrun_decode"),
        CDecoder::Tokens => (C_TOKENS, "tokens_decode"),
        CDecoder::Shortdist => (C_SHORTDIST, "shortdist_decode"),
        CDecoder::ZeroRle => (C_ZERO_RLE, "zero_rle_decode"),
        CDecoder::RleEscape => (C_RLE_ESCAPE, "rle_escape_decode"),
        CDecoder::Packbits => (C_PACKBITS, "packbits_decode"),
        CDecoder::Dpcm => (C_DPCM, "dpcm_decode"),
        CDecoder::XorPrev => (C_XOR, "xor_prev_decode"),
        CDecoder::Delta16 => (C_DELTA16, "delta16_decode"),
        CDecoder::RlePair => (C_RLE_PAIR, "rle_pair_decode"),
        CDecoder::Mtf => (C_MTF, "mtf_decode"),
        CDecoder::Bitplane => (C_BITPLANE, "bitplane_decode"),
        CDecoder::Bpe => (C_BPE, "bpe_decode"),
        CDecoder::Varint => (C_VARINT, "varint_decode"),
        CDecoder::AsciiTokens => (C_ASCII_TOKENS, "ascii_tokens_decode"),
        CDecoder::Heatshrink => (C_HEATSHRINK, "heatshrink_decode"),
        CDecoder::Inflate => (C_INFLATE, "zlib_decode"),
        CDecoder::Rans => (C_RANS, "rans_decode"),
        CDecoder::DpcmRans => (C_RANS, "dpcm_rans_decode"),
        CDecoder::MtfRans => (C_RANS, "mtf_rans_decode"),
        CDecoder::BitplaneRans => (C_RANS, "bitplane_rans_decode"),
        CDecoder::Lz4 => (C_LZ4, "lz4_decode"),
        CDecoder::Snappy => (C_SNAPPY, "snappy_decode"),
        CDecoder::Lzf => (C_LZF, "lzf_decode"),
        CDecoder::Lzjb => (C_LZJB, "lzjb_decode"),
        CDecoder::BriefLz => (C_BRIEFLZ, "brieflz_decode"),
        CDecoder::Lzo1x => (C_LZO1X, "lzo1x_decode"),
        CDecoder::Nrv2b => (C_NRV, "nrv2b_decode"),
        CDecoder::Nrv2d => (C_NRV, "nrv2d_decode"),
        CDecoder::Nrv2e => (C_NRV, "nrv2e_decode"),
        CDecoder::Lzsa2 => (C_LZSA2, "lzsa2_decode"),
    })
}

/// FNV-1a, so the generated C can verify its output without a CRC table.
fn fnv1a(data: &[u8]) -> u32
{
    let mut h: u32 = 0x811c9dc5;
    for &b in data
    {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// Emit a small C decoder specialised for this package.
///
/// The packed bytes remain in the companion `.epk`; only its payload offsets,
/// the piece recipe, and the decoders actually used are emitted here.
pub fn emitC(pkg: &Package, params: &codecs::Params, source_name: &str) -> Result<String, String>
{
    // Codec ids in first-use order; 0 is reserved for a verbatim copy.
    let mut codecs_used: Vec<(&str, &'static str, &'static str)> = Vec::new(); // name, template, fn
    for c in &pkg.chunks
    {
        if !codecs_used.iter().any(|(n, _, _)| *n == c.codec)
        {
            let (tpl, f) = templateFor(&c.codec)
                .ok_or_else(|| format!("{} has no C decoder template", c.codec))?;
            codecs_used.push((&c.codec, tpl, f));
        }
    }

    // Reconstruct locally so the emitted checksum is of the real output.
    let mut rebuilt = vec![0u8; pkg.original_len];
    for c in &pkg.chunks
    {
        let raw =
            decodeAny(&c.codec, &c.data, params).map_err(|e| format!("{}: {}", c.label, e))?;
        rebuilt[c.offset..c.offset + raw.len()].copy_from_slice(&raw);
    }
    for (off, g) in &pkg.gaps
    {
        rebuilt[*off..*off + g.len()].copy_from_slice(g);
    }
    let checksum = fnv1a(&rebuilt);

    // Locate each payload directly in the serialised ENPK2 file.
    struct Piece
    {
        label: String,
        in_off: usize,
        in_len: usize,
        out_off: usize,
        out_len: usize,
        codec: usize,
    }
    let mut pieces = Vec::with_capacity(pkg.chunks.len() + pkg.gaps.len());
    let mut pos = HEADER_LEN;
    for c in &pkg.chunks
    {
        pos += 1 + c.codec.len() + 12;
        pieces.push(Piece {
            label: c.label.clone(),
            in_off: pos,
            in_len: c.data.len(),
            out_off: c.offset,
            out_len: c.original_len,
            codec: 1 + codecs_used
                .iter()
                .position(|(n, _, _)| *n == c.codec)
                .unwrap(),
        });
        pos += c.data.len();
    }
    for (off, g) in &pkg.gaps
    {
        pos += 8;
        pieces.push(Piece {
            label: "verbatim".into(),
            in_off: pos,
            in_len: g.len(),
            out_off: *off,
            out_len: g.len(),
            codec: 0,
        });
        pos += g.len();
    }
    let packed_len = pos;
    pieces.sort_by_key(|p| p.out_off);

    let mut s = String::new();
    s.push_str(&format!(
        "/* sprism decoder for {}: pass the companion ENPK2 file to sprism_unpack(). */
#define SPRISM_ORIGINAL_LEN {}u
#define SPRISM_PACKED_LEN   {}u
#define SPRISM_CHECKSUM     0x{:08x}u

#define MIN_MATCH {}
#define ESC_BYTE 0x{:02x}
",
        source_name,
        pkg.original_len,
        packed_len,
        checksum,
        params.min_match.max(2),
        params.esc
    ));

    let mut emitted: Vec<&str> = Vec::new();
    for (_, tpl, _) in &codecs_used
    {
        if !emitted.contains(tpl)
        {
            s.push_str(tpl);
            emitted.push(tpl);
        }
    }

    s.push_str("\n/* ENPK2 offset, packed length, output offset, output length, codec: 0 = copy");
    for (i, (name, _, f)) in codecs_used.iter().enumerate()
    {
        s.push_str(&format!(", {} = {} ({})", i + 1, name, f));
    }
    s.push_str(" */\nstatic const struct { unsigned in_off, in_len, out_off, out_len; unsigned char codec; }\nsprism_pieces[");
    s.push_str(&format!("{}] = {{\n", pieces.len()));
    for p in &pieces
    {
        s.push_str(&format!(
            "    {{ {}u, {}u, {}u, {}u, {} }}, /* {} */\n",
            p.in_off, p.in_len, p.out_off, p.out_len, p.codec, p.label
        ));
    }
    s.push_str("};\n");

    s.push_str(
        r#"
/* FNV-1a over the reconstructed output, so a corrupted image is caught here
   rather than by whatever runs next. */
static unsigned sprism_checksum(const unsigned char *p, unsigned n) {
    unsigned h = 0x811c9dc5u;
    for (unsigned i = 0; i < n; i++) { h ^= p[i]; h *= 0x01000193u; }
    return h;
}

/* Returns bytes written, or 0 on malformed input or insufficient output space. */
unsigned sprism_unpack(const unsigned char *packed, unsigned packed_len,
                       unsigned char *out, unsigned out_cap) {
    if (packed_len < SPRISM_PACKED_LEN || out_cap < SPRISM_ORIGINAL_LEN) return 0;
    for (unsigned i = 0; i < sizeof sprism_pieces / sizeof sprism_pieces[0]; i++) {
        const unsigned char *src = packed + sprism_pieces[i].in_off;
        unsigned n = sprism_pieces[i].in_len, want = sprism_pieces[i].out_len;
        unsigned char *dst = out + sprism_pieces[i].out_off;
        unsigned got = 0;
        switch (sprism_pieces[i].codec) {
        case 0: for (unsigned k = 0; k < n; k++) dst[k] = src[k]; got = n; break;
"#,
    );
    for (i, (_, _, f)) in codecs_used.iter().enumerate()
    {
        s.push_str(&format!(
            "        case {}: got = {}(src, n, dst, want); break;\n",
            i + 1,
            f
        ));
    }
    s.push_str(
        r#"        default: return 0;
        }
        if (got != want) return 0;
    }
    if (sprism_checksum(out, SPRISM_ORIGINAL_LEN) != SPRISM_CHECKSUM) return 0;
    return SPRISM_ORIGINAL_LEN;
}
"#,
    );
    Ok(s)
}
