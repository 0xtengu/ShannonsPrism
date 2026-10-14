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

//! Library compressors, statically linked; each carries a decoder so the round-trip check covers it.

use std::io::{Read, Write};

pub struct LibCodec
{
    pub name: &'static str,
    pub family: &'static str,
    pub encode: fn(&[u8]) -> Result<Vec<u8>, String>,
    pub decode: fn(&[u8]) -> Result<Vec<u8>, String>,
}

fn e<E: std::fmt::Display>(err: E) -> String
{
    err.to_string()
}

// --- deflate ---------------------------------------------------------------

fn deflate(d: &[u8], level: u32) -> Result<Vec<u8>, String>
{
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(level));
    enc.write_all(d).map_err(e)?;
    enc.finish().map_err(e)
}

fn inflate(d: &[u8]) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(d)
        .read_to_end(&mut out)
        .map_err(e)?;
    Ok(out)
}

fn deflate1(d: &[u8]) -> Result<Vec<u8>, String>
{
    deflate(d, 1)
}
fn deflate6(d: &[u8]) -> Result<Vec<u8>, String>
{
    deflate(d, 6)
}
fn deflate9(d: &[u8]) -> Result<Vec<u8>, String>
{
    deflate(d, 9)
}

fn zopfliEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    let opts = zopfli::Options::default();
    let mut out = Vec::new();
    zopfli::compress(opts, zopfli::Format::Zlib, d, &mut out).map_err(e)?;
    Ok(out)
}

// --- bzip2 -----------------------------------------------------------------

fn bz2(d: &[u8], level: u32) -> Result<Vec<u8>, String>
{
    let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::new(level));
    enc.write_all(d).map_err(e)?;
    enc.finish().map_err(e)
}

fn bz21(d: &[u8]) -> Result<Vec<u8>, String>
{
    bz2(d, 1)
}
fn bz29(d: &[u8]) -> Result<Vec<u8>, String>
{
    bz2(d, 9)
}

fn bz2Dec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    bzip2::read::BzDecoder::new(d)
        .read_to_end(&mut out)
        .map_err(e)?;
    Ok(out)
}

// --- xz / lzma -------------------------------------------------------------

fn xz(d: &[u8], preset: u32) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    xz2::read::XzEncoder::new(d, preset)
        .read_to_end(&mut out)
        .map_err(e)?;
    Ok(out)
}

fn xz0(d: &[u8]) -> Result<Vec<u8>, String>
{
    xz(d, 0)
}
fn xz6(d: &[u8]) -> Result<Vec<u8>, String>
{
    xz(d, 6)
}
fn xz9(d: &[u8]) -> Result<Vec<u8>, String>
{
    xz(d, 9)
}

fn lzmaAlone(d: &[u8]) -> Result<Vec<u8>, String>
{
    let opts = xz2::stream::LzmaOptions::new_preset(6).map_err(e)?;
    let stream = xz2::stream::Stream::new_lzma_encoder(&opts).map_err(e)?;
    let mut enc = xz2::write::XzEncoder::new_stream(Vec::new(), stream);
    enc.write_all(d).map_err(e)?;
    enc.finish().map_err(e)
}

fn lzmaAloneDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let stream = xz2::stream::Stream::new_lzma_decoder(512 << 20).map_err(e)?;
    let mut out = Vec::new();
    xz2::read::XzDecoder::new_stream(d, stream)
        .read_to_end(&mut out)
        .map_err(e)?;
    Ok(out)
}

fn xzDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    xz2::read::XzDecoder::new(d)
        .read_to_end(&mut out)
        .map_err(e)?;
    Ok(out)
}

fn xz6Bcj(d: &[u8]) -> Result<Vec<u8>, String>
{
    xzFiltered(d, 6, Bcj::X86)
}
fn xz9Bcj(d: &[u8]) -> Result<Vec<u8>, String>
{
    xzFiltered(d, 9, Bcj::X86)
}

// --- zstd ------------------------------------------------------------------

fn zstdEnc(d: &[u8], level: i32) -> Result<Vec<u8>, String>
{
    // zstd sizes its tables from the level, not the input (level 22 allocates ~700 MB for a
    // 256 KB file), so clamp the window to the input as the zstd CLI does and the library does not.
    let needed = 64 - (d.len().max(1) as u64).leading_zeros(); // ceil(log2(len))
    let window_log = needed.clamp(10, 27);

    let mut c = zstd::bulk::Compressor::new(level).map_err(e)?;
    c.set_parameter(zstd::zstd_safe::CParameter::WindowLog(window_log))
        .map_err(e)?;
    c.set_pledged_src_size(Some(d.len() as u64)).map_err(e)?;
    c.compress(d).map_err(e)
}

fn zstd1(d: &[u8]) -> Result<Vec<u8>, String>
{
    zstdEnc(d, 1)
}
fn zstd3(d: &[u8]) -> Result<Vec<u8>, String>
{
    zstdEnc(d, 3)
}
fn zstd9(d: &[u8]) -> Result<Vec<u8>, String>
{
    zstdEnc(d, 9)
}
fn zstd19(d: &[u8]) -> Result<Vec<u8>, String>
{
    zstdEnc(d, 19)
}
fn zstd22(d: &[u8]) -> Result<Vec<u8>, String>
{
    zstdEnc(d, 22)
}

fn zstdDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    zstd::decode_all(d).map_err(e)
}

// --- brotli ----------------------------------------------------------------

fn brotliEnc(d: &[u8], quality: u32, lgwin: u32) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    let mut r = d;
    brotli::BrotliCompress(
        &mut r,
        &mut out,
        &brotli::enc::BrotliEncoderParams {
            quality: quality as i32,
            lgwin: lgwin as i32,
            ..Default::default()
        },
    )
    .map_err(e)?;
    Ok(out)
}

