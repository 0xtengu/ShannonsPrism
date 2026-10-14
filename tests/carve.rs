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

#![allow(non_snake_case)]
//! Embedded-stream carving and the chi-square split of the random band.

use sprism::{carve, emap};
use std::io::Write;

fn xorshift(seed: u64) -> impl FnMut() -> u8
{
    let mut s = seed;
    move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 33) as u8
    }
}

fn random(n: usize, seed: u64) -> Vec<u8>
{
    let mut r = xorshift(seed);
    (0..n).map(|_| r()).collect()
}

/// Something that compresses well but not trivially: pseudo-code with
/// structure, so codec output is neither tiny nor incompressible.
fn codeish(n: usize, seed: u64) -> Vec<u8>
{
    let mut r = xorshift(seed);
    let mut v = Vec::with_capacity(n);
    while v.len() < n
    {
        let op = r() % 8;
        match op
        {
            0 => v.extend_from_slice(&[0x48, 0x89, 0xe5]),
            1 => v.extend_from_slice(&[0xe8, r(), r(), 0x00, 0x00]),
            2 => v.extend_from_slice(&[0x55, 0x48, 0x83, 0xec, r() & 0x7f]),
            3 => v.extend_from_slice(b"error: %s\n\0"),
            4 => v.extend_from_slice(&[0x0f, 0x1f, 0x44, 0x00, 0x00]),
            _ => v.extend_from_slice(&[0x8b, r(), r()]),
        }
    }
    v.truncate(n);
    v
}

fn gz(d: &[u8]) -> Vec<u8>
{
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(d).unwrap();
    e.finish().unwrap()
}
fn zl(d: &[u8]) -> Vec<u8>
{
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(d).unwrap();
    e.finish().unwrap()
}
fn rawDeflate(d: &[u8]) -> Vec<u8>
{
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(d).unwrap();
    e.finish().unwrap()
}
fn xz(d: &[u8]) -> Vec<u8>
{
    let mut e = xz2::write::XzEncoder::new(Vec::new(), 6);
    e.write_all(d).unwrap();
    e.finish().unwrap()
}
fn lzmaAlone(d: &[u8]) -> Vec<u8>
{
    let opts = xz2::stream::LzmaOptions::new_preset(6).unwrap();
    let st = xz2::stream::Stream::new_lzma_encoder(&opts).unwrap();
    let mut e = xz2::write::XzEncoder::new_stream(Vec::new(), st);
    e.write_all(d).unwrap();
    e.finish().unwrap()
}
fn bz(d: &[u8]) -> Vec<u8>
{
    let mut e = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    e.write_all(d).unwrap();
    e.finish().unwrap()
}
fn zs(d: &[u8]) -> Vec<u8>
{
    zstd::encode_all(d, 3).unwrap()
}
fn lz4f(d: &[u8]) -> Vec<u8>
{
    let mut e = lz4_flex::frame::FrameEncoder::new(Vec::new());
    e.write_all(d).unwrap();
    e.finish().unwrap()
}

fn mapOf(d: &[u8]) -> emap::EntropyMap
{
    let w = (d.len() / 2048).clamp(64, 65536);
    emap::compute(d, w, w)
}

#[test]
fn chiSquareSeparatesRandomFromDeflateOutput()
{
    let rnd = random(200_000, 0x9e3779b97f4a7c15);
    let u = emap::uniformity(&rnd);
    assert!(
        u.isUniform(),
        "random bytes should pass: chi2 {} p {}",
        u.chi2,
        u.p
    );
    assert!(
        (150.0..400.0).contains(&u.chi2),
        "chi2 {} far from its expectation of 255",
        u.chi2
    );

    // Deflate output of any size carries Huffman structure the histogram sees.
    let packed = gz(&codeish(400_000, 7));
    assert!(packed.len() > 50_000);
    let u = emap::uniformity(&packed);
    assert!(
        !u.isUniform(),
        "deflate output should fail: chi2 {} p {}",
        u.chi2,
        u.p
    );

    assert_eq!(
        emap::Band::Random.describe(Some(emap::uniformity(&rnd))),
        "uniform-like high entropy"
    );
    assert_eq!(
        emap::Band::Random.describe(Some(emap::uniformity(&packed))),
        "nonuniform high entropy"
    );
    assert_eq!(emap::Band::Random.describe(None), "random");
    assert_eq!(
        emap::Band::Code.describe(Some(emap::uniformity(&rnd))),
        "code/text"
    );
}

