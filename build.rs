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

fn main()
{
    let root = "vendor/lzsa/src";
    let mut build = cc::Build::new();
    build
        .include(root)
        .include(format!("{root}/libdivsufsort/include"))
        .warnings(false)
        .opt_level(3)
        .flag_if_supported("-fomit-frame-pointer");

    for file in [
        "dictionary.c",
        "expand_block_v1.c",
        "expand_block_v2.c",
        "expand_context.c",
        "expand_inmem.c",
        "expand_streaming.c",
        "frame.c",
        "matchfinder.c",
        "shrink_block_v1.c",
        "shrink_block_v2.c",
        "shrink_context.c",
        "shrink_inmem.c",
        "shrink_streaming.c",
        "stream.c",
        "libdivsufsort/lib/divsufsort.c",
        "libdivsufsort/lib/divsufsort_utils.c",
        "libdivsufsort/lib/sssort.c",
        "libdivsufsort/lib/trsort.c",
    ]
    {
        build.file(format!("{root}/{file}"));
    }

    build.compile("lzsa");
    println!("cargo:rustc-link-lib=static=lzsa");
    println!("cargo:rerun-if-changed=vendor/lzsa/src");

    let root = "vendor/ucl";
    let mut build = cc::Build::new();
    build
        .include(root)
        .include(format!("{root}/include"))
        .warnings(false)
        .opt_level(3)
        .flag_if_supported("-fomit-frame-pointer");

    for file in [
        "alloc.c",
        "n2b_99.c",
        "n2b_d.c",
        "n2b_ds.c",
        "n2b_to.c",
        "n2d_99.c",
        "n2d_d.c",
        "n2d_ds.c",
        "n2d_to.c",
        "n2e_99.c",
        "n2e_d.c",
        "n2e_ds.c",
        "n2e_to.c",
        "ucl_crc.c",
        "ucl_ptr.c",
        "ucl_str.c",
        "ucl_util.c",
    ]
    {
        build.file(format!("{root}/src/{file}"));
    }

    build.compile("ucl");
    println!("cargo:rustc-link-lib=static=ucl");
    println!("cargo:rerun-if-changed=vendor/ucl");
}