fn brotli5(d: &[u8]) -> Result<Vec<u8>, String>
{
    brotliEnc(d, 5, 22)
}
fn brotli9(d: &[u8]) -> Result<Vec<u8>, String>
{
    brotliEnc(d, 9, 22)
}
fn brotli11(d: &[u8]) -> Result<Vec<u8>, String>
{
    brotliEnc(d, 11, 24)
}

fn brotliDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    let mut r = d;
    brotli::BrotliDecompress(&mut r, &mut out).map_err(e)?;
    Ok(out)
}

// --- lz4 / snappy ----------------------------------------------------------

fn lz4Enc(d: &[u8]) -> Result<Vec<u8>, String>
{
    Ok(lz4_flex::block::compress_prepend_size(d))
}

fn lz4Dec(d: &[u8]) -> Result<Vec<u8>, String>
{
    // decompress_size_prepended allocates the declared u32 up front, so a corrupt 0xFFFFFFFF
    // prefix asks for 4 GB and aborts; reject lengths the payload could not plausibly expand to.
    if d.len() < 4
    {
        return Err("lz4: truncated size prefix".into());
    }
    let claimed = u32::from_le_bytes([d[0], d[1], d[2], d[3]]) as usize;
    let max_plausible = d.len().saturating_mul(512).saturating_add(1 << 16);
    if claimed > max_plausible
    {
        return Err(format!(
            "lz4: declared length {} implausible for {} input bytes",
            claimed,
            d.len()
        ));
    }
    lz4_flex::block::decompress_size_prepended(d).map_err(e)
}

fn snappyEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    snap::raw::Encoder::new().compress_vec(d).map_err(e)
}

fn snappyDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    snap::raw::Decoder::new().decompress_vec(d).map_err(e)
}

// --- ppmd ------------------------------------------------------------------

fn ppmdEnc(d: &[u8], order: u32, mem_mb: u32) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    {
        let mut enc = ppmd_rust::Ppmd7Encoder::new(&mut out, order, mem_mb << 20).map_err(e)?;
        enc.write_all(d).map_err(e)?;
        enc.flush().map_err(e)?;
    }
    // The decoder needs the original length; PPMd streams are not self-terminating.
    let mut framed = (d.len() as u64).to_le_bytes().to_vec();
    framed.extend_from_slice(&out);
    Ok(framed)
}

fn ppmdDec(d: &[u8], order: u32, mem_mb: u32) -> Result<Vec<u8>, String>
{
    if d.len() < 8
    {
        return Err("ppmd: truncated header".into());
    }
    let n = u64::from_le_bytes(d[0..8].try_into().unwrap()) as usize;

    // Attacker-controlled on malformed input; PPMd compresses hard, so the margin is wide but bounded.
    let max_plausible = d.len().saturating_mul(4096).saturating_add(1 << 20);
    if n > max_plausible
    {
        return Err(format!(
            "ppmd: declared length {} implausible for {} input bytes",
            n,
            d.len()
        ));
    }

    let mut dec = ppmd_rust::Ppmd7Decoder::new(&d[8..], order, mem_mb << 20).map_err(e)?;
    let mut out = vec![0u8; n];
    dec.read_exact(&mut out).map_err(e)?;
    Ok(out)
}

fn ppmd6(d: &[u8]) -> Result<Vec<u8>, String>
{
    ppmdEnc(d, 6, 16)
}
fn ppmd6D(d: &[u8]) -> Result<Vec<u8>, String>
{
    ppmdDec(d, 6, 16)
}
fn ppmd16(d: &[u8]) -> Result<Vec<u8>, String>
{
    ppmdEnc(d, 16, 64)
}
fn ppmd16D(d: &[u8]) -> Result<Vec<u8>, String>
{
    ppmdDec(d, 16, 64)
}

// --- BCJ branch filters ----------------------------------------------------

fn xzFiltered(d: &[u8], preset: u32, which: Bcj) -> Result<Vec<u8>, String>
{
    let mut filters = xz2::stream::Filters::new();
    match which
    {
        Bcj::X86 =>
        {
            filters.x86();
        }
        Bcj::Arm =>
        {
            filters.arm();
        }
        Bcj::ArmThumb =>
        {
            filters.arm_thumb();
        }
        Bcj::PowerPc =>
        {
            filters.powerpc();
        }
        Bcj::Sparc =>
        {
            filters.sparc();
        }
        Bcj::Ia64 =>
        {
            filters.ia64();
        }
    }
    let opts = xz2::stream::LzmaOptions::new_preset(preset).map_err(e)?;
    filters.lzma2(&opts);
    let stream =
        xz2::stream::Stream::new_stream_encoder(&filters, xz2::stream::Check::Crc64).map_err(e)?;
    let mut out = Vec::new();
    xz2::read::XzEncoder::new_stream(d, stream)
        .read_to_end(&mut out)
        .map_err(e)?;
    Ok(out)
}

#[derive(Clone, Copy)]
enum Bcj
{
    X86,
    Arm,
    ArmThumb,
    PowerPc,
    Sparc,
    Ia64,
}

fn xz6Arm(d: &[u8]) -> Result<Vec<u8>, String>
{
    xzFiltered(d, 6, Bcj::Arm)
}
fn xz6Thumb(d: &[u8]) -> Result<Vec<u8>, String>
{
    xzFiltered(d, 6, Bcj::ArmThumb)
}
fn xz6Ppc(d: &[u8]) -> Result<Vec<u8>, String>
{
    xzFiltered(d, 6, Bcj::PowerPc)
}
fn xz6Sparc(d: &[u8]) -> Result<Vec<u8>, String>
{
    xzFiltered(d, 6, Bcj::Sparc)
}
fn xz6Ia64(d: &[u8]) -> Result<Vec<u8>, String>
{
    xzFiltered(d, 6, Bcj::Ia64)
}

// --- LZW -------------------------------------------------------------------

fn lzwEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    weezl::encode::Encoder::new(weezl::BitOrder::Msb, 8)
        .encode(d)
        .map_err(e)
}

fn lzwDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    weezl::decode::Decoder::new(weezl::BitOrder::Msb, 8)
        .decode(d)
        .map_err(e)
}

