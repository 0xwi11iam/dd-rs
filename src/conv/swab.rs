/// Byte swap: conv=swab swaps every pair of input bytes.
///
/// If the length is odd, the last byte is left unchanged.
/// This was originally used for endianness conversion on PDP-11 data.

pub fn swab_bytes(buf: &mut [u8]) -> usize {
    let end = buf.len() & !1; // round down to even
    for i in (0..end).step_by(2) {
        buf.swap(i, i + 1);
    }
    buf.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swab() {
        let mut buf = [0x01, 0x02, 0x03, 0x04, 0x05];
        swab_bytes(&mut buf);
        assert_eq!(buf, [0x02, 0x01, 0x04, 0x03, 0x05]);
    }

    #[test]
    fn test_swab_even() {
        let mut buf = [0xAA, 0xBB, 0xCC, 0xDD];
        swab_bytes(&mut buf);
        assert_eq!(buf, [0xBB, 0xAA, 0xDD, 0xCC]);
    }

    #[test]
    fn test_swab_empty() {
        let mut buf: [u8; 0] = [];
        swab_bytes(&mut buf);
    }
}
