# Vendored LZRW binding

This directory contains `lzrw-sys` 0.1.0 and the bundled original C
implementations of LZRW1, LZRW1-A, LZRW2, LZRW3, LZRW3-A and LZRW5.

Local compatibility fixes give internal routines static prototypes, use the
shared `fast_copy` implementation, and suppress warnings from the historical
K&R-style C sources. The upstream MIT license is retained as `LICENSE`.
