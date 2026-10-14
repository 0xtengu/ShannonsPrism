# lzrw-sys

[![Crates.io](https://img.shields.io/crates/v/lzrw-sys.svg)](https://crates.io/crates/lzrw-sys)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Raw FFI bindings to the [LZRW](https://en.wikipedia.org/wiki/LZRW) family of lossless data-compression algorithms.

This crate vendors the original C source by [Ross Williams](http://www.ross.net/compression/) and compiles it via [`cc`](https://crates.io/crates/cc). It exposes the raw `lzrw*_compress` functions as unsafe extern C bindings.
Most users will want to use the [`lzrw`](https://crates.io/crates/lzrw), a safe and ergonomic Rust API on top of these bindings.

## Authors

This crate was developed and is maintained by [Brandon "SaturnKai" Gardenhire](https://panka.io).

The original **LZRW** source was released by **Ross Williams** into the public domain.
The source has been slightly modified to be compiled on modern platforms, available in the `lzrw-sys/lzrw/` directory.

## License

MIT License, see [LICENSE](LICENSE) for more details.