#[test]
fn mapRegionsCarryUniformityForRandomBand()
{
    let mut img = vec![0u8; 20_000];
    img.extend(random(60_000, 42));
    img.extend(vec![0u8; 20_000]);
    let map = mapOf(&img);
    let rnd: Vec<&emap::Region> = map
        .regions
        .iter()
        .filter(|r| r.band == emap::Band::Random)
        .collect();
    assert!(
        !rnd.is_empty(),
        "no Random region found in a 60 KB random blob"
    );
    assert!(rnd.iter().all(|r| r.uniformity.is_some()));
    assert!(rnd
        .iter()
        .any(|r| r.describe() == "uniform-like high entropy"));
    let pad = map
        .regions
        .iter()
        .find(|r| r.band == emap::Band::Padding)
        .unwrap();
    assert!(pad.uniformity.is_none());
}

#[test]
fn carvesEveryHeaderedCodecWithExactLengths()
{
    let payload = codeish(60_000, 1);
    let streams: Vec<(&str, Vec<u8>)> = vec![
        ("xz", xz(&payload)),
        ("gzip", gz(&payload)),
        ("zlib", zl(&payload)),
        ("bzip2", bz(&payload)),
        ("zstd", zs(&payload)),
        ("lz4", lz4f(&payload)),
    ];
    let pad = vec![0xffu8; 8192];
    let mut img = codeish(30_000, 2);
    let mut expect = Vec::new();
    for (name, s) in &streams
    {
        img.extend_from_slice(&pad);
        expect.push((*name, img.len(), s.len()));
        img.extend_from_slice(s);
        // Trailing junk right after each stream: the reported length must stop
        // at the stream's real end, not run on into whatever follows.
        img.extend_from_slice(&random(3000, 99));
    }
    img.extend_from_slice(&pad);

    let map = mapOf(&img);
    let found = carve::carve(&img, &map, &carve::Options::default());
    for (name, off, len) in &expect
    {
        let f = found.iter().find(|f| f.offset == *off).unwrap_or_else(|| {
            panic!(
                "{} at 0x{:x} not found; found {:?}",
                name,
                off,
                found
                    .iter()
                    .map(|f| (f.codec, f.offset))
                    .collect::<Vec<_>>()
            )
        });
        assert_eq!(f.codec, *name);
        assert_eq!(f.stream_len, *len, "{}: wrong stream length", name);
        assert_eq!(
            f.decoded_len,
            payload.len(),
            "{}: wrong decoded length",
            name
        );
        assert!(f.by_magic);
        assert!(
            f.partial.is_none(),
            "{}: reported partial: {:?}",
            name,
            f.partial
        );
        assert_eq!(f.payload, payload);
    }
}

#[test]
fn probesHeaderlessLzmaAndDeflateAtRegionEdges()
{
    let payload = codeish(50_000, 3);
    let pad = vec![0xffu8; 8192];
    let mut img = codeish(20_000, 4);
    img.extend_from_slice(&pad);
    let lz_off = img.len();
    let lz = lzmaAlone(&payload);
    img.extend_from_slice(&lz);
    img.extend_from_slice(&pad);
    let df_off = img.len();
    let df = rawDeflate(&payload);
    img.extend_from_slice(&df);
    img.extend_from_slice(&pad);

    let map = mapOf(&img);
    let found = carve::carve(&img, &map, &carve::Options::default());
    let names: Vec<(&str, usize)> = found.iter().map(|f| (f.codec, f.offset)).collect();
    let l = found
        .iter()
        .find(|f| f.offset == lz_off)
        .unwrap_or_else(|| panic!("lzma-alone not found: {:?}", names));
    assert_eq!(l.codec, "lzma");
    assert_eq!(l.stream_len, lz.len());
    assert_eq!(l.decoded_len, payload.len());
    let d = found
        .iter()
        .find(|f| f.offset == df_off)
        .unwrap_or_else(|| panic!("raw deflate not found: {:?}", names));
    assert_eq!(d.codec, "deflate");
    assert_eq!(d.stream_len, df.len());
    assert_eq!(d.decoded_len, payload.len());
    assert!(!d.by_magic);
}

#[test]
fn randomDataYieldsNoFalseStreams()
{
    // 1 MB of noise: the magic scan and the blind probes both walk it.
    let img = random(1 << 20, 0xdead_beef_cafe_f00d);
    let map = mapOf(&img);
    let found = carve::carve(&img, &map, &carve::Options::default());
    assert!(
        found.is_empty(),
        "false positives on random data: {:?}",
        found
            .iter()
            .map(|f| (f.codec, f.offset, f.decoded_len))
            .collect::<Vec<_>>()
    );
}

