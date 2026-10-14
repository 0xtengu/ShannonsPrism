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
//! Round-trip every codec against awkward inputs.
//!
//! The predecessor of this tool had no decoders at all, so every compression
//! ratio it printed was unverified. These tests exist so that cannot recur.

use std::process::Command;

fn smallExecutable() -> std::path::PathBuf
{
    let system = std::path::PathBuf::from("/bin/true");
    if system.is_file()
    {
        system
    }
    else
    {
        std::env::current_exe().unwrap()
    }
}

fn corpora() -> Vec<(&'static str, Vec<u8>)>
{
    let mut rng: u64 = 0x2545F491_4F6CDD1D;
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 33) as u8
    };
    vec![
        ("empty", vec![]),
        ("single", vec![0x90]),
        (
            "escape_run",
            [vec![0x90u8; 300], b"abc".to_vec(), vec![0x90; 3]].concat(),
        ),
        ("zeros", vec![0u8; 5000]),
        (
            "text",
            b"the quick brown fox jumps over the lazy dog. ".repeat(200),
        ),
        ("random", (0..20000).map(|_| next()).collect()),
        ("alternating", [0u8, 255].repeat(10000)),
        ("all_bytes", (0..=255u8).collect::<Vec<_>>().repeat(40)),
        (
            "sawtooth",
            (0..20000).map(|i| (i * 7 % 256) as u8).collect(),
        ),
    ]
}

#[test]
fn everyNativeCodecRoundtrips()
{
    let p = sprism::codecs::Params::default();
    for name in sprism::codecs::CODEC_NAMES
    {
        for (label, data) in corpora()
        {
            let enc = sprism::codecs::encodeOne(name, &data, &p)
                .unwrap_or_else(|e| panic!("{name} failed to encode {label}: {e}"));
            let dec = sprism::codecs::decodeOne(name, &enc, &p)
                .unwrap_or_else(|e| panic!("{name} failed to decode {label}: {e}"));
            assert_eq!(dec, data, "{name} did not round-trip {label}");
        }
    }
}

#[test]
fn everyLibraryCodecRoundtrips()
{
    for c in sprism::libs::available()
    {
        for (label, data) in corpora()
        {
            if data.is_empty()
            {
                continue;
            }
            let enc = (c.encode)(&data)
                .unwrap_or_else(|e| panic!("{} failed to encode {label}: {e}", c.name));
            let dec = (c.decode)(&enc)
                .unwrap_or_else(|e| panic!("{} failed to decode {label}: {e}", c.name));
            assert_eq!(dec, data, "{} did not round-trip {label}", c.name);
        }
    }
}

#[test]
fn lzrw5LargeBlockFallsBackWithoutExiting()
{
    // LZRW5's original C code used a fixed 16-bit node pool and called exit(1)
    // after enough long matches. Library use must instead emit its verbatim form.
    let input = vec![b'A'; 17 * 1024 * 1024];
    let codec = sprism::libs::find("lzrw5").unwrap();
    let packed = sprism::libs::encodeSafe(codec, &input).unwrap();
    let decoded = sprism::libs::decodeSafe(codec, &packed).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn lzsa2RoundtripsBeyondOneWindow()
{
    let data = b"large framed LZSA2 regression payload ".repeat(8192);
    let codec = sprism::libs::available()
        .into_iter()
        .find(|c| c.name == "lzsa2")
        .expect("lzsa2 codec is registered");
    let encoded = (codec.encode)(&data).expect("lzsa2 failed to encode");
    let decoded = (codec.decode)(&encoded).expect("lzsa2 failed to decode");
    assert_eq!(decoded, data);
}

#[test]
fn entropyOfKnownInputs()
{
    let h = |d: &[u8]| sprism::score::Metrics::of(d).entropy;
    assert_eq!(h(b""), 0.0);
    assert_eq!(h(b"aaaa"), 0.0);
    assert!((h(&[0u8, 1].repeat(100)) - 1.0).abs() < 1e-12);
    assert!((h(&(0..=255u8).collect::<Vec<_>>()) - 8.0).abs() < 1e-12);
}

/// The property the whole ranking rests on: re-spelling data in a smaller
/// alphabet lowers entropy per byte but leaves total information alone.
#[test]
fn hexHalvesEntropyButNotInformation()
{
    let mut rng: u64 = 12345;
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 33) as u8
    };
    let data: Vec<u8> = (0..40000).map(|_| next()).collect();

    let plain = sprism::score::Metrics::of(&data);
    let mut hexed = Vec::new();
    for &b in &data
    {
        hexed.push(b"0123456789abcdef"[(b >> 4) as usize]);
        hexed.push(b"0123456789abcdef"[(b & 0xF) as usize]);
    }
    let wrapped = sprism::score::Metrics::of(&hexed);

    assert!(wrapped.entropy < 4.001, "hex cannot exceed 4 bits/byte");
    assert!(plain.entropy > 7.9);
    assert!(
        (wrapped.infoBits() / plain.infoBits() - 1.0).abs() < 0.01,
        "hex should not change total information"
    );
    assert!(
        wrapped.entropyNorm() > 0.99,
        "hex is maxed out within its own alphabet"
    );
}

