/// Conversion pipeline.
///
/// dd's `conv=` options form a pipeline:
///   input → [ascii/ebcdic/ibm] → [block/unblock] → [lcase/ucase] → [swab] → [sync] → output
///
/// The order matters (GNU dd processes conversions in a fixed order regardless of
/// the order specified on the command line).

pub mod block;
pub mod case;
pub mod ebcdic;
pub mod swab;

use crate::error::Result;

// =============================================================================
// Conversion enumeration
// =============================================================================

/// A single conversion in the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvOp {
    /// EBCDIC → ASCII (implies unblock)
    Ascii,
    /// ASCII → EBCDIC (implies block)
    Ebcdic,
    /// ASCII → alternate EBCDIC (IBM1047; implies block)
    Ibm,
    /// Pad newline-terminated records with spaces to cbs size
    Block,
    /// Replace trailing spaces in cbs-sized blocks with newlines
    Unblock,
    /// Map uppercase to lowercase
    Lcase,
    /// Map lowercase to uppercase
    Ucase,
    /// Swap every pair of bytes
    Swab,
    /// Pad input blocks with NULs (or spaces for block/unblock) to ibs size
    Sync,
    /// Seek rather than write NUL output blocks (sparse files)
    Sparse,
    /// Continue after read errors
    Noerror,
    /// Do not truncate output file
    Notrunc,
    /// Fail if output file exists
    Excl,
    /// Do not create output file
    Nocreat,
    /// fdatasync before exit
    Fdatasync,
    /// fsync before exit
    Fsync,
}

impl ConvOp {
    /// Parse a single conversion name (case-insensitive).
    pub fn parse(s: &str) -> Result<ConvOp> {
        match s.trim().to_lowercase().as_str() {
            "ascii" => Ok(ConvOp::Ascii),
            "ebcdic" => Ok(ConvOp::Ebcdic),
            "ibm" => Ok(ConvOp::Ibm),
            "block" => Ok(ConvOp::Block),
            "unblock" => Ok(ConvOp::Unblock),
            "lcase" => Ok(ConvOp::Lcase),
            "ucase" => Ok(ConvOp::Ucase),
            "swab" => Ok(ConvOp::Swab),
            "sync" => Ok(ConvOp::Sync),
            "sparse" => Ok(ConvOp::Sparse),
            "noerror" => Ok(ConvOp::Noerror),
            "notrunc" => Ok(ConvOp::Notrunc),
            "excl" => Ok(ConvOp::Excl),
            "nocreat" => Ok(ConvOp::Nocreat),
            "fdatasync" => Ok(ConvOp::Fdatasync),
            "fsync" => Ok(ConvOp::Fsync),
            other => Err(crate::error::Error::InvalidArgument(format!(
                "unknown conversion: {}",
                other
            ))),
        }
    }

    /// Parse a comma-separated list of conversions.
    pub fn parse_list(input: &str) -> Result<Vec<ConvOp>> {
        if input.is_empty() {
            return Ok(vec![]);
        }
        input.split(',').map(|s| ConvOp::parse(s.trim())).collect()
    }

    /// Returns true if this conversion operates on data (as opposed to file-level flags).
    pub fn is_data_conversion(&self) -> bool {
        matches!(
            self,
            ConvOp::Ascii
                | ConvOp::Ebcdic
                | ConvOp::Ibm
                | ConvOp::Block
                | ConvOp::Unblock
                | ConvOp::Lcase
                | ConvOp::Ucase
                | ConvOp::Swab
                | ConvOp::Sync
        )
    }
}

// =============================================================================
// Conversion context
// =============================================================================

/// State carried across the conversion pipeline.
pub struct ConvContext {
    /// Conversion buffer size (cbs=)
    pub cbs: usize,
    /// Input block size (ibs=)
    pub ibs: usize,
    /// Whether block/unblock uses space-padding (true) or NUL-padding (false)
    pub use_spaces: bool,
    /// Remaining bytes in the current unblock record (for partial writes)
    pub unblock_remaining: usize,
    /// Position within current unblock record
    pub unblock_pos: usize,
}

impl ConvContext {
    pub fn new(cbs: usize, ibs: usize) -> Self {
        Self {
            cbs,
            ibs,
            use_spaces: false,
            unblock_remaining: 0,
            unblock_pos: 0,
        }
    }
}

// =============================================================================
// Conversion pipeline
// =============================================================================

/// The ordered list of data conversions to apply.
#[derive(Debug, Default)]
pub struct ConversionPipeline {
    ops: Vec<ConvOp>,
}