// --- libdeflate ------------------------------------------------------------

fn libdeflate(d: &[u8], level: i32) -> Result<Vec<u8>, String>
{
    let lvl = libdeflater::CompressionLvl::new(level).map_err(|x| format!("{:?}", x))?;
    let mut c = libdeflater::Compressor::new(lvl);
    let mut out = vec![0u8; c.zlib_compress_bound(d.len())];
    let n = c
        .zlib_compress(d, &mut out)
        .map_err(|x| format!("{:?}", x))?;
    out.truncate(n);
    Ok(out)
}

fn libdeflate6(d: &[u8]) -> Result<Vec<u8>, String>
{
    libdeflate(d, 6)
}
fn libdeflate12(d: &[u8]) -> Result<Vec<u8>, String>
{
    libdeflate(d, 12)
}

// --- LZO -------------------------------------------------------------------
// decompress() needs the output size up front, so the stream carries a u64 LE length header.

fn lzoEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    let body = lzo1x::compress(d, lzo1x::CompressLevel::default());
    let mut out = (d.len() as u64).to_le_bytes().to_vec();
    out.extend_from_slice(&body);
    Ok(out)
}

fn lzoDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    if d.len() < 8
    {
        return Err("lzo: truncated header".into());
    }
    let n = u64::from_le_bytes(d[0..8].try_into().unwrap()) as usize;
    let max_plausible = d.len().saturating_mul(512).saturating_add(1 << 16);
    if n > max_plausible
    {
        return Err(format!(
            "lzo: declared length {} implausible for {} input bytes",
            n,
            d.len()
        ));
    }
    let mut out = vec![0u8; n];
    lzo1x::decompress(&d[8..], &mut out).map_err(|x| format!("{:?}", x))?;
    Ok(out)
}

// --- LZSA2 (bundled C, zlib/CC0 licensed) ----------------------------------

#[link(name = "lzsa", kind = "static")]
unsafe extern "C" {
    fn lzsa_get_max_compressed_size_inmem(input_len: usize) -> usize;
    fn lzsa_compress_inmem(
        input: *mut u8,
        output: *mut u8,
        input_len: usize,
        output_cap: usize,
        flags: u32,
        min_match: i32,
        version: i32,
    ) -> usize;
    fn lzsa_decompress_inmem(
        input: *mut u8,
        output: *mut u8,
        input_len: usize,
        output_cap: usize,
        flags: u32,
        version: *mut i32,
    ) -> usize;
}

const LZSA_FRAMED: u32 = 0;

fn lzsa2Enc(d: &[u8]) -> Result<Vec<u8>, String>
{
    let mut input = d.to_vec();
    let cap = unsafe { lzsa_get_max_compressed_size_inmem(input.len()) };
    if cap == 0 || cap == usize::MAX
    {
        return Err("lzsa2: could not determine output capacity".into());
    }
    let mut payload = vec![0u8; cap];
    let n = unsafe {
        lzsa_compress_inmem(
            input.as_mut_ptr(),
            payload.as_mut_ptr(),
            input.len(),
            payload.len(),
            LZSA_FRAMED,
            2,
            2,
        )
    };
    if n == usize::MAX || n > payload.len()
    {
        return Err("lzsa2: compression failed".into());
    }
    payload.truncate(n);

    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(b"LSA2");
    out.extend_from_slice(&(d.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

fn lzsa2Dec(d: &[u8]) -> Result<Vec<u8>, String>
{
    if d.len() < 8 || &d[..4] != b"LSA2"
    {
        return Err("lzsa2: bad header".into());
    }
    let n = u32::from_le_bytes([d[4], d[5], d[6], d[7]]) as usize;
    let max_plausible = d.len().saturating_mul(65536).saturating_add(65536);
    if n > max_plausible
    {
        return Err(format!("lzsa2: declared length {} implausible", n));
    }
    let mut input = d[8..].to_vec();
    let mut out = vec![0u8; n];
    let mut version = 2;
    let got = unsafe {
        lzsa_decompress_inmem(
            input.as_mut_ptr(),
            out.as_mut_ptr(),
            input.len(),
            out.len(),
            LZSA_FRAMED,
            &mut version,
        )
    };
    if got == usize::MAX || got != n || version != 2
    {
        return Err("lzsa2: decompression failed".into());
    }
    Ok(out)
}

// --- Density ---------------------------------------------------------------

fn wrappedEncode(
    magic: &[u8; 4],
    d: &[u8],
    encode: impl FnOnce(&[u8], &mut [u8]) -> Result<usize, String>,
) -> Result<Vec<u8>, String>
{
    let mut payload = vec![0u8; d.len().saturating_mul(2).saturating_add(64)];
    let n = encode(d, &mut payload)?;
    payload.truncate(n);
    let mut out = Vec::with_capacity(8 + n);
    out.extend_from_slice(magic);
    out.extend_from_slice(&(d.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

fn wrappedLen(d: &[u8], magic: &[u8; 4], codec: &str) -> Result<usize, String>
{
    if d.len() < 8 || &d[..4] != magic
    {
        return Err(format!("{}: bad header", codec));
    }
    let n = u32::from_le_bytes([d[4], d[5], d[6], d[7]]) as usize;
    let max_plausible = d.len().saturating_mul(65536).saturating_add(65536);
    if n > max_plausible
    {
        return Err(format!("{}: declared length {} implausible", codec, n));
    }
    Ok(n)
}

fn densityChameleonEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    wrappedEncode(b"DNC1", d, |src, dst| {
        density_rs::algorithms::chameleon::chameleon::Chameleon::encode(src, dst)
            .map_err(|e| format!("{:?}", e))
    })
}

fn densityChameleonDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let n = wrappedLen(d, b"DNC1", "density_chameleon")?;
    let mut out = vec![0u8; n];
    let got = density_rs::algorithms::chameleon::chameleon::Chameleon::decode(&d[8..], &mut out)
        .map_err(|e| format!("{:?}", e))?;
    if got != n
    {
        return Err(format!(
            "density_chameleon: decoded {}, expected {}",
            got, n
        ));
    }
    Ok(out)
}

fn densityCheetahEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    wrappedEncode(b"DNT1", d, |src, dst| {
        density_rs::algorithms::cheetah::cheetah::Cheetah::encode(src, dst)
            .map_err(|e| format!("{:?}", e))
    })
}

fn densityCheetahDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let n = wrappedLen(d, b"DNT1", "density_cheetah")?;
    let mut out = vec![0u8; n];
    let got = density_rs::algorithms::cheetah::cheetah::Cheetah::decode(&d[8..], &mut out)
        .map_err(|e| format!("{:?}", e))?;
    if got != n
    {
        return Err(format!("density_cheetah: decoded {}, expected {}", got, n));
    }
    Ok(out)
}

fn densityLionEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    wrappedEncode(b"DNL1", d, |src, dst| {
        density_rs::algorithms::lion::lion::Lion::encode(src, dst).map_err(|e| format!("{:?}", e))
    })
}

fn densityLionDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let n = wrappedLen(d, b"DNL1", "density_lion")?;
    let mut out = vec![0u8; n];
    let got = density_rs::algorithms::lion::lion::Lion::decode(&d[8..], &mut out)
        .map_err(|e| format!("{:?}", e))?;
    if got != n
    {
        return Err(format!("density_lion: decoded {}, expected {}", got, n));
    }
    Ok(out)
}

