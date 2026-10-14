# Vendored LZHAM binding

This directory contains `lzham-sys` 0.1.1 and its bundled `lzham_codec`
C++ sources.

Local build fixes:

- allow the legacy upstream CMake policy level with current CMake;
- link libc++ on macOS and libstdc++ on other Unix targets;
- build on arm64: the debug-break uses `__builtin_trap()` instead of x86
  `int $3`, and the `pause` spin in `lzham_yield_processor` is compiled only
  for i386/x86_64 (upstream guarded it by platform, not by architecture).

The upstream MIT license files are retained as `LICENSE` and
`lzham_codec/LICENSE`.