impl ConversionPipeline {
    pub fn new(ops: Vec<ConvOp>) -> Self {
        // Sort into the canonical dd order
        let mut ordered = Vec::new();
        // 1. EBCDIC/ASCII/IBM conversions (at most one of these)
        if ops.contains(&ConvOp::Ascii) {
            ordered.push(ConvOp::Ascii);
        } else if ops.contains(&ConvOp::Ibm) {
            ordered.push(ConvOp::Ibm);
        } else if ops.contains(&ConvOp::Ebcdic) {
            ordered.push(ConvOp::Ebcdic);
        }
        // 2. Block/Unblock
        if ordered.contains(&ConvOp::Ebcdic) || ordered.contains(&ConvOp::Ibm) {
            // ebcdic/ibm implies block
            if !ops.contains(&ConvOp::Unblock) {
                ordered.push(ConvOp::Block);
            }
        }
        if ordered.contains(&ConvOp::Ascii) {
            // ascii implies unblock
            if !ops.contains(&ConvOp::Block) {
                ordered.push(ConvOp::Unblock);
            }
        }
        if ops.contains(&ConvOp::Block) && !ordered.contains(&ConvOp::Block) {
            ordered.push(ConvOp::Block);
        }
        if ops.contains(&ConvOp::Unblock) && !ordered.contains(&ConvOp::Unblock) {
            ordered.push(ConvOp::Unblock);
        }
        // 3. Case
        if ops.contains(&ConvOp::Lcase) && !ordered.contains(&ConvOp::Lcase) {
            ordered.push(ConvOp::Lcase);
        }
        if ops.contains(&ConvOp::Ucase) && !ordered.contains(&ConvOp::Ucase) {
            ordered.push(ConvOp::Ucase);
        }
        // 4. Swab
        if ops.contains(&ConvOp::Swab) {
            ordered.push(ConvOp::Swab);
        }
        // 5. Sync (always last data conv)
        if ops.contains(&ConvOp::Sync) {
            ordered.push(ConvOp::Sync);
        }
        // Non-data conversions
        for op in &[
            ConvOp::Sparse,
            ConvOp::Noerror,
            ConvOp::Notrunc,
            ConvOp::Excl,
            ConvOp::Nocreat,
            ConvOp::Fdatasync,
            ConvOp::Fsync,
        ] {
            if ops.contains(op) {
                ordered.push(*op);
            }
        }

        Self { ops: ordered }
    }

    /// Apply all data conversions to a buffer in pipeline order.
    pub fn apply(&self, buf: &mut [u8], ctx: &mut ConvContext) -> Result<usize> {
        let mut len = buf.len();
        for op in &self.ops {
            if !op.is_data_conversion() {
                continue;
            }
            len = match op {
                ConvOp::Ascii => ebcdic::conv_ebcdic_to_ascii(&mut buf[..len]),
                ConvOp::Ebcdic => ebcdic::conv_ascii_to_ebcdic(&mut buf[..len]),
                ConvOp::Ibm => ebcdic::conv_ibm_ebcdic_to_ascii(&mut buf[..len]),
                ConvOp::Lcase => Ok(case::lcase(&mut buf[..len])),
                ConvOp::Ucase => Ok(case::ucase(&mut buf[..len])),
                ConvOp::Swab => Ok(swab::swab_bytes(&mut buf[..len])),
                ConvOp::Block => block::block_record(&mut buf[..len], ctx),
                ConvOp::Unblock => block::unblock_record(&mut buf[..len], ctx),
                ConvOp::Sync => {
                    // sync: pad with NULs (or spaces for block/unblock) to ibs
                    sync_buffer(buf, len, ctx)
                }
                _ => Ok(len),
            }?;
        }
        Ok(len)
    }

    pub fn has_sparse(&self) -> bool {
        self.ops.contains(&ConvOp::Sparse)
    }

    pub fn has_noerror(&self) -> bool {
        self.ops.contains(&ConvOp::Noerror)
    }

    pub fn has_any_data_conv(&self) -> bool {
        self.ops.iter().any(|op| op.is_data_conversion())
    }

    /// Access the ordered list of conversion operations.
    pub fn ops(&self) -> &[ConvOp] {
        &self.ops
    }
}

/// Pad buffer to ibs with NULs (or spaces for block/unblock mode).
fn sync_buffer(buf: &mut [u8], actual_len: usize, ctx: &ConvContext) -> Result<usize> {
    let pad_byte = if ctx.use_spaces { b' ' } else { 0u8 };
    let target = ctx.ibs;
    if actual_len < target {
        for byte in buf.iter_mut().skip(actual_len).take(target - actual_len) {
            *byte = pad_byte;
        }
        Ok(target)
    } else {
        Ok(actual_len.max(target))
    }
}
