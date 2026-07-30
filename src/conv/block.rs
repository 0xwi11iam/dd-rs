/// Block / Unblock conversion.
///
/// ## Block (conv=block)
/// Converts variable-length newline-terminated records into fixed-length blocks.
/// Each newline-terminated record is padded with spaces to `cbs` bytes.
/// If a record is longer than `cbs`, it is truncated. The newline is replaced
/// by spaces.
///
/// ## Unblock (conv=unblock)
/// Converts fixed-length blocks back into newline-terminated records.
/// Trailing spaces in each `cbs`-sized block are replaced by a single newline.

use crate::conv::ConvContext;
use crate::error::Result;

/// Pad a newline-terminated record to `cbs` bytes with spaces.
/// The trailing newline is consumed and replaced by spaces.
/// If the record exceeds `cbs`, it is truncated to `cbs` (the newline is dropped).
pub fn block_record(buf: &mut [u8], ctx: &ConvContext) -> Result<usize> {
    let cbs = ctx.cbs;
    let len = buf.len();

    // Find the newline position
    let nl_pos = buf.iter().position(|&b| b == b'\n');

    match nl_pos {
        Some(pos) => {
            let data_len = pos; // length before newline
            if data_len >= cbs {
                // Record too long: truncate to cbs
                // The output is exactly cbs bytes of data (newline dropped)
                Ok(cbs)
            } else {
                // Pad with spaces up to cbs
                let pad_start = data_len;
                let pad_end = cbs.min(len);
                for i in pad_start..pad_end {
                    buf[i] = b' ';
                }
                Ok(cbs.min(len))
            }
        }
        None => {
            // No newline found; if len >= cbs, truncate; otherwise pad
            if len >= cbs {
                Ok(cbs)
            } else if len < buf.len() {
                // This shouldn't happen normally, but be safe
                Ok(len)
            } else {
                Ok(len)
            }
        }
    }
}

/// Replace trailing spaces in a cbs-sized block with a single newline.
/// Returns the number of meaningful bytes (including the newline).
pub fn unblock_record(buf: &mut [u8], ctx: &mut ConvContext) -> Result<usize> {
    let cbs = ctx.cbs;

    // Find the last non-space byte
    if let Some(last_non_space) = buf.iter().rposition(|&b| b != b' ') {
        let new_len = last_non_space + 1;
        if new_len < buf.len() {
            buf[new_len] = b'\n';
            Ok(new_len + 1)
        } else if new_len < cbs {
            // All meaningful bytes, no trailing spaces, but shorter than cbs
            buf[new_len] = b'\n';
            Ok(new_len + 1)
        } else {
            Ok(new_len)
        }
    } else {
        // All spaces: just a newline
        buf[0] = b'\n';
        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_simple() {
        let ctx = ConvContext::new(80, 512);
        let mut buf = b"hello\n".to_vec();
        let len = buf.len();
        buf.resize(80, 0);
        let result = block_record(&mut buf[..len], &ctx).unwrap();
        // result should be min(cbs, original buffer)
        // But our block_record doesn't resize the buffer; it just returns the effective length
        // and pads in-place up to available space. The caller is responsible for using the
        // right buffer size. This test illustrates the expected padding behavior:
        assert_eq!(buf[0], b'h');
        assert_eq!(buf[4], b'o');
        // position 5 (where \n was) should now be space
        assert_eq!(buf[5], b' ');
    }

    #[test]
    fn test_unblock_simple() {
        let mut ctx = ConvContext::new(80, 512);
        let mut buf = b"hello                                                                           ".to_vec(); // 80 bytes: 5 chars + 75 spaces
        let result = unblock_record(&mut buf, &mut ctx).unwrap();
        assert_eq!(result, 6);
        assert_eq!(&buf[..6], b"hello\n");
    }
}
