/// Case conversion: lcase / ucase.
///
/// Only affects ASCII letters A-Z / a-z.

/// Map uppercase A-Z to lowercase a-z in-place.
pub fn lcase(buf: &mut [u8]) -> usize {
    for byte in buf.iter_mut() {
        if (b'A'..=b'Z').contains(byte) {
            *byte += 32;
        }
    }
    buf.len()
}

/// Map lowercase a-z to uppercase A-Z in-place.
pub fn ucase(buf: &mut [u8]) -> usize {
    for byte in buf.iter_mut() {
        if (b'a'..=b'z').contains(byte) {
            *byte -= 32;
        }
    }
    buf.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lcase() {
        let mut buf = b"Hello, WORLD!".to_vec();
        lcase(&mut buf);
        assert_eq!(&buf, b"hello, world!");
    }

    #[test]
    fn test_ucase() {
        let mut buf = b"Hello, world!".to_vec();
        ucase(&mut buf);
        assert_eq!(&buf, b"HELLO, WORLD!");
    }
}