// --- LZF / LZFSE / QuickLZ -------------------------------------------------

fn lzfEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    let mut payload = vec![0u8; lzf_rust::max_compressed_size(d.len())];
    let n = lzf_rust::compress(d, &mut payload).map_err(e)?;
    payload.truncate(n);
    let mut out = Vec::with_capacity(8 + n);
    out.extend_from_slice(b"LZF1");
    out.extend_from_slice(&(d.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

fn lzfDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let n = wrappedLen(d, b"LZF1", "lzf")?;
    let mut out = vec![0u8; n];
    let got = lzf_rust::decompress(&d[8..], &mut out).map_err(e)?;
    if got != n
    {
        return Err(format!("lzf: decoded {}, expected {}", got, n));
    }
    Ok(out)
}

fn lzfseEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    let mut payload = vec![0u8; d.len().saturating_add(12)];
    let n = lzfse::encode_buffer(d, &mut payload).map_err(|e| format!("{:?}", e))?;
    payload.truncate(n);
    let mut out = Vec::with_capacity(8 + n);
    out.extend_from_slice(b"LFS1");
    out.extend_from_slice(&(d.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

fn lzfseDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let n = wrappedLen(d, b"LFS1", "lzfse")?;
    let mut out = vec![0u8; n.saturating_add(1)];
    let got = lzfse::decode_buffer(&d[8..], &mut out).map_err(|e| format!("{:?}", e))?;
    if got != n
    {
        return Err(format!("lzfse: decoded {}, expected {}", got, n));
    }
    out.truncate(n);
    Ok(out)
}

fn quicklzEnc(d: &[u8], level: quicklz::CompressionLevel) -> Result<Vec<u8>, String>
{
    let payload = quicklz::compress(d, level);
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(b"QLZ1");
    out.extend_from_slice(&(d.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

fn quicklz1Enc(d: &[u8]) -> Result<Vec<u8>, String>
{
    quicklzEnc(d, quicklz::CompressionLevel::Lvl1)
}

fn quicklz3Enc(d: &[u8]) -> Result<Vec<u8>, String>
{
    quicklzEnc(d, quicklz::CompressionLevel::Lvl3)
}

fn quicklzDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let n = wrappedLen(d, b"QLZ1", "quicklz")?;
    let mut cursor = std::io::Cursor::new(&d[8..]);
    let out = quicklz::decompress(&mut cursor, n as u32).map_err(e)?;
    if out.len() != n
    {
        return Err(format!("quicklz: decoded {}, expected {}", out.len(), n));
    }
    Ok(out)
}

// --- LZAV / LZJB -----------------------------------------------------------

fn lzavEnc(d: &[u8], high_ratio: bool) -> Result<Vec<u8>, String>
{
    let src_len = i32::try_from(d.len()).map_err(|_| "lzav: input exceeds 2 GiB")?;
    let cap = unsafe {
        if high_ratio
        {
            lzav::compress_bound_hi(src_len)
        }
        else
        {
            lzav::compress_bound(src_len)
        }
    };
    if cap < 0
    {
        return Err("lzav: invalid output bound".into());
    }
    let mut payload = vec![0u8; cap as usize];
    let got = if d.is_empty()
    {
        0
    }
    else
    {
        unsafe {
            if high_ratio
            {
                lzav::compress_hi(d.as_ptr().cast(), payload.as_mut_ptr().cast(), src_len, cap)
            }
            else
            {
                lzav::compress_default(d.as_ptr().cast(), payload.as_mut_ptr().cast(), src_len, cap)
            }
        }
    };
    if got <= 0 && !d.is_empty()
    {
        return Err("lzav: compression failed".into());
    }
    payload.truncate(got as usize);
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(b"LZAV");
    out.extend_from_slice(&(d.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

fn lzavDefaultEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    lzavEnc(d, false)
}
fn lzavHighEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    lzavEnc(d, true)
}
fn lzavDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let n = wrappedLen(d, b"LZAV", "lzav")?;
    if n == 0
    {
        return Ok(Vec::new());
    }
    let src_len = i32::try_from(d.len() - 8).map_err(|_| "lzav: input exceeds 2 GiB")?;
    let dst_len = i32::try_from(n).map_err(|_| "lzav: output exceeds 2 GiB")?;
    let mut out = vec![0u8; n];
    let got = unsafe {
        lzav::decompress(
            d[8..].as_ptr().cast(),
            out.as_mut_ptr().cast(),
            src_len,
            dst_len,
        )
    };
    if got != dst_len
    {
        return Err(format!("lzav: decompression failed ({})", got));
    }
    Ok(out)
}

fn lzjbEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    Ok(lzjb::compress(d))
}
fn lzjb9Enc(d: &[u8]) -> Result<Vec<u8>, String>
{
    Ok(lzjb::compress_level(d, 9))
}
fn lzjbDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    lzjb::try_decompress(d).map_err(e)
}

// --- BriefLZ ---------------------------------------------------------------

fn briefLzEnc(d: &[u8], level: u8) -> Result<Vec<u8>, String>
{
    let mut payload = vec![0u8; brieflz::max_packed_size(d.len())];
    let work_bytes = brieflz::workmem_size_level(d.len(), level)
        .ok_or_else(|| format!("brieflz: invalid level {}", level))?;
    let mut work = vec![0u32; work_bytes.div_ceil(std::mem::size_of::<u32>())];
    let n = brieflz::pack_level(d, &mut payload, &mut work, level).map_err(e)?;
    payload.truncate(n);
    let mut out = Vec::with_capacity(8 + n);
    out.extend_from_slice(b"BLZ1");
    out.extend_from_slice(&(d.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

fn briefLz1Enc(d: &[u8]) -> Result<Vec<u8>, String>
{
    briefLzEnc(d, 1)
}
fn briefLz5Enc(d: &[u8]) -> Result<Vec<u8>, String>
{
    briefLzEnc(d, 5)
}
fn briefLz10Enc(d: &[u8]) -> Result<Vec<u8>, String>
{
    briefLzEnc(d, 10)
}
fn briefLzDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let n = wrappedLen(d, b"BLZ1", "brieflz")?;
    let mut out = vec![0u8; n];
    let got = brieflz::depack_safe(&d[8..], &mut out, n).map_err(e)?;
    if got != n
    {
        return Err(format!("brieflz: decoded {}, expected {}", got, n));
    }
    Ok(out)
}

// --- LZRW family (bundled C through lzrw-sys) -----------------------------

fn lzrwEnc(magic: &[u8; 4], algorithm: lzrw::LzrwAlgorithm, d: &[u8]) -> Result<Vec<u8>, String>
{
    let payload = lzrw::compress(algorithm, d);
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(magic);
    out.extend_from_slice(&(d.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

fn lzrwDec(
    magic: &[u8; 4],
    name: &str,
    function: lzrw_sys::CompressFn,
    d: &[u8],
) -> Result<Vec<u8>, String>
{
    let n = wrappedLen(d, magic, name)?;
    let src_len = u32::try_from(d.len() - 8).map_err(|_| "lzrw: input exceeds 4 GiB")?;
    let mut out = vec![0u8; n.saturating_add(lzrw_sys::COMPRESS_OVERRUN)];
    let mut out_len = 0u32;
    let mut work = vec![0u8; lzrw_sys::get_wrk_mem_size(function)];
    unsafe {
        function(
            lzrw_sys::COMPRESS_ACTION_DECOMPRESS,
            work.as_mut_ptr(),
            d[8..].as_ptr(),
            src_len,
            out.as_mut_ptr(),
            &mut out_len,
        );
    }
    if out_len as usize != n
    {
        return Err(format!("{}: decoded {}, expected {}", name, out_len, n));
    }
    out.truncate(n);
    Ok(out)
}

macro_rules! lzrwCodec {
    ($enc:ident, $dec:ident, $magic:literal, $name:literal, $variant:ident, $ffi:ident) => {
        fn $enc(d: &[u8]) -> Result<Vec<u8>, String>
        {
            lzrwEnc($magic, lzrw::LzrwAlgorithm::$variant, d)
        }
        fn $dec(d: &[u8]) -> Result<Vec<u8>, String>
        {
            lzrwDec($magic, $name, lzrw_sys::$ffi, d)
        }
    };
}

lzrwCodec!(lzrw1Enc, lzrw1Dec, b"LRW1", "lzrw1", Lzrw1, lzrw1_compress);
lzrwCodec!(
    lzrw1aEnc,
    lzrw1aDec,
    b"LR1A",
    "lzrw1a",
    Lzrw1a,
    lzrw1a_compress
);
lzrwCodec!(lzrw2Enc, lzrw2Dec, b"LRW2", "lzrw2", Lzrw2, lzrw2_compress);
lzrwCodec!(lzrw3Enc, lzrw3Dec, b"LRW3", "lzrw3", Lzrw3, lzrw3_compress);
lzrwCodec!(
    lzrw3aEnc,
    lzrw3aDec,
    b"LR3A",
    "lzrw3a",
    Lzrw3a,
    lzrw3a_compress
);
lzrwCodec!(lzrw5Enc, lzrw5Dec, b"LRW5", "lzrw5", Lzrw5, lzrw5_compress);

// --- LZHAM (bundled C++) ---------------------------------------------------

fn lzhamEnc(d: &[u8], level: lzham_sys::lzham_compress_level) -> Result<Vec<u8>, String>
{
    let params = lzham_sys::lzham_compress_params {
        m_struct_size: std::mem::size_of::<lzham_sys::lzham_compress_params>() as u32,
        m_dict_size_log2: 20,
        m_level: level,
        m_table_update_rate: 8,
        m_max_helper_threads: 0,
        m_compress_flags: 0,
        m_num_seed_bytes: 0,
        m_pSeed_bytes: std::ptr::null(),
        m_table_max_update_interval: 2048,
        m_table_update_interval_slow_rate: 384,
    };
    let cap = d.len().saturating_add(d.len() / 8).saturating_add(1 << 20);
    let mut payload = vec![0u8; cap];
    let mut payload_len = payload.len() as u64;
    let status = unsafe {
        lzham_sys::lzham_compress_memory(
            &params,
            payload.as_mut_ptr(),
            &mut payload_len,
            d.as_ptr(),
            d.len() as u64,
            std::ptr::null_mut(),
        )
    };
    if status != lzham_sys::lzham_compress_status_t_LZHAM_COMP_STATUS_SUCCESS
    {
        return Err(format!("lzham: compression failed ({:?})", status));
    }
    payload.truncate(payload_len as usize);
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(b"LZHM");
    out.extend_from_slice(&(d.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

fn lzhamFastEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    lzhamEnc(d, lzham_sys::lzham_compress_level_LZHAM_COMP_LEVEL_FASTEST)
}
fn lzhamDefaultEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    lzhamEnc(d, lzham_sys::lzham_compress_level_LZHAM_COMP_LEVEL_DEFAULT)
}
fn lzhamUberEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    lzhamEnc(d, lzham_sys::lzham_compress_level_LZHAM_COMP_LEVEL_UBER)
}
fn lzhamDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let n = wrappedLen(d, b"LZHM", "lzham")?;
    let params = lzham_sys::lzham_decompress_params {
        m_struct_size: std::mem::size_of::<lzham_sys::lzham_decompress_params>() as u32,
        m_dict_size_log2: 20,
        m_table_update_rate: 8,
        m_decompress_flags: 0,
        m_num_seed_bytes: 0,
        m_pSeed_bytes: std::ptr::null(),
        m_table_max_update_interval: 2048,
        m_table_update_interval_slow_rate: 384,
    };
    let mut out = vec![0u8; n];
    let mut out_len = n as u64;
    let status = unsafe {
        lzham_sys::lzham_decompress_memory(
            &params,
            out.as_mut_ptr(),
            &mut out_len,
            d[8..].as_ptr(),
            (d.len() - 8) as u64,
            std::ptr::null_mut(),
        )
    };
    if status != lzham_sys::lzham_decompress_status_t_LZHAM_DECOMP_STATUS_SUCCESS
        || out_len as usize != n
    {
        return Err(format!(
            "lzham: decompression failed ({:?}, {} of {})",
            status, out_len, n
        ));
    }
    Ok(out)
}

// --- bzip3 -----------------------------------------------------------------

fn bzip3Enc(d: &[u8]) -> Result<Vec<u8>, String>
{
    let block = bzip3::BlockSize::mib(1).ok_or("bzip3: invalid block size")?;
    let mut out = Vec::new();
    bzip3::read::Bz3Encoder::new(d, block)
        .map_err(e)?
        .read_to_end(&mut out)
        .map_err(e)?;
    Ok(out)
}

fn bzip3Dec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    bzip3::read::Bz3Decoder::new(d)
        .map_err(e)?
        .read_to_end(&mut out)
        .map_err(e)?;
    Ok(out)
}

// --- UCL NRV (bundled GPL-2.0-or-later C) ---------------------------------

type UclCompress = unsafe extern "C" fn(
    *const u8,
    u32,
    *mut u8,
    *mut u32,
    *mut std::ffi::c_void,
    i32,
    *const std::ffi::c_void,
    *mut u32,
) -> i32;
type UclDecompress =
    unsafe extern "C" fn(*const u8, u32, *mut u8, *mut u32, *mut std::ffi::c_void) -> i32;

#[link(name = "ucl", kind = "static")]
unsafe extern "C" {
    fn ucl_nrv2b_99_compress(
        src: *const u8,
        src_len: u32,
        dst: *mut u8,
        dst_len: *mut u32,
        callback: *mut std::ffi::c_void,
        level: i32,
        config: *const std::ffi::c_void,
        result: *mut u32,
    ) -> i32;
    fn ucl_nrv2d_99_compress(
        src: *const u8,
        src_len: u32,
        dst: *mut u8,
        dst_len: *mut u32,
        callback: *mut std::ffi::c_void,
        level: i32,
        config: *const std::ffi::c_void,
        result: *mut u32,
    ) -> i32;
    fn ucl_nrv2e_99_compress(
        src: *const u8,
        src_len: u32,
        dst: *mut u8,
        dst_len: *mut u32,
        callback: *mut std::ffi::c_void,
        level: i32,
        config: *const std::ffi::c_void,
        result: *mut u32,
    ) -> i32;
    fn ucl_nrv2b_decompress_safe_8(
        src: *const u8,
        src_len: u32,
        dst: *mut u8,
        dst_len: *mut u32,
        workmem: *mut std::ffi::c_void,
    ) -> i32;
    fn ucl_nrv2d_decompress_safe_8(
        src: *const u8,
        src_len: u32,
        dst: *mut u8,
        dst_len: *mut u32,
        workmem: *mut std::ffi::c_void,
    ) -> i32;
    fn ucl_nrv2e_decompress_safe_8(
        src: *const u8,
        src_len: u32,
        dst: *mut u8,
        dst_len: *mut u32,
        workmem: *mut std::ffi::c_void,
    ) -> i32;
}

fn uclEnc(magic: &[u8; 4], d: &[u8], compress: UclCompress) -> Result<Vec<u8>, String>
{
    let src_len = u32::try_from(d.len()).map_err(|_| "ucl: input exceeds 4 GiB")?;
    let cap = d.len().saturating_add(d.len() / 8).saturating_add(256);
    let mut payload = vec![0u8; cap];
    let mut payload_len = u32::try_from(cap).map_err(|_| "ucl: output capacity exceeds 4 GiB")?;
    let status = unsafe {
        compress(
            d.as_ptr(),
            src_len,
            payload.as_mut_ptr(),
            &mut payload_len,
            std::ptr::null_mut(),
            10,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    if status != 0 || payload_len as usize > payload.len()
    {
        return Err(format!("ucl: compression failed ({})", status));
    }
    payload.truncate(payload_len as usize);
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(magic);
    out.extend_from_slice(&src_len.to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

fn uclDec(
    magic: &[u8; 4],
    name: &str,
    d: &[u8],
    decompress: UclDecompress,
) -> Result<Vec<u8>, String>
{
    let n = wrappedLen(d, magic, name)?;
    let mut out = vec![0u8; n];
    let mut out_len = u32::try_from(n).map_err(|_| "ucl: output exceeds 4 GiB")?;
    let src_len = u32::try_from(d.len() - 8).map_err(|_| "ucl: input exceeds 4 GiB")?;
    let status = unsafe {
        decompress(
            d[8..].as_ptr(),
            src_len,
            out.as_mut_ptr(),
            &mut out_len,
            std::ptr::null_mut(),
        )
    };
    if status != 0 || out_len as usize != n
    {
        return Err(format!("{}: decompression failed ({})", name, status));
    }
    Ok(out)
}

fn nrv2bEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    uclEnc(b"NRVB", d, ucl_nrv2b_99_compress)
}
fn nrv2bDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    uclDec(b"NRVB", "nrv2b", d, ucl_nrv2b_decompress_safe_8)
}
fn nrv2dEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    uclEnc(b"NRVD", d, ucl_nrv2d_99_compress)
}
fn nrv2dDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    uclDec(b"NRVD", "nrv2d", d, ucl_nrv2d_decompress_safe_8)
}
fn nrv2eEnc(d: &[u8]) -> Result<Vec<u8>, String>
{
    uclEnc(b"NRVE", d, ucl_nrv2e_99_compress)
}
fn nrv2eDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    uclDec(b"NRVE", "nrv2e", d, ucl_nrv2e_decompress_safe_8)
}

// --- registry --------------------------------------------------------------

pub const LIB_CODECS: &[LibCodec] = &[
    LibCodec {
        name: "deflate1",
        family: "deflate",
        encode: deflate1,
        decode: inflate,
    },
    LibCodec {
        name: "deflate6",
        family: "deflate",
        encode: deflate6,
        decode: inflate,
    },
    LibCodec {
        name: "deflate9",
        family: "deflate",
        encode: deflate9,
        decode: inflate,
    },
    LibCodec {
        name: "zopfli",
        family: "deflate",
        encode: zopfliEnc,
        decode: inflate,
    },
    LibCodec {
        name: "libdeflate6",
        family: "deflate",
        encode: libdeflate6,
        decode: inflate,
    },
    LibCodec {
        name: "libdeflate12",
        family: "deflate",
        encode: libdeflate12,
        decode: inflate,
    },
    LibCodec {
        name: "bz2_1",
        family: "bwt",
        encode: bz21,
        decode: bz2Dec,
    },
    LibCodec {
        name: "bz2_9",
        family: "bwt",
        encode: bz29,
        decode: bz2Dec,
    },
    LibCodec {
        name: "bzip3",
        family: "bwt",
        encode: bzip3Enc,
        decode: bzip3Dec,
    },
    LibCodec {
        name: "xz0",
        family: "lzma",
        encode: xz0,
        decode: xzDec,
    },
    LibCodec {
        name: "xz6",
        family: "lzma",
        encode: xz6,
        decode: xzDec,
    },
    LibCodec {
        name: "xz9",
        family: "lzma",
        encode: xz9,
        decode: xzDec,
    },
    LibCodec {
        name: "lzma_alone",
        family: "lzma",
        encode: lzmaAlone,
        decode: lzmaAloneDec,
    },
    LibCodec {
        name: "xz6_x86",
        family: "lzma",
        encode: xz6Bcj,
        decode: xzDec,
    },
    LibCodec {
        name: "xz9_x86",
        family: "lzma",
        encode: xz9Bcj,
        decode: xzDec,
    },
    LibCodec {
        name: "xz6_arm",
        family: "lzma",
        encode: xz6Arm,
        decode: xzDec,
    },
    LibCodec {
        name: "xz6_thumb",
        family: "lzma",
        encode: xz6Thumb,
        decode: xzDec,
    },
    LibCodec {
        name: "xz6_ppc",
        family: "lzma",
        encode: xz6Ppc,
        decode: xzDec,
    },
    LibCodec {
        name: "xz6_sparc",
        family: "lzma",
        encode: xz6Sparc,
        decode: xzDec,
    },
    LibCodec {
        name: "xz6_ia64",
        family: "lzma",
        encode: xz6Ia64,
        decode: xzDec,
    },
    LibCodec {
        name: "zstd1",
        family: "zstd",
        encode: zstd1,
        decode: zstdDec,
    },
    LibCodec {
        name: "zstd3",
        family: "zstd",
        encode: zstd3,
        decode: zstdDec,
    },
    LibCodec {
        name: "zstd9",
        family: "zstd",
        encode: zstd9,
        decode: zstdDec,
    },
    LibCodec {
        name: "zstd19",
        family: "zstd",
        encode: zstd19,
        decode: zstdDec,
    },
    LibCodec {
        name: "zstd22",
        family: "zstd",
        encode: zstd22,
        decode: zstdDec,
    },
    LibCodec {
        name: "brotli5",
        family: "brotli",
        encode: brotli5,
        decode: brotliDec,
    },
    LibCodec {
        name: "brotli9",
        family: "brotli",
        encode: brotli9,
        decode: brotliDec,
    },
    LibCodec {
        name: "brotli11",
        family: "brotli",
        encode: brotli11,
        decode: brotliDec,
    },
    LibCodec {
        name: "lz4",
        family: "lz4",
        encode: lz4Enc,
        decode: lz4Dec,
    },
    LibCodec {
        name: "snappy",
        family: "lz4",
        encode: snappyEnc,
        decode: snappyDec,
    },
    LibCodec {
        name: "lzo1x",
        family: "lz4",
        encode: lzoEnc,
        decode: lzoDec,
    },
    LibCodec {
        name: "lzsa2",
        family: "embedded",
        encode: lzsa2Enc,
        decode: lzsa2Dec,
    },
    LibCodec {
        name: "density_chameleon",
        family: "density",
        encode: densityChameleonEnc,
        decode: densityChameleonDec,
    },
    LibCodec {
        name: "density_cheetah",
        family: "density",
        encode: densityCheetahEnc,
        decode: densityCheetahDec,
    },
    LibCodec {
        name: "density_lion",
        family: "density",
        encode: densityLionEnc,
        decode: densityLionDec,
    },
    LibCodec {
        name: "lzf",
        family: "fast-lz",
        encode: lzfEnc,
        decode: lzfDec,
    },
    LibCodec {
        name: "lzfse",
        family: "fast-lz",
        encode: lzfseEnc,
        decode: lzfseDec,
    },
    LibCodec {
        name: "quicklz1",
        family: "fast-lz",
        encode: quicklz1Enc,
        decode: quicklzDec,
    },
    LibCodec {
        name: "quicklz3",
        family: "fast-lz",
        encode: quicklz3Enc,
        decode: quicklzDec,
    },
    LibCodec {
        name: "lzav",
        family: "fast-lz",
        encode: lzavDefaultEnc,
        decode: lzavDec,
    },
    LibCodec {
        name: "lzav_hi",
        family: "fast-lz",
        encode: lzavHighEnc,
        decode: lzavDec,
    },
    LibCodec {
        name: "lzjb",
        family: "fast-lz",
        encode: lzjbEnc,
        decode: lzjbDec,
    },
    LibCodec {
        name: "lzjb9",
        family: "fast-lz",
        encode: lzjb9Enc,
        decode: lzjbDec,
    },
    LibCodec {
        name: "brieflz1",
        family: "fast-lz",
        encode: briefLz1Enc,
        decode: briefLzDec,
    },
    LibCodec {
        name: "brieflz5",
        family: "fast-lz",
        encode: briefLz5Enc,
        decode: briefLzDec,
    },
    LibCodec {
        name: "brieflz10",
        family: "fast-lz",
        encode: briefLz10Enc,
        decode: briefLzDec,
    },
    LibCodec {
        name: "lzrw1",
        family: "lzrw",
        encode: lzrw1Enc,
        decode: lzrw1Dec,
    },
    LibCodec {
        name: "lzrw1a",
        family: "lzrw",
        encode: lzrw1aEnc,
        decode: lzrw1aDec,
    },
    LibCodec {
        name: "lzrw2",
        family: "lzrw",
        encode: lzrw2Enc,
        decode: lzrw2Dec,
    },
    LibCodec {
        name: "lzrw3",
        family: "lzrw",
        encode: lzrw3Enc,
        decode: lzrw3Dec,
    },
    LibCodec {
        name: "lzrw3a",
        family: "lzrw",
        encode: lzrw3aEnc,
        decode: lzrw3aDec,
    },
    LibCodec {
        name: "lzrw5",
        family: "lzrw",
        encode: lzrw5Enc,
        decode: lzrw5Dec,
    },
    LibCodec {
        name: "lzham_fast",
        family: "lzham",
        encode: lzhamFastEnc,
        decode: lzhamDec,
    },
    LibCodec {
        name: "lzham",
        family: "lzham",
        encode: lzhamDefaultEnc,
        decode: lzhamDec,
    },
    LibCodec {
        name: "lzham_uber",
        family: "lzham",
        encode: lzhamUberEnc,
        decode: lzhamDec,
    },
    LibCodec {
        name: "nrv2b",
        family: "nrv",
        encode: nrv2bEnc,
        decode: nrv2bDec,
    },
    LibCodec {
        name: "nrv2d",
        family: "nrv",
        encode: nrv2dEnc,
        decode: nrv2dDec,
    },
    LibCodec {
        name: "nrv2e",
        family: "nrv",
        encode: nrv2eEnc,
        decode: nrv2eDec,
    },
    LibCodec {
        name: "lzw",
        family: "lzw",
        encode: lzwEnc,
        decode: lzwDec,
    },
    LibCodec {
        name: "ppmd_o6",
        family: "ppmd",
        encode: ppmd6,
        decode: ppmd6D,
    },
    LibCodec {
        name: "ppmd_o16",
        family: "ppmd",
        encode: ppmd16,
        decode: ppmd16D,
    },
];

pub fn find(name: &str) -> Option<&'static LibCodec>
{
    LIB_CODECS.iter().find(|c| c.name == name)
}

/// Codecs the linked libraries actually support, probed once.
pub fn available() -> &'static [&'static LibCodec]
{
    use std::sync::OnceLock;
    static AVAILABLE: OnceLock<Vec<&'static LibCodec>> = OnceLock::new();
    AVAILABLE.get_or_init(|| {
        let probe = vec![0x41u8; 128];
        LIB_CODECS
            .iter()
            .filter(|c| {
                // Which BCJ filters liblzma accepts depends on how it was built; nothing else is in doubt.
                if !c.name.contains('_') || c.family != "lzma"
                {
                    return true;
                }
                (c.encode)(&probe).is_ok()
            })
            .collect()
    })
}

/// Names of BCJ filters the linked liblzma refused, for reporting.
pub fn unavailableNames() -> Vec<&'static str>
{
    let ok: Vec<&str> = available().iter().map(|c| c.name).collect();
    LIB_CODECS
        .iter()
        .map(|c| c.name)
        .filter(|n| !ok.contains(n))
        .collect()
}

// --- panic containment ------------------------------------------------------
// Some third-party decoders panic on malformed input; the tool is pointed at hostile files,
// so a panic must become an ordinary error rather than take the run down.

fn guard(name: &str, f: impl FnOnce() -> Result<Vec<u8>, String>) -> Result<Vec<u8>, String>
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
    {
        Ok(r) => r,
        Err(p) =>
        {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            Err(format!("{}: codec panicked: {}", name, msg))
        }
    }
}

/// Encode with a library codec, converting a panic inside it into an error.
pub fn encodeSafe(c: &LibCodec, d: &[u8]) -> Result<Vec<u8>, String>
{
    guard(c.name, || (c.encode)(d))
}

/// Decode with a library codec, converting a panic inside it into an error.
pub fn decodeSafe(c: &LibCodec, d: &[u8]) -> Result<Vec<u8>, String>
{
    guard(c.name, || (c.decode)(d))
}