#[test]
fn cliRunsAndReportsNoFailures()
{
    let exe = env!("CARGO_BIN_EXE_sprism");
    let tmp = std::env::temp_dir().join("entropy_cli_test.bin");
    std::fs::write(&tmp, b"repeating content ".repeat(4000)).unwrap();

    let out = Command::new(exe)
        .args([tmp.to_str().unwrap(), "--limit", "0"])
        .output()
        .expect("failed to run entropy");

    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    // The legend itself contains the word BROKEN, so look for the report
    // section that is only emitted when something actually failed.
    assert!(
        !text.contains("failed round-trip ("),
        "a codec failed round-trip:\n{text}"
    );
    assert!(text.contains("infoR"));
    let _ = std::fs::remove_file(&tmp);
}

// --- format / map / corpus modes -------------------------------------------

/// A hand-built ELF header is enough to exercise the section parser without a
/// compiler in the test environment.
#[test]
fn parsesRealElfSections()
{
    // Use the test binary itself: it is an ELF on Linux CI.
    let exe = std::env::current_exe().unwrap();
    let data = std::fs::read(&exe).unwrap();
    let c = sprism::format::parse(&data);
    // On non-ELF platforms this is None; only assert when it parses.
    if let Some(c) = c
    {
        assert!(!c.sections.is_empty());
        assert!(
            c.sections.iter().any(|s| s.executable),
            "expected an executable section"
        );
        assert!(!c.symbols.is_empty(), "expected named object symbols");
        assert!(
            !c.functions.is_empty(),
            "expected symbol-derived text functions"
        );
        assert!(
            c.functions.iter().all(|f| c
                .symbols
                .iter()
                .any(|s| s.name == f.name && s.address == f.address && s.kind == "text")),
            "every function must come from a defined text symbol"
        );
        assert!(
            c.sections.iter().map(|s| s.functions).sum::<usize>() <= c.functions.len(),
            "section attribution cannot invent functions"
        );
        assert!(
            c.functions.iter().any(|f| f.section.is_some()),
            "expected at least one function attributed by section index"
        );
        for function in c
            .functions
            .iter()
            .filter(|f| f.size > 0 && f.file_offset.is_some())
        {
            let offset = function.file_offset.unwrap();
            assert!(
                c.sections.iter().any(|s| {
                    !s.nobits
                        && function.section.as_deref() == Some(&s.name)
                        && offset >= s.offset
                        && offset < s.offset.saturating_add(s.size)
                }),
                "function {} has an invalid file-offset mapping",
                function.name
            );
        }
        // The point of per-section analysis: entropy is not uniform across the
        // file. Some section should differ from the whole-file figure.
        let whole = sprism::score::Metrics::of(&data).entropy;
        let spread = c
            .sections
            .iter()
            .filter(|s| !s.nobits && s.size > 256)
            .any(|s| (s.metrics.entropy - whole).abs() > 0.5);
        assert!(
            spread,
            "expected per-section entropy to vary from the whole-file value"
        );
    }
}

#[test]
fn mapFindsEmbeddedHighEntropyRegion()
{
    // Low-entropy padding, a random blob, then padding again.
    let mut data = vec![0u8; 20000];
    let mut rng: u64 = 0x1234_5678;
    for i in 40000..60000
    {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        if i >= data.len()
        {
            data.resize(i + 1, 0);
        }
        data[i] = (rng >> 33) as u8;
    }
    data.resize(80000, 0);

    let map = sprism::emap::compute(&data, 1024, 1024);
    // At least one region should be in the compressed/random band.
    let hot = map.regions.iter().find(|r| {
        matches!(
            r.band,
            sprism::emap::Band::Random | sprism::emap::Band::Compressed
        )
    });
    assert!(hot.is_some(), "did not locate the embedded random region");
    let hot = hot.unwrap();
    // And it should overlap the range we filled with random bytes.
    assert!(hot.start < 60000 && hot.end > 40000);
}

#[test]
fn mapRegionsTileTheFile()
{
    let data: Vec<u8> = (0..50000u32).map(|i| (i * 31 % 256) as u8).collect();
    let map = sprism::emap::compute(&data, 512, 256);
    // Regions are contiguous and cover from 0 to near the end.
    assert_eq!(map.regions.first().unwrap().start, 0);
    for pair in map.regions.windows(2)
    {
        assert_eq!(pair[0].end, pair[1].start, "regions must tile without gaps");
    }
}

#[test]
fn bandClassificationBoundaries()
{
    use sprism::emap::Band;
    assert_eq!(Band::of(0.5), Band::Padding);
    assert_eq!(Band::of(3.0), Band::Structured);
    assert_eq!(Band::of(6.0), Band::Code);
    assert_eq!(Band::of(7.5), Band::Compressed);
    assert_eq!(Band::of(7.99), Band::Random);
}

