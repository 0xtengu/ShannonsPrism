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

//! Codec metadata shared by sweeping, listing, packing and C emission.
//!
//! Encoders and decoders remain in `codecs` and `libs`; this is the single
//! source of truth for identity, family, sweep policy and portable decoder.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Backend
{
    Native,
    Library,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CDecoder
{
    Lzss,
    Lz4ish,
    Litrun,
    Tokens,
    Shortdist,
    ZeroRle,
    RleEscape,
    Packbits,
    Dpcm,
    XorPrev,
    Delta16,
    RlePair,
    Mtf,
    Bitplane,
    Bpe,
    Varint,
    AsciiTokens,
    Heatshrink,
    Inflate,
    Rans,
    DpcmRans,
    MtfRans,
    BitplaneRans,
    Lz4,
    Snappy,
    Lzf,
    Lzjb,
    BriefLz,
    Lzo1x,
    Nrv2b,
    Nrv2d,
    Nrv2e,
    Lzsa2,
}

#[derive(Clone, Copy, Debug)]
pub struct CodecMeta
{
    pub name: &'static str,
    pub family: &'static str,
    pub backend: Backend,
    pub c_decoder: Option<CDecoder>,
    pub slow_for_chunks: bool,
}

macro_rules! native {
    ($name:literal, $family:literal) => {
        CodecMeta {
            name: $name,
            family: $family,
            backend: Backend::Native,
            c_decoder: None,
            slow_for_chunks: false,
        }
    };
    ($name:literal, $family:literal, $decoder:ident) => {
        CodecMeta {
            name: $name,
            family: $family,
            backend: Backend::Native,
            c_decoder: Some(CDecoder::$decoder),
            slow_for_chunks: false,
        }
    };
}

macro_rules! library {
    ($name:literal, $family:literal) => {
        CodecMeta {
            name: $name,
            family: $family,
            backend: Backend::Library,
            c_decoder: None,
            slow_for_chunks: false,
        }
    };
    ($name:literal, $family:literal, slow) => {
        CodecMeta {
            name: $name,
            family: $family,
            backend: Backend::Library,
            c_decoder: None,
            slow_for_chunks: true,
        }
    };
    ($name:literal, $family:literal, $decoder:ident) => {
        CodecMeta {
            name: $name,
            family: $family,
            backend: Backend::Library,
            c_decoder: Some(CDecoder::$decoder),
            slow_for_chunks: false,
        }
    };
    ($name:literal, $family:literal, $decoder:ident, slow) => {
        CodecMeta {
            name: $name,
            family: $family,
            backend: Backend::Library,
            c_decoder: Some(CDecoder::$decoder),
            slow_for_chunks: true,
        }
    };
}

pub const ALL: &[CodecMeta] = &[
    native!("dpcm", "transform", Dpcm),
    native!("xor_prev", "transform", XorPrev),
    native!("delta16", "transform", Delta16),
    native!("rle_pair", "rle", RlePair),
    native!("rle_escape", "rle", RleEscape),
    native!("zero_rle", "rle", ZeroRle),
    native!("packbits", "rle", Packbits),
    native!("mtf", "transform", Mtf),
    native!("bitplane_split", "transform", Bitplane),
    native!("bpe", "dictionary", Bpe),
    native!("lzss", "lz", Lzss),
    native!("lz77_tokens", "lz", Tokens),
    native!("lz77_shortdist", "lz", Shortdist),
    native!("lz77_varint", "lz", Varint),
    native!("lz77_litrun", "lz", Litrun),
    native!("lz4ish", "lz", Lz4ish),
    native!("lz77_ascii_tokens", "lz", AsciiTokens),
    native!("heatshrink", "embedded", Heatshrink),
    native!("rans", "entropy", Rans),
    native!("dpcm_rans", "entropy", DpcmRans),
    native!("mtf_rans", "entropy", MtfRans),
    native!("bitplane_rans", "entropy", BitplaneRans),
    library!("deflate1", "deflate", Inflate),
    library!("deflate6", "deflate", Inflate),
    library!("deflate9", "deflate", Inflate),
    library!("zopfli", "deflate", Inflate, slow),
    library!("libdeflate6", "deflate", Inflate),
    library!("libdeflate12", "deflate", Inflate),
    library!("bz2_1", "bwt"),
    library!("bz2_9", "bwt"),
    library!("bzip3", "bwt"),
    library!("xz0", "lzma"),
    library!("xz6", "lzma"),
    library!("xz9", "lzma", slow),
    library!("lzma_alone", "lzma"),
    library!("xz6_x86", "lzma"),
    library!("xz9_x86", "lzma", slow),
    library!("xz6_arm", "lzma"),
    library!("xz6_thumb", "lzma"),
    library!("xz6_ppc", "lzma"),
    library!("xz6_sparc", "lzma"),
    library!("xz6_ia64", "lzma"),
    library!("zstd1", "zstd"),
    library!("zstd3", "zstd"),
    library!("zstd9", "zstd"),
    library!("zstd19", "zstd"),
    library!("zstd22", "zstd", slow),
    library!("brotli5", "brotli"),
    library!("brotli9", "brotli"),
    library!("brotli11", "brotli", slow),
    library!("lz4", "fast-lz", Lz4),
    library!("snappy", "fast-lz", Snappy),
    library!("lzo1x", "fast-lz", Lzo1x),
    library!("lzsa2", "embedded", Lzsa2),
    library!("density_chameleon", "density"),
    library!("density_cheetah", "density"),
    library!("density_lion", "density"),
    library!("lzf", "fast-lz", Lzf),
    library!("lzfse", "fast-lz"),
    library!("quicklz1", "fast-lz"),
    library!("quicklz3", "fast-lz"),
    library!("lzav", "fast-lz"),
    library!("lzav_hi", "fast-lz"),
    library!("lzjb", "fast-lz", Lzjb),
    library!("lzjb9", "fast-lz", Lzjb),
    library!("brieflz1", "fast-lz", BriefLz),
    library!("brieflz5", "fast-lz", BriefLz),
    library!("brieflz10", "fast-lz", BriefLz, slow),
    library!("lzrw1", "lzrw"),
    library!("lzrw1a", "lzrw"),
    library!("lzrw2", "lzrw"),
    library!("lzrw3", "lzrw"),
    library!("lzrw3a", "lzrw"),
    library!("lzrw5", "lzrw"),
    library!("lzham_fast", "lzham"),
    library!("lzham", "lzham"),
    library!("lzham_uber", "lzham", slow),
    library!("nrv2b", "nrv", Nrv2b),
    library!("nrv2d", "nrv", Nrv2d),
    library!("nrv2e", "nrv", Nrv2e),
    library!("lzw", "dictionary"),
    library!("ppmd_o6", "context"),
    library!("ppmd_o16", "context", slow),
];

pub fn find(name: &str) -> Option<&'static CodecMeta>
{
    ALL.iter().find(|c| c.name == name)
}

/// Maximum-ratio variants are useful on firmware-sized pieces but take an
/// impractical amount of time on large monolithic inputs. Families keep a
/// faster representative except NRV, whose three variants share the slow
/// `_99_compress` implementation.
pub fn includeInDefaultWholeSweep(name: &str, input_len: usize) -> bool
{
    const EXHAUSTIVE_THRESHOLD: usize = 8 * 1024 * 1024;
    input_len <= EXHAUSTIVE_THRESHOLD
        || find(name).is_none_or(|codec| codec.family != "nrv" && !codec.slow_for_chunks)
}

pub fn familyLabel(family: &str) -> &'static str
{
    match family
    {
        "transform" => "transforms",
        "rle" => "run length",
        "lz" => "native LZ",
        "embedded" => "embedded",
        "entropy" => "entropy coders",
        "deflate" => "deflate",
        "bwt" => "BWT",
        "lzma" => "LZMA",
        "zstd" => "zstd",
        "brotli" => "brotli",
        "fast-lz" => "fast LZ",
        "lzrw" => "LZRW",
        "lzham" => "LZHAM",
        "nrv" => "UCL NRV",
        "density" => "Density",
        "dictionary" => "dictionary",
        "context" => "context",
        _ => "other",
    }
}

pub fn decoderHint(codec: &CodecMeta) -> &'static str
{
    match codec.family
    {
        "deflate" => "zlib stream (RFC 1950)",
        "bwt" => "libbz2/bzip3",
        "lzma" => "liblzma",
        "zstd" => "libzstd",
        "brotli" => "libbrotli",
        "fast-lz" => "LZ4/Snappy/LZO",
        "lzrw" => "LZRW",
        "lzham" => "LZHAM",
        "nrv" => "UCL NRV",
        "density" => "Density",
        "dictionary" => "built-in or LZW",
        "context" => "PPMd7",
        _ => "built-in",
    }
}
