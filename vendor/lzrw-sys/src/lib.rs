//! Raw FFI bindings to the LZRW family of lossless data-compression algorithms.
//!
//! This crate vendors the original C functions from the LZRW family of
//! compressors (`lzrw1`, `lzrw1a`, `lzrw2`, `lzrw3`, `lzrw3a`, `lzrw5`)
//! with no safe abstractions on top. It is intended to be used as the foundation
//! for the higher-level, safe wrapper crate [`lzrw`](https://crates.io/crates/lzrw).
//!
//! # Safety
//!
//! Every function in this crate is `unsafe`. Callers are responsible for:
//! - Providing a working-memory buffer that is large enough for the algorithm.
//!   (use [`get_wrk_mem_size`] to determine the required size)
//! - Ensuring `src_adr` points to at least `src_len` bytes of valid data.
//! - Ensuring `dst_adr` points to a buffer large enough to hold the output.
//! - Ensuring `dst_len` points to a valid, writable `u32`.
//!
//! See the `lzrw` crate for a safe, ergonomic API on top of these bindings.

use std::ptr;

/// Action code for requesting the identity of the algorithm.
pub const COMPRESS_ACTION_IDENTITY: u16 = 0;

/// Action code for compressing `src` into `dst`.
pub const COMPRESS_ACTION_COMPRESS: u16 = 1;

/// Action code for decompressing `src` into `dst`.
pub const COMPRESS_ACTION_DECOMPRESS: u16 = 2;

/// Additional space required after the compressed data to account for algorithm overhead.
pub const COMPRESS_OVERRUN: usize = 1024;

/// Identity record returned by an algorithm's compressor function when invoked
/// with [`COMPRESS_ACTION_IDENTITY`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CompressIdentity {
    /// The algorithm ID.
    pub id: u32,
    /// The amount of memory required for the algorithm.
    pub memory: u32,
    /// The name of the algorithm.
    pub name: *const u8,
    /// The version of the algorithm.
    pub version: *const u8,
    /// The date of the algorithm.
    pub date: *const u8,
    /// The copyright of the algorithm.
    pub copyright: *const u8,
    /// The author of the algorithm.
    pub author: *const u8,
}

unsafe extern "C" {
    /// Raw binding to the `lzrw1` compressor/decompressor function.
    ///
    /// # Safety
    ///
    /// See the [safety notes above](crate#safety).
    pub unsafe fn lzrw1_compress(
        action: u16,
        wrk_mem: *mut u8,
        src_adr: *const u8,
        src_len: u32,
        dst_adr: *mut u8,
        dst_len: *mut u32,
    );

    /// Raw binding to the `lzrw1a` compressor/decompressor function.
    ///
    /// # Safety
    ///
    /// See the [safety notes above](crate#safety).
    pub unsafe fn lzrw1a_compress(
        action: u16,
        wrk_mem: *mut u8,
        src_adr: *const u8,
        src_len: u32,
        dst_adr: *mut u8,
        dst_len: *mut u32,
    );

    /// Raw binding to the `lzrw2` compressor/decompressor function.
    ///
    /// # Safety
    ///
    /// See the [safety notes above](crate#safety).
    pub unsafe fn lzrw2_compress(
        action: u16,
        wrk_mem: *mut u8,
        src_adr: *const u8,
        src_len: u32,
        dst_adr: *mut u8,
        dst_len: *mut u32,
    );

    /// Raw binding to the `lzrw3` compressor/decompressor function.
    ///
    /// # Safety
    ///
    /// See the [safety notes above](crate#safety).
    pub unsafe fn lzrw3_compress(
        action: u16,
        wrk_mem: *mut u8,
        src_adr: *const u8,
        src_len: u32,
        dst_adr: *mut u8,
        dst_len: *mut u32,
    );

    /// Raw binding to the `lzrw3a` compressor/decompressor function.
    ///
    /// # Safety
    ///
    /// See the [safety notes above](crate#safety).
    pub unsafe fn lzrw3a_compress(
        action: u16,
        wrk_mem: *mut u8,
        src_adr: *const u8,
        src_len: u32,
        dst_adr: *mut u8,
        dst_len: *mut u32,
    );

    /// Raw binding to the `lzrw5` compressor/decompressor function.
    ///
    /// # Safety
    ///
    /// See the [safety notes above](crate#safety).
    pub unsafe fn lzrw5_compress(
        action: u16,
        wrk_mem: *mut u8,
        src_adr: *const u8,
        src_len: u32,
        dst_adr: *mut u8,
        dst_len: *mut u32,
    );
}

/// Function signature for the compress functions used by all algorithms.
pub type CompressFn = unsafe extern "C" fn(
    action: u16,
    wrk_mem: *mut u8,
    src_adr: *const u8,
    src_len: u32,
    dst_adr: *mut u8,
    dst_len: *mut u32,
);

/// Queries the amount of memory required for the given algorithm by
/// invoking the compress function with [`COMPRESS_ACTION_IDENTITY`],
/// returning the memory size from the [`CompressIdentity`] struct.
pub fn get_wrk_mem_size(compress_fn: CompressFn) -> usize {
    let mut identity_addr: usize = 0;
    unsafe {
        compress_fn(
            COMPRESS_ACTION_IDENTITY,
            ptr::null_mut(),
            ptr::null(),
            0,
            ptr::null_mut(),
            &mut identity_addr as *mut usize as *mut u32,
        );
    }
    if identity_addr == 0 {
        return 0;
    }
    let identity_ptr = identity_addr as *const CompressIdentity;
    unsafe { (*identity_ptr).memory as usize }
}