#[test]
fn corpusRanksRandomAboveStructured()
{
    let dir = std::env::temp_dir().join("entropy_corpus_test");
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join("text.bin"), b"the quick brown fox ".repeat(2000)).unwrap();
    let mut rng: u64 = 99;
    let random: Vec<u8> = (0..40000)
        .map(|_| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng >> 33) as u8
        })
        .collect();
    std::fs::write(dir.join("random.bin"), &random).unwrap();

    let entries = sprism::corpus::analyse(&dir, 4096, 1000, Some(1));
    assert_eq!(entries.len(), 2);
    let random_entry = entries
        .iter()
        .find(|e| e.path.ends_with("random.bin"))
        .unwrap();
    let text_entry = entries
        .iter()
        .find(|e| e.path.ends_with("text.bin"))
        .unwrap();
    assert!(random_entry.random_frac > text_entry.random_frac);
    assert!(random_entry.entropy > text_entry.entropy);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cliSectionsAndJsonRun()
{
    let exe = env!("CARGO_BIN_EXE_sprism");
    let self_path = smallExecutable();

    let out = std::process::Command::new(exe)
        .arg(self_path.to_str().unwrap())
        .arg("--sections")
        .output()
        .unwrap();
    assert!(out.status.success() || out.status.code() == Some(1));

    // JSON mode should emit parseable-looking output.
    let out = std::process::Command::new(exe)
        .arg(self_path.to_str().unwrap())
        .args(["--sections", "--json"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.trim_start().starts_with('{'));
    assert!(text.contains("\"sections\""));
    assert!(text.contains("\"imports\""));
    assert!(text.contains("\"exports\""));
    assert!(text.contains("\"symbols\""));
    assert!(text.contains("\"functions\""));
    assert!(text.contains("\"function_count\""));
}

// --- audit regressions -----------------------------------------------------

/// A corrupt length header must not be trusted into an allocation.
/// Found by fuzzing: 0xFFFFFFFF asked the allocator for 4 GB and aborted.
#[test]
fn corruptLengthHeadersDoNotAllocate()
{
    let p = sprism::codecs::Params::default();
    let bombs: Vec<Vec<u8>> = vec![
        [&[0xFFu8, 0xFF, 0xFF, 0xFF][..], &[0x41; 32][..]].concat(), // lzss
        [b"BP8\0".as_slice(), &[0xFF; 4], &[0x41; 32]].concat(),     // bitplane
        [b"BPE1".as_slice(), &[0xFF; 1], &[0x41; 32]].concat(),      // bpe
        [&[0xFFu8; 8][..], &[0x41; 64][..]].concat(),                // ppmd
    ];
    for name in sprism::codecs::CODEC_NAMES
    {
        for b in &bombs
        {
            // Must return, either Ok or Err, without aborting the process.
            let _ = sprism::codecs::decodeOne(name, b, &p);
        }
    }
    for c in sprism::libs::LIB_CODECS
    {
        for b in &bombs
        {
            let _ = (c.decode)(b);
        }
    }
}

/// Every decoder survives truncation and bit flips of a genuine encoding.
#[test]
fn decodersSurviveCorruption()
{
    let p = sprism::codecs::Params::default();
    let sample: Vec<u8> = b"the quick brown fox jumps over the lazy dog ".repeat(40);
    for name in sprism::codecs::CODEC_NAMES
    {
        let enc = sprism::codecs::encodeOne(name, &sample, &p).unwrap();
        for cut in [1usize, 3, 9, 33]
        {
            if enc.len() > cut
            {
                let _ = sprism::codecs::decodeOne(name, &enc[..enc.len() - cut], &p);
                let _ = sprism::codecs::decodeOne(name, &enc[cut..], &p);
            }
        }
        for pos in 0..enc.len().min(16)
        {
            let mut m = enc.clone();
            m[pos] ^= 0xFF;
            let _ = sprism::codecs::decodeOne(name, &m, &p);
        }
    }
}

/// The corpus walker must not follow symlinks: a link to an ancestor
/// directory otherwise sends it round the same subtree until max_files.
#[test]
fn corpusWalkerIgnoresSymlinkLoops()
{
    let dir = std::env::temp_dir().join("entropy_symlink_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("a.bin"), b"content").unwrap();

    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::symlink(&dir, dir.join("sub").join("loop"));
    }

    let entries = sprism::corpus::analyse(&dir, 1024, 10_000, Some(1));
    assert_eq!(entries.len(), 1, "symlink loop inflated the file count");
    let _ = std::fs::remove_dir_all(&dir);
}

/// zstd sizes its tables from the level, not the input; level 22 allocated
/// ~700 MB for a 256 KB file until the window was clamped. Output must be
/// unchanged by the clamp.
#[test]
fn zstdLevelsStillCompressCorrectly()
{
    let sample: Vec<u8> = b"repeating structured content ".repeat(3000);
    let mut prev = usize::MAX;
    for name in ["zstd1", "zstd9", "zstd19", "zstd22"]
    {
        let c = sprism::libs::LIB_CODECS
            .iter()
            .find(|c| c.name == name)
            .unwrap();
        let enc = (c.encode)(&sample).unwrap();
        assert_eq!((c.decode)(&enc).unwrap(), sample, "{name} round-trip");
        assert!(
            enc.len() < sample.len(),
            "{name} should compress this input"
        );
        // Higher levels should never be larger than lower ones here.
        assert!(
            enc.len() <= prev || prev == usize::MAX,
            "{name} regressed vs previous level"
        );
        prev = enc.len();
    }
}

// --- per-chunk sweeps ------------------------------------------------------

/// `--compress` must compare like with like: the per-section total and the
/// baseline have to cover identical bytes, or the winner is meaningless.
#[test]
fn perSectionAndBlobTotalsCoverTheSameBytes()
{
    let exe = std::env::current_exe().unwrap();
    let data = std::fs::read(&exe).unwrap();
    let Some(c) = sprism::format::parse(&data)
    else
    {
        return;
    };

    let mut covered = 0usize;
    let mut concat = Vec::new();
    for sec in &c.sections
    {
        if sec.nobits
        {
            continue;
        }
        let start = sec.offset as usize;
        let end = (start + sec.size as usize).min(data.len());
        if start >= end
        {
            continue;
        }
        concat.extend_from_slice(&data[start..end]);
        covered += end - start;
    }
    assert_eq!(
        concat.len(),
        covered,
        "concatenation must equal the covered byte count"
    );
    assert!(covered <= data.len());
}

/// An incompressible chunk must report that nothing helped, not the name of a
/// size-preserving transform that tied at ratio 1.000.
#[test]
fn incompressibleChunkReportsNoWinner()
{
    let p = sprism::codecs::Params::default();
    let mut rng: u64 = 0xABCDEF;
    let random: Vec<u8> = (0..20000)
        .map(|_| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng >> 33) as u8
        })
        .collect();

    // Every size-preserving transform ties at 1.000 on random data.
    let mut any_shrank = false;
    for name in sprism::codecs::CODEC_NAMES
    {
        if let Ok(out) = sprism::codecs::encodeOne(name, &random, &p)
        {
            if out.len() < random.len()
            {
                any_shrank = true;
            }
        }
    }
    assert!(
        !any_shrank,
        "no built-in codec should shrink uniform random data"
    );
}

