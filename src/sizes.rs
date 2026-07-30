/// Size suffix parsing.
///
/// Implements the full GNU dd size syntax:
///
/// | Suffix  | Multiplier        | Example           |
/// |---------|--------------------|-------------------|
/// | c       | 1                  | 10c = 10          |
/// | w       | 2                  | 10w = 20          |
/// | b       | 512                | 10b = 5120        |
/// | kB / KB | 1000 / 1000        | 4kB = 4000        |
/// | K / KiB | 1024 / 1024        | 4K = 4096         |
/// | M / MiB | 1024^2 / 1024^2    | 4M = 4194304      |
/// | MB      | 1000^2             | 4MB = 4000000     |
/// | G / GiB | 1024^3 / 1024^3    |                   |
/// | GB      | 1000^3             |                   |
/// | ... up to E/EiB/EB          |                   |
/// | xN      | N times number     | 4xM = 4*1048576   |
///
/// GNU extension: a bare 'B' suffix means bytes (not blocks):
///   count=512B → 512 bytes (not 512 blocks)
///
/// Numbers can use hex (0x...) or decimal notation.

use crate::error::{Error, Result};

/// A parsed size value in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub bytes: u64,
    /// Whether the original input ended in 'B' (GNU byte-count mode).
    /// This matters for `count` and `skip`/`seek` operands.
    pub explicit_bytes: bool,
}

/// Parse a size string like "4K", "10M", "512", "0x1000", "4xM", "512B".
pub fn parse_size(input: &str) -> Result<Size> {
    let input = input.trim();
    if input.is_empty() {
        return Err(Error::InvalidSize {
            input: input.to_string(),
            reason: "empty string".into(),
        });
    }

    // GNU extension: if it ends with 'B' (and isn't just "B" or ends with "iB"/"KB"/"MB"/etc),
    // strip the B and set explicit_bytes = true.
    // But we need to be careful: "KiB", "MiB", "kB", "MB" etc. already include the B
    // as part of their suffix and should not be treated as explicit_bytes.
    let (numeric_part, explicit_bytes) = if input.len() > 1 && input.ends_with('B') {
        // Check if the B is part of a known multi-char suffix
        let upper = input.to_uppercase();
        let known_suffixes = [
            "KIB", "MIB", "GIB", "TIB", "PIB", "EIB", "KB", "MB", "GB", "TB", "PB", "EB",
        ];
        let is_known = known_suffixes.iter().any(|s| upper.ends_with(s));
        if is_known {
            (&input[..], false)
        } else {
            // Just a trailing 'B' — GNU byte-count mode
            (&input[..input.len() - 1], true)
        }
    } else {
        (input, false)
    };

    // Handle hex: 0x...
    let (base_num_str, multiplier_str) = if let Some(rest) = numeric_part.strip_prefix("0x") {
        // Hex number — parse the hex part, then look for multiplier suffix
        split_number_suffix(rest, true)?
    } else if let Some(rest) = numeric_part.strip_prefix("0X") {
        split_number_suffix(rest, true)?
    } else {
        split_number_suffix(numeric_part, false)?
    };

    // Parse the base number
    let base: u64 = if numeric_part.starts_with("0x") || numeric_part.starts_with("0X") {
        u64::from_str_radix(base_num_str, 16).map_err(|_| Error::InvalidSize {
            input: input.to_string(),
            reason: format!("invalid hex number: {}", base_num_str),
        })?
    } else {
        base_num_str
            .parse::<u64>()
            .map_err(|_| Error::InvalidSize {
                input: input.to_string(),
                reason: format!("invalid number: {}", base_num_str),
            })?
    };

    let multiplier = parse_multiplier(multiplier_str)?;
    let bytes = base
        .checked_mul(multiplier)
        .ok_or_else(|| Error::InvalidSize {
            input: input.to_string(),
            reason: "size overflow".into(),
        })?;

    Ok(Size {
        bytes,
        explicit_bytes,
    })
}

