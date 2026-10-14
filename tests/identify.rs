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
//! `format::identify` decides whether a sweep is worth running. Uncompressed archive
//! containers must not be flagged as precompressed, or every codec is written off before
//! the first one runs.

/// A newc cpio with one empty regular file and the trailer, as `cpio -o -H newc` emits it.
fn newcCpio() -> Vec<u8>
{
    fn entry(name: &str, mode: u32) -> Vec<u8>
    {
        let mut v = Vec::new();
        v.extend_from_slice(b"070701");
        for field in [0u32, mode, 0, 0, 1, 0, 0, 0, 0, 0, 0, name.len() as u32 + 1, 0]
        {
            v.extend_from_slice(format!("{field:08x}").as_bytes());
        }
        v.extend_from_slice(name.as_bytes());
        v.push(0);
        while v.len() % 4 != 0
        {
            v.push(0);
        }
        v
    }
    let mut v = entry("init", 0o100755);
    v.extend_from_slice(&entry("TRAILER!!!", 0));
    v
}

#[test]
fn newcCpioIsNotPrecompressed()
{
    let id = sprism::format::identify(&newcCpio());
    assert_eq!(id.short, "cpio", "fixture should be recognised as cpio, got {}", id.label);
    assert!(!id.precompressed, "a plain cpio holds raw bytes; sweeping it is the whole point");
}

#[test]
fn gzipIsPrecompressed()
{
    let gz: &[u8] = &[
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x4b, 0x4c, 0x4a, 0x06, 0x00, 0xc2,
        0x41, 0x24, 0x35, 0x03, 0x00, 0x00, 0x00,
    ];
    let id = sprism::format::identify(gz);
    assert_eq!(id.short, "gz");
    assert!(id.precompressed);
}