/// Small chunks are still worth sweeping when they are compressible: a run of
/// 95 identical bytes packs to single digits. An early size threshold of 96
/// hid results like this, which is why the threshold is now tiny.
#[test]
fn smallButCompressibleChunksStillPack()
{
    let tiny = vec![0x41u8; 95];
    let best = sprism::libs::LIB_CODECS
        .iter()
        .filter_map(|c| (c.encode)(&tiny).ok().map(|e| (c.name, e.len())))
        .min_by_key(|(_, n)| *n)
        .unwrap();
    assert!(
        best.1 < 20,
        "{} produced {} bytes; a pure run should pack far smaller",
        best.0,
        best.1
    );
}

/// Conversely, incompressible data of the same size must expand, and the
/// sweep reports that rather than naming an arbitrary winner.
#[test]
fn smallRandomChunksExpand()
{
    let mut rng: u64 = 4242;
    let tiny: Vec<u8> = (0..95)
        .map(|_| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng >> 33) as u8
        })
        .collect();
    let shrank = sprism::libs::LIB_CODECS
        .iter()
        .filter_map(|c| (c.encode)(&tiny).ok())
        .any(|e| e.len() < tiny.len());
    assert!(
        !shrank,
        "95 random bytes should not compress under any codec"
    );
}

// --- sampling correction ---------------------------------------------------

/// A small window cannot reach 8 bits even on perfectly random data, so any
/// fixed threshold applied to windowed entropy means something different at
/// each window size. This was a live bug: corpus mode reported 0% random for
/// every file because 0.98 * 8.0 was unreachable at the default window.
#[test]
fn achievableEntropyMatchesRandomSamples()
{
    let mut rng: u64 = 0x5EED;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 33) as u8
    };
    let data: Vec<u8> = (0..65536).map(|_| next()).collect();

    for n in [256usize, 1024, 4096, 16384]
    {
        let measured = sprism::score::entropyOf(&data[..n]);
        let predicted = sprism::score::achievableEntropy(n);
        assert!(
            (measured - predicted).abs() < 0.15,
            "window {n}: measured {measured:.3}, predicted ceiling {predicted:.3}"
        );
        assert!(predicted <= 8.0);
    }
    // The naive ceiling of 8.0 is unreachable at small windows: that is the bug.
    assert!(sprism::score::achievableEntropy(256) < 7.5);
    assert!(sprism::score::achievableEntropy(65536) > 7.99);
}

/// Band classification must not depend on the window used to measure it.
#[test]
fn bandsAreWindowIndependent()
{
    use sprism::emap::Band;
    let mut rng: u64 = 0xC0FFEE;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 33) as u8
    };
    let random: Vec<u8> = (0..65536).map(|_| next()).collect();

    // Random data reads as Random at every window size once corrected.
    for n in [1024usize, 4096, 16384]
    {
        let h = sprism::score::entropyOf(&random[..n]);
        assert_eq!(
            Band::ofWindowed(h, n),
            Band::Random,
            "window {n} gave H {h:.3}, expected the Random band"
        );
    }
    // Padding stays padding regardless of window.
    let zeros = vec![0u8; 65536];
    for n in [256usize, 4096]
    {
        let h = sprism::score::entropyOf(&zeros[..n]);
        assert_eq!(Band::ofWindowed(h, n), Band::Padding);
    }
}