/// Split a numeric string into (number_part, suffix_part).
/// e.g., "4KiB" → ("4", "KiB"), "10xM" → ("10", "xM")
fn split_number_suffix<'a>(s: &'a str, is_hex: bool) -> Result<(&'a str, &'a str)> {
    if s.is_empty() {
        return Ok(("0", ""));
    }

    // Find where the numeric part ends
    let num_end = if is_hex {
        // Hex: digits and a-f, A-F
        s.find(|c: char| !c.is_ascii_hexdigit())
            .unwrap_or(s.len())
    } else {
        // Decimal: digits only
        s.find(|c: char| !c.is_ascii_digit())
            .unwrap_or(s.len())
    };

    let num = &s[..num_end];
    let suffix = &s[num_end..];

    if num.is_empty() {
        return Err(Error::InvalidSize {
            input: s.to_string(),
            reason: "no number found".into(),
        });
    }

    Ok((num, suffix))
}

/// Parse a multiplier suffix like "K", "M", "KiB", "MB", "xM", "c", "w", "b", etc.
fn parse_multiplier(suffix: &str) -> Result<u64> {
    if suffix.is_empty() {
        return Ok(1);
    }

    match suffix {
        "c" => Ok(1),
        "w" => Ok(2),
        "b" => Ok(512),

        // Powers of 1024 (binary)
        "K" | "KiB" => Ok(1024),
        "M" | "MiB" => Ok(1024 * 1024),
        "G" | "GiB" => Ok(1024 * 1024 * 1024),
        "T" | "TiB" => Ok(1024u64.pow(4)),
        "P" | "PiB" => Ok(1024u64.pow(5)),
        "E" | "EiB" => Ok(1024u64.pow(6)),

        // Powers of 1000 (decimal/SI)
        "kB" | "KB" => Ok(1000),
        "MB" => Ok(1_000_000),
        "GB" => Ok(1_000_000_000),
        "TB" => Ok(1_000_000_000_000),
        "PB" => Ok(1_000_000_000_000_000),
        "EB" => Ok(1_000_000_000_000_000_000),

        // xM syntax: "xM" means "times 1048576"
        other => {
            if let Some(rest) = other.strip_prefix('x') {
                // "xK", "xM", "xG", etc.
                parse_multiplier(rest)
            } else {
                Err(Error::InvalidSize {
                    input: suffix.to_string(),
                    reason: format!("unknown suffix: {}", suffix),
                })
            }
        }
    }
}

/// Parse a size that must not be zero (used for bs/ibs/obs).
pub fn parse_positive_size(input: &str) -> Result<Size> {
    let size = parse_size(input)?;
    if size.bytes == 0 {
        return Err(Error::InvalidSize {
            input: input.to_string(),
            reason: "block size must be positive".into(),
        });
    }
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_numbers() {
        assert_eq!(parse_size("512").unwrap().bytes, 512);
        assert_eq!(parse_size("0").unwrap().bytes, 0);
        assert_eq!(parse_size("1024").unwrap().bytes, 1024);
    }

    #[test]
    fn test_kilo() {
        assert_eq!(parse_size("4K").unwrap().bytes, 4096);
        assert_eq!(parse_size("4KiB").unwrap().bytes, 4096);
        assert_eq!(parse_size("4kB").unwrap().bytes, 4000);
        assert_eq!(parse_size("4KB").unwrap().bytes, 4000);
    }

    #[test]
    fn test_mega() {
        assert_eq!(parse_size("1M").unwrap().bytes, 1048576);
        assert_eq!(parse_size("1MiB").unwrap().bytes, 1048576);
        assert_eq!(parse_size("1MB").unwrap().bytes, 1_000_000);
    }

    #[test]
    fn test_single_char_suffixes() {
        assert_eq!(parse_size("10c").unwrap().bytes, 10);
        assert_eq!(parse_size("10w").unwrap().bytes, 20);
        assert_eq!(parse_size("10b").unwrap().bytes, 5120);
    }

    #[test]
    fn test_x_syntax() {
        assert_eq!(parse_size("4xM").unwrap().bytes, 4 * 1048576);
        assert_eq!(parse_size("2xK").unwrap().bytes, 2048);
    }

    #[test]
    fn test_gnu_byte_count() {
        let s = parse_size("512B").unwrap();
        assert_eq!(s.bytes, 512);
        assert!(s.explicit_bytes);
    }

    #[test]
    fn test_hex() {
        assert_eq!(parse_size("0x1000").unwrap().bytes, 4096);
        assert_eq!(parse_size("0xFF").unwrap().bytes, 255);
    }

    #[test]
    fn test_overflow() {
        assert!(parse_size("99999999999999999999E").is_err());
    }
}