#[test]
fn recursesOneLevelAndIdentifiesElfPayloads()
{
    let exe = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    let inner = zl(&codeish(40_000, 5));
    let outer = gz(&[inner.clone(), vec![0u8; 500]].concat());
    let mut img = vec![0xffu8; 8192];
    let elf_off = img.len();
    img.extend(xz(&exe));
    img.extend(vec![0xffu8; 8192]);
    let gz_off = img.len();
    img.extend(outer);
    img.extend(vec![0xffu8; 8192]);

    let map = mapOf(&img);
    let found = carve::carve(&img, &map, &carve::Options::default());
    let e = found
        .iter()
        .find(|f| f.offset == elf_off)
        .expect("xz-wrapped ELF not found");
    assert_eq!(e.decoded_len, exe.len());
    // The payload is this test binary, so expect the host's executable format.
    let native = if cfg!(target_os = "macos")
    {
        "Mach-O"
    }
    else if cfg!(windows)
    {
        "PE"
    }
    else
    {
        "ELF"
    };
    assert!(
        e.container
            .as_deref()
            .map(|c| c.starts_with(native))
            .unwrap_or(false),
        "payload not recognised as {}: {:?} / {}",
        native,
        e.container,
        e.identity
    );
    let g = found
        .iter()
        .find(|f| f.offset == gz_off)
        .expect("gzip not found");
    assert_eq!(
        g.children.len(),
        1,
        "nested zlib not carved: {:?}",
        g.children
            .iter()
            .map(|k| (k.codec, k.offset))
            .collect::<Vec<_>>()
    );
    assert_eq!(g.children[0].codec, "zlib");
    assert_eq!(g.children[0].decoded_len, 40_000);
    // Children past the recursion limit hold no payload.
    assert!(g.children[0].payload.is_empty());
}

#[test]
fn truncatedStreamIsReportedAsPartial()
{
    let payload = codeish(300_000, 6);
    let full = xz(&payload);
    let cut = &full[..full.len() * 2 / 3];
    let mut img = vec![0xffu8; 8192];
    let off = img.len();
    img.extend_from_slice(cut);
    img.extend(vec![0xffu8; 8192]);
    let map = mapOf(&img);
    let found = carve::carve(&img, &map, &carve::Options::default());
    let f = found
        .iter()
        .find(|f| f.offset == off)
        .expect("truncated xz not reported");
    assert!(f.partial.is_some());
    assert!(f.decoded_len >= 64 << 10 && f.decoded_len < payload.len());
    // The decoder reads some of what follows the cut before it notices, so
    // the consumed length overshoots slightly; it must not run far.
    assert!(
        f.stream_len <= cut.len() + 8192,
        "consumed {} for a {} byte cut",
        f.stream_len,
        cut.len()
    );
}

#[test]
fn decodeCapBoundsABomb()
{
    // 64 MB of zeros packs to a few KB; a cap of 1 MB must stop it there.
    let bomb = zl(&vec![0u8; 64 << 20]);
    assert!(bomb.len() < 100_000);
    let mut img = vec![0xffu8; 4096];
    let off = img.len();
    img.extend_from_slice(&bomb);
    img.extend(vec![0xffu8; 4096]);
    let opts = carve::Options {
        max_decoded: 1 << 20,
        ..Default::default()
    };
    // Force the probe rather than relying on the map: a 64 KB zlib stream of
    // zeros is itself nearly all repeated bytes, so its region may not be hot.
    let f = carve::probeAt(&img, off, "zlib", opts.max_decoded).expect("zlib bomb not decoded");
    assert!(f.capped);
    assert_eq!(f.decoded_len, 1 << 20);
}

#[test]
fn epkContainerRoundTripsAndRejectsHostileInput()
{
    use sprism::pack::{deserialise, serialise, Chunk, Package};
    let p = sprism::codecs::Params::default();
    let original = codeish(30_000, 8);
    let mk = |name: &str, off: usize, len: usize| -> Chunk {
        let body = &original[off..off + len];
        let packed = match sprism::libs::find(name)
        {
            Some(c) => (c.encode)(body).unwrap(),
            None => sprism::codecs::encodeOne(name, body, &p).unwrap(),
        };
        Chunk {
            label: name.into(),
            codec: name.into(),
            offset: off,
            original_len: len,
            data: packed,
        }
    };
    let pkg = Package {
        chunks: vec![
            mk("xz6", 0, 10_000),
            mk("zstd3", 12_000, 10_000),
            mk("lzss", 24_000, 4_000),
        ],
        original_len: original.len(),
        gaps: vec![
            (10_000, original[10_000..12_000].to_vec()),
            (22_000, original[22_000..24_000].to_vec()),
            (28_000, original[28_000..].to_vec()),
        ],
    };
    let bytes = serialise(&pkg).unwrap();
    assert_eq!(deserialise(&bytes, &p).unwrap(), original);
    assert!(deserialise(&bytes[..bytes.len() / 2], &p).is_err());
    assert!(deserialise(b"not an epk", &p).is_err());
    // A header claiming 4 GB with 30 KB of pieces must be refused, not allocated.
    let mut lie = bytes.clone();
    lie[5..9].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(deserialise(&lie, &p).is_err());
    // Layout offsets point at each chunk's payload.
    let lay = pkg.layout();
    for (c, off) in pkg.chunks.iter().zip(lay.iter())
    {
        assert_eq!(&bytes[*off..*off + c.data.len()], &c.data[..]);
    }
}