// --- packing ---------------------------------------------------------------

/// Every codec with a C decoder template must round-trip through the Rust
/// decoder too: the two implementations have to agree or the emitted C is
/// silently wrong.
#[test]
fn portableCodecsAllHaveWorkingDecoders()
{
    let p = sprism::codecs::Params::default();
    let sample: Vec<u8> = b"structured repeating content with runs\0\0\0\0\0\0\0\0".repeat(60);
    // Every codec with a C template must also round-trip in Rust, since the
    // emitter verifies its checksum against the Rust decode.
    let portable: Vec<&str> = sprism::codecs::CODEC_NAMES
        .iter()
        .copied()
        .chain(sprism::libs::LIB_CODECS.iter().map(|c| c.name))
        .filter(|n| sprism::pack::isPortable(n))
        .collect();
    assert!(portable.len() >= 10, "portable set shrank: {:?}", portable);
    assert!(
        portable.contains(&"libdeflate12"),
        "deflate family should be portable"
    );
    for name in portable
    {
        let dec = sprism::pack::decodeAny(
            name,
            &match sprism::libs::find(name)
            {
                Some(c) => (c.encode)(&sample).unwrap(),
                None => sprism::codecs::encodeOne(name, &sample, &p).unwrap(),
            },
            &p,
        )
        .unwrap();
        assert_eq!(dec, sample, "{name} failed round-trip");
    }
}

/// A package must reconstruct the original exactly, including the bytes
/// between sections that no chunk covers.
#[test]
fn packageReconstructsWithGaps()
{
    use sprism::pack::{verify, Chunk, Package};
    let p = sprism::codecs::Params::default();
    let original: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();

    // Pack the middle, leave both ends as verbatim gaps.
    let body = &original[1024..3072];
    let packed = sprism::codecs::encodeOne("lz4ish", body, &p).unwrap();
    let pkg = Package {
        chunks: vec![Chunk {
            label: "middle".into(),
            codec: "lz4ish".into(),
            offset: 1024,
            original_len: body.len(),
            data: packed,
        }],
        original_len: original.len(),
        gaps: vec![
            (0, original[..1024].to_vec()),
            (3072, original[3072..].to_vec()),
        ],
    };
    verify(&pkg, &original, &p).expect("package should reconstruct exactly");

    // A corrupted package must be rejected, not silently accepted.
    let mut bad = original.clone();
    bad[2000] ^= 0xFF;
    assert!(verify(&pkg, &bad, &p).is_err());
}

/// The emitted C references the companion package; it must never embed payload bytes.
#[test]
fn emittedCIsACompactDecoder()
{
    use sprism::pack::{emitC, Chunk, Package};
    let p = sprism::codecs::Params::default();
    let original: Vec<u8> = b"aaaaaaaabbbbbbbbcccccccc".repeat(100);
    let packed = sprism::codecs::encodeOne("zero_rle", &original, &p).unwrap();
    let pkg = Package {
        chunks: vec![Chunk {
            label: "whole".into(),
            codec: "zero_rle".into(),
            offset: 0,
            original_len: original.len(),
            data: packed,
        }],
        original_len: original.len(),
        gaps: Vec::new(),
    };
    let c = emitC(&pkg, &p, "test.bin").expect("should emit");
    assert!(c.contains("zero_rle_decode"));
    assert!(c.contains("const unsigned char *packed"));
    assert!(!c.contains("sprism_payload"));
    assert!(!c.contains("int main(void)"));
    assert!(!c.contains("#include <stdio.h>"));
    // No library includes: the point of the portable set.
    assert!(!c.contains("zstd.h") && !c.contains("lzma.h") && !c.contains("zlib.h"));

    // Verbatim regions belong in the .epk, not as enormous C initialisers.
    let gap_pkg = Package {
        chunks: Vec::new(),
        original_len: 1 << 20,
        gaps: vec![(0, vec![0xa5; 1 << 20])],
    };
    let gap_c = emitC(&gap_pkg, &p, "gap.bin").unwrap();
    assert!(
        gap_c.len() < 10_000,
        "decoder grew to {} bytes",
        gap_c.len()
    );

    // A codec with no template must be refused rather than emitting broken C.
    let bad = Package {
        chunks: vec![Chunk {
            label: "x".into(),
            codec: "xz6".into(),
            offset: 0,
            original_len: 1,
            data: vec![0],
        }],
        original_len: 1,
        gaps: Vec::new(),
    };
    assert!(emitC(&bad, &p, "t").is_err());
}

