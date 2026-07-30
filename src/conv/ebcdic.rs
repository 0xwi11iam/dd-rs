/// FFI bindings to the C EBCDIC conversion routines.
///
/// The actual conversion tables live in `c_src/ebcdic_tables.c` and are
/// compiled into a static library linked at build time.

use crate::error::Result;

// C function declarations (prefixed to avoid collision with Rust wrappers)
extern "C" {
    fn ebcdic_to_ascii(buf: *mut u8, len: usize) -> usize;
    fn ascii_to_ebcdic(buf: *mut u8, len: usize) -> usize;
    fn ibm_ebcdic_to_ascii(buf: *mut u8, len: usize) -> usize;
    fn ascii_to_ibm_ebcdic(buf: *mut u8, len: usize) -> usize;
}

/// Convert EBCDIC (CP037) to ASCII in-place.
pub fn conv_ebcdic_to_ascii(buf: &mut [u8]) -> Result<usize> {
    let len = buf.len();
    unsafe { ebcdic_to_ascii(buf.as_mut_ptr(), len); }
    Ok(len)
}

/// Convert ASCII to EBCDIC (CP037) in-place.
pub fn conv_ascii_to_ebcdic(buf: &mut [u8]) -> Result<usize> {
    let len = buf.len();
    unsafe { ascii_to_ebcdic(buf.as_mut_ptr(), len); }
    Ok(len)
}

/// Convert alternate EBCDIC (IBM1047) to ASCII in-place.
pub fn conv_ibm_ebcdic_to_ascii(buf: &mut [u8]) -> Result<usize> {
    let len = buf.len();
    unsafe { ibm_ebcdic_to_ascii(buf.as_mut_ptr(), len); }
    Ok(len)
}

/// Convert ASCII to alternate EBCDIC (IBM1047) in-place.
#[allow(dead_code)]
pub fn conv_ascii_to_ibm_ebcdic(buf: &mut [u8]) -> Result<usize> {
    let len = buf.len();
    unsafe { ascii_to_ibm_ebcdic(buf.as_mut_ptr(), len); }
    Ok(len)
}
