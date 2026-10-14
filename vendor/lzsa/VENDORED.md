# Vendored LZSA

- Upstream: https://github.com/emmanuel-marty/lzsa
- Version: 1.4.1
- Archive: `https://github.com/emmanuel-marty/lzsa/archive/refs/tags/1.4.1.tar.gz`
- LZSA license: zlib (`LICENSE.zlib.md`)
- Match finder license: CC0 (`LICENSE.cc0.md`)
- libdivsufsort license: MIT (`src/libdivsufsort/LICENSE`)

The sources are compiled by the repository root `build.rs`. ShannonsPrism uses
the upstream in-memory API to emit and decode framed LZSA2 streams wrapped with
an output-length header.