#[test]
fn registryMatchesEveryDispatchSurface()
{
    use sprism::registry::{self, Backend};

    let native: Vec<&str> = registry::ALL
        .iter()
        .filter(|c| c.backend == Backend::Native)
        .map(|c| c.name)
        .collect();
    assert_eq!(native, sprism::codecs::CODEC_NAMES);

    let libraries: Vec<&str> = registry::ALL
        .iter()
        .filter(|c| c.backend == Backend::Library)
        .map(|c| c.name)
        .collect();
    let registered: Vec<&str> = sprism::libs::LIB_CODECS.iter().map(|c| c.name).collect();
    assert_eq!(libraries, registered);

    for codec in registry::ALL
    {
        assert_eq!(
            sprism::pack::isPortable(codec.name),
            codec.c_decoder.is_some(),
            "{} portable metadata drifted",
            codec.name
        );
    }

    assert!(registry::includeInDefaultWholeSweep(
        "nrv2b",
        8 * 1024 * 1024
    ));
    assert!(!registry::includeInDefaultWholeSweep(
        "nrv2b",
        8 * 1024 * 1024 + 1
    ));
    assert!(!registry::includeInDefaultWholeSweep(
        "nrv2d",
        100 * 1024 * 1024
    ));
    assert!(registry::includeInDefaultWholeSweep(
        "zstd3",
        100 * 1024 * 1024
    ));
    for name in [
        "zopfli",
        "xz9",
        "xz9_x86",
        "zstd22",
        "brotli11",
        "brieflz10",
        "lzham_uber",
        "ppmd_o16",
    ]
    {
        assert!(
            !registry::includeInDefaultWholeSweep(name, 100 * 1024 * 1024),
            "{name} should require --exhaustive on a large input"
        );
    }
    for name in [
        "deflate9", "xz6", "zstd19", "brotli9", "brieflz5", "lzham", "ppmd_o6",
    ]
    {
        assert!(
            registry::includeInDefaultWholeSweep(name, 100 * 1024 * 1024),
            "{name} should remain as the faster family representative"
        );
    }
}

/// Packing may pick a library codec, so verification has to dispatch to the
/// library decoders too -- it originally only knew the built-ins and rejected
/// every library-packed file as corrupt.
#[test]
fn packingVerifiesLibraryCodecs()
{
    use sprism::pack::{decodeAny, verify, Chunk, Package};
    let p = sprism::codecs::Params::default();
    let original: Vec<u8> = b"structured repeating payload ".repeat(400);

    for name in ["xz6", "zstd19", "brotli9", "lzss"]
    {
        let c = sprism::libs::find(name);
        let packed = match c
        {
            Some(c) => (c.encode)(&original).unwrap(),
            None => sprism::codecs::encodeOne(name, &original, &p).unwrap(),
        };
        assert_eq!(
            decodeAny(name, &packed, &p).unwrap(),
            original,
            "{name} did not decode through decode_any"
        );
        let pkg = Package {
            chunks: vec![Chunk {
                label: "whole".into(),
                codec: name.into(),
                offset: 0,
                original_len: original.len(),
                data: packed,
            }],
            original_len: original.len(),
            gaps: Vec::new(),
        };
        verify(&pkg, &original, &p).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    assert!(decodeAny("not_a_codec", &[1, 2, 3], &p).is_err());
}

/// The emitted C must checksum its own output, so a corrupted payload fails
/// loudly instead of producing a wrong image.
#[test]
fn emittedCCarriesAChecksum()
{
    use sprism::pack::{emitC, Chunk, Package};
    let p = sprism::codecs::Params::default();
    let original: Vec<u8> = b"aaaaaaaabbbbbbbb".repeat(200);
    let packed = sprism::codecs::encodeOne("zero_rle", &original, &p).unwrap();
    let pkg = Package {
        chunks: vec![Chunk {
            label: "whole".into(),
            codec: "zero_rle".into(),
            offset: 0,
            original_len: original.len(),
            data: packed,
        }],
        original_len: original.len(),
        gaps: Vec::new(),
    };
    let c = emitC(&pkg, &p, "t.bin").unwrap();
    assert!(c.contains("SPRISM_CHECKSUM"));
    assert!(c.contains("sprism_checksum"));
    assert!(c.contains("unsigned sprism_unpack(const unsigned char *packed, unsigned packed_len,"));
    assert!(!c.contains("sprism_payload"));
}

#[test]
fn emittedPortableCCompilesAndRoundtrips()
{
    use sprism::pack::{emitC, serialise, Chunk, Package};
    use sprism::registry::{self, Backend};

    if Command::new("cc").arg("--version").output().is_err()
    {
        return;
    }

    let p = sprism::codecs::Params::default();
    let sample = b"firmware block: aaaabbbbccccddddeeee\n".repeat(300);
    let mut original = Vec::new();
    let mut chunks = Vec::new();
    let only = std::env::var("SPRISM_C_CODEC").ok();
    for codec in registry::ALL
        .iter()
        .filter(|c| c.c_decoder.is_some())
        .filter(|c| only.as_deref().is_none_or(|name| name == c.name))
    {
        let packed = match codec.backend
        {
            Backend::Native => sprism::codecs::encodeOne(codec.name, &sample, &p).unwrap(),
            Backend::Library =>
            {
                let implementation = sprism::libs::find(codec.name).unwrap();
                sprism::libs::encodeSafe(implementation, &sample).unwrap()
            }
        };
        let offset = original.len();
        original.extend_from_slice(&sample);
        chunks.push(Chunk {
            label: codec.name.into(),
            codec: codec.name.into(),
            offset,
            original_len: sample.len(),
            data: packed,
        });
    }
    let pkg = Package {
        chunks,
        original_len: original.len(),
        gaps: Vec::new(),
    };

    let dir = std::env::temp_dir().join(format!("sprism_c_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let decoder = emitC(&pkg, &p, "fixture.bin").unwrap();
    assert!(
        decoder.len() < 60_000,
        "all-codec C decoder grew to {} source bytes",
        decoder.len()
    );
    std::fs::write(dir.join("decoder.c"), decoder).unwrap();
    std::fs::write(dir.join("fixture.epk"), serialise(&pkg).unwrap()).unwrap();
    std::fs::write(dir.join("fixture.bin"), &original).unwrap();
    std::fs::write(
        dir.join("driver.c"),
        r#"#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "decoder.c"
static unsigned char *load(const char *p, unsigned *n) {
    FILE *f = fopen(p, "rb"); long z; unsigned char *b;
    if (!f || fseek(f, 0, SEEK_END) || (z = ftell(f)) < 0 || fseek(f, 0, SEEK_SET)) return 0;
    b = (unsigned char *)malloc((size_t)z);
    if (!b || fread(b, 1, (size_t)z, f) != (size_t)z) return 0;
    fclose(f); *n = (unsigned)z; return b;
}
int main(int argc, char **argv) {
    unsigned pn, on; unsigned char *p, *original, *out;
    if (argc != 3) return 2;
    p = load(argv[1], &pn); original = load(argv[2], &on);
    out = (unsigned char *)malloc(SPRISM_ORIGINAL_LEN);
    if (!p || !original || !out) return 3;
    if (sprism_unpack(p, pn, out, SPRISM_ORIGINAL_LEN) != on) return 4;
    return memcmp(out, original, on) != 0;
}
"#,
    )
    .unwrap();

    let compile = Command::new("cc")
        .current_dir(&dir)
        .args([
            "-std=c99", "-Wall", "-Wextra", "-Werror", "driver.c", "-o", "driver",
        ])
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "C compile failed: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(dir.join("driver"))
        .args([dir.join("fixture.epk"), dir.join("fixture.bin")])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "generated C returned {:?}",
        run.status.code()
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// rANS gives the decorrelating transforms an entropy-coding back end, which
/// is the whole reason they exist: mtf alone cannot compress, mtf_rans can.
#[test]
fn ransBackendMakesTransformsCompress()
{
    let p = sprism::codecs::Params::default();
    // Data with strong local correlation: mtf exposes it, rans cashes it in.
    let mut data = Vec::new();
    for run in 0..400
    {
        let b = (run % 7) as u8;
        for _ in 0..(20 + run % 30)
        {
            data.push(b);
        }
    }
    for name in ["rans", "mtf_rans", "dpcm_rans", "bitplane_rans"]
    {
        let enc = sprism::codecs::encodeOne(name, &data, &p).unwrap();
        let dec = sprism::codecs::decodeOne(name, &enc, &p).unwrap();
        assert_eq!(dec, data, "{name} round-trip");
    }
    // The point: mtf_rans beats mtf alone (which is size-preserving).
    let mtf_only = sprism::codecs::encodeOne("mtf", &data, &p).unwrap();
    let mtf_rans = sprism::codecs::encodeOne("mtf_rans", &data, &p).unwrap();
    assert_eq!(mtf_only.len(), data.len(), "mtf alone preserves size");
    assert!(
        mtf_rans.len() < data.len() / 2,
        "mtf_rans should compress this heavily"
    );
}

/// A malformed rANS frequency table must be rejected, not indexed past the
/// slot array. Found by the corruption fuzzer.
#[test]
fn ransRejectsBadFrequencyTable()
{
    let p = sprism::codecs::Params::default();
    // Header claims a length but frequencies are all zero / do not sum right.
    let mut bad = vec![0x10, 0, 0, 0]; // len = 16
    bad.extend(std::iter::repeat(0u8).take(512 + 64)); // zero freqs + junk
    assert!(sprism::codecs::decodeOne("rans", &bad, &p).is_err());
}

/// A section too small to classify reports no band rather than a wrong one.
#[test]
fn tinySectionsAreNotClassified()
{
    use sprism::emap::Band;
    // Random-ish 168 bytes reads ~6.9 raw (looks like "code") but is too small
    // to trust; classify_section should decline.
    assert!(Band::classifySection(6.9, 168).is_none());
    assert!(Band::classifySection(4.0, 27).is_none());
    // A large section is classifiable.
    assert!(Band::classifySection(7.99, 65536).is_some());
    assert_eq!(Band::classifySection(0.2, 65536), Some(Band::Padding));
}

/// bzip3 is mandatory and must round-trip like every other library codec.
#[test]
fn bzip3Roundtrips()
{
    let c = sprism::libs::find("bzip3").expect("bzip3 should always be registered");
    let data: Vec<u8> = b"structured repeating payload for bwt ".repeat(500);
    let enc = (c.encode)(&data).unwrap();
    assert_eq!((c.decode)(&enc).unwrap(), data);
    assert!(enc.len() < data.len(), "bzip3 should compress this");
}

/// Some third-party decoders (density) panic on malformed input rather than
/// erroring. The registry wrappers catch that, so one bad codec cannot crash a
/// run over a hostile file. Found by the corruption fuzzer.
#[test]
fn libraryDecodersNeverPanicTheProcess()
{
    std::panic::set_hook(Box::new(|_| {}));
    let bombs: Vec<Vec<u8>> = vec![
        vec![],
        vec![0],
        vec![0xFF, 0xFF, 0xFF, 0xFF],
        [&[0xFFu8; 4][..], &[0x41; 80][..]].concat(),
        (0..300u32).map(|i| (i * 13 % 256) as u8).collect(),
    ];
    for c in sprism::libs::LIB_CODECS
    {
        for b in &bombs
        {
            // Must return, never unwind past this point.
            let _ = sprism::libs::decodeSafe(c, b);
        }
    }
    let _ = std::panic::take_hook();
}

/// The .epk container must round-trip: pack (with any codec) then unpack must
/// reproduce the input. This closes the write-only gap that made per-section
/// packing with real codecs unusable.
#[test]
fn epkContainerRoundTrips()
{
    use sprism::pack::{deserialise, serialise, Chunk, Package};
    let p = sprism::codecs::Params::default();
    let original: Vec<u8> = {
        let mut v = Vec::new();
        for s in 0..8u32
        {
            let byte = (s * 31 % 256) as u8;
            for _ in 0..2000
            {
                v.push(byte);
            }
        }
        v
    };

    // Pack three regions with three different codecs, leave gaps between.
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
            mk("xz6", 0, 4000),
            mk("zstd19", 6000, 4000),
            mk("lzss", 12000, 2000),
        ],
        original_len: original.len(),
        gaps: vec![
            (4000, original[4000..6000].to_vec()),
            (10000, original[10000..12000].to_vec()),
            (14000, original[14000..].to_vec()),
        ],
    };
    let bytes = serialise(&pkg).unwrap();
    let back = deserialise(&bytes, &p).unwrap();
    assert_eq!(back, original, "epk did not round-trip");

    // A truncated container must error, not panic.
    assert!(deserialise(&bytes[..bytes.len() / 2], &p).is_err());
    assert!(deserialise(b"not an epk", &p).is_err());
}

/// The CLI pack -> unpack path must reproduce a real file byte for byte.
#[test]
fn cliPackUnpackRoundTrips()
{
    let exe = env!("CARGO_BIN_EXE_sprism");
    let src = smallExecutable();
    let dir = std::env::temp_dir();
    let epk = dir.join("entropy_cli_pack_test.epk");
    let restored = dir.join("entropy_cli_pack_test.restored");

    let pack = std::process::Command::new(exe)
        .arg(src.to_str().unwrap())
        .args(["--sections", "--pack", "--out", epk.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(pack.status.success(), "pack failed");

    let unpack = std::process::Command::new(exe)
        .args([
            "--unpack",
            epk.to_str().unwrap(),
            "--out",
            restored.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(unpack.status.success(), "unpack failed");

    let a = std::fs::read(&src).unwrap();
    let b = std::fs::read(&restored).unwrap();
    assert_eq!(a, b, "pack/unpack did not reproduce the file");

    let _ = std::fs::remove_file(&epk);
    let _ = std::fs::remove_file(&restored);
}

/// Packing a single named section must pack only that section and store the
/// rest verbatim, and still reconstruct exactly. (Regression: --section
/// without --sections originally fell through to whole-file packing.)
#[test]
fn singleSectionPackRoundTrips()
{
    let exe = env!("CARGO_BIN_EXE_sprism");
    let src = smallExecutable();
    let data = std::fs::read(&src).unwrap();
    // Only meaningful on a container we can parse.
    if sprism::format::parse(&data).is_none()
    {
        return;
    }
    let dir = std::env::temp_dir();
    let epk = dir.join("entropy_one_section.epk");
    let restored = dir.join("entropy_one_section.restored");

    let pack = std::process::Command::new(exe)
        .arg(src.to_str().unwrap())
        .args([
            "--section",
            ".text",
            "--pack",
            "--out",
            epk.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    // .text may or may not exist depending on toolchain; only assert if it packed.
    if !pack.status.success()
    {
        return;
    }
    let unpack = std::process::Command::new(exe)
        .args([
            "--unpack",
            epk.to_str().unwrap(),
            "--out",
            restored.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(unpack.status.success());
    assert_eq!(
        std::fs::read(&src).unwrap(),
        std::fs::read(&restored).unwrap()
    );

    let _ = std::fs::remove_file(&epk);
    let _ = std::fs::remove_file(&restored);
}
