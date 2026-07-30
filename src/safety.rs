/// Disk safety — prevents accidentally overwriting block devices, mounted
/// partitions, or system-critical files.
///
/// GNU dd's biggest design flaw is that it will silently destroy any file or
/// device you point it at. dd-rs adds multiple layers of protection:
///
/// ## Safety checks (in order)
///
/// 1. **Path inspection** — Is the target a block device? A symlink to one?
/// 2. **Mount check** — Is the device currently mounted? Writing to a mounted
///    partition will corrupt the filesystem.
/// 3. **Size check** — Is the input larger than the target device?
/// 4. **System-critical paths** — e.g., `/dev/sda` on Linux, `/dev/disk0` on macOS
/// 5. **Confirmation prompt** — Interactive warning unless `--force` is passed
///
/// ## Bypassing safety checks
///
/// ```bash
/// dd-rs --yes         # Skip confirmation prompts (non-interactive use)
/// dd-rs --force       # Skip ALL safety checks (DANGEROUS, like dd)
/// dd-rs --dry-run     # Validate everything except the actual write
/// ```

use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

use crate::conv::ConvOp;
use crate::error::{Error, Result};

// =============================================================================
// Risk scoring system
// =============================================================================

/// Risk level for an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    /// Safe operation — write to a regular file, no special concerns.
    Safe = 0,
    /// Minor concerns — unusual flags, small block sizes, or unknown sizes.
    Caution = 25,
    /// Dangerous — could cause data loss if you're not careful.
    Dangerous = 50,
    /// Catastrophic — will destroy your system if executed.
    Catastrophic = 75,
}

/// Full risk assessment of a command.
#[derive(Debug, Clone)]
pub struct RiskAssessment {
    /// Overall risk level.
    pub level: RiskLevel,
    /// Numeric score from 0 (completely safe) to 100 (certain doom).
    pub score: u32,
    /// Specific warnings about this operation.
    pub warnings: Vec<String>,
    /// Suggestions to make the operation safer.
    pub mitigations: Vec<String>,
    /// Individual risk factors that contributed to the score.
    pub factors: Vec<RiskFactor>,
}

/// A single risk factor contributing to the overall score.
#[derive(Debug, Clone)]
pub struct RiskFactor {
    pub name: String,
    pub score: u32,
    pub description: String,
}

/// Assess the risk of a dd-rs operation WITHOUT accessing the filesystem.
/// This is used by the --explain mode which doesn't want side effects.
pub fn assess_risk(
    output_path: Option<&Path>,
    input_path: Option<&Path>,
    count: Option<u64>,
    ibs: u64,
    conversions: &[ConvOp],
) -> RiskAssessment {
    let mut score: u32 = 0;
    let mut factors: Vec<RiskFactor> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut mitigations: Vec<String> = Vec::new();

    // ---- Factor 1: Output path analysis ----
    if let Some(path) = output_path {
        let path_str = path.to_string_lossy();
        let _path_lower = path_str.to_lowercase();

        // Raw disk devices (no partition number)
        if is_raw_disk_device(path) {
            let factor = RiskFactor {
                name: "Raw disk device".into(),
                score: 60,
                description: format!("'{}' is a raw disk device (no partition number). Writing to it will destroy the partition table, bootloader, and ALL partitions.", path_str),
            };
            score += factor.score;
            warnings.push(factor.description.clone());
            mitigations.push("Use a partition instead (e.g., /dev/sda1 not /dev/sda)".into());
            mitigations.push("If you're sure, use --force to override".into());
            factors.push(factor);
        }
        // Block device with partition
        else if is_block_device_path(path) {
            let factor = RiskFactor {
                name: "Block device partition".into(),
                score: 40,
                description: format!("'{}' is a block device partition. Writing will destroy all data on that partition.", path_str),
            };
            score += factor.score;
            warnings.push(factor.description.clone());
            mitigations.push("Verify this is the correct partition (check with lsblk or diskutil list)".into());
            mitigations.push("Make sure the partition is NOT mounted".into());
            factors.push(factor);
        }
        // Symlink to a device
        else if is_disk_by_path(path) {
            let factor = RiskFactor {
                name: "Device symlink".into(),
                score: 30,
                description: format!("'{}' appears to be a symlink to a block device (e.g., /dev/disk/by-id/...). Verify the target.", path_str),
            };
            score += factor.score;
            warnings.push(factor.description.clone());
            factors.push(factor);
        }
        // Writing to /dev/ directory
        else if path_str.starts_with("/dev/") {
            if !is_safe_dev_output(path) {
                let factor = RiskFactor {
                    name: "Device file target".into(),
                    score: 20,
                    description: format!("'{}' is in /dev. Device files have special behaviour — make sure this is what you intend.", path_str),
                };
                score += factor.score;
                warnings.push(factor.description.clone());
                factors.push(factor);
            }
        }
        // Writing to system directories
        if is_system_path(path) {
            let factor = RiskFactor {
                name: "System path".into(),
                score: 15,
                description: format!("'{}' is in a system directory. Overwriting system files can break your OS.", path_str),
            };
            score += factor.score;
            warnings.push(factor.description.clone());
            mitigations.push("Consider writing to /tmp or ~/ instead for testing".into());
            factors.push(factor);
        }
        // Writing to the same file as reading?
        if let Some(input) = input_path {
            if input == path {
                let factor = RiskFactor {
                    name: "Same file I/O".into(),
                    score: 40,
                    description: "Input and output are the SAME file. This will likely corrupt the data (read and write positions interfere).".into(),
                };
                score += factor.score;
                warnings.push(factor.description.clone());
                mitigations.push("Use a temporary file and rename it when done".into());
                factors.push(factor);
            }
        }
    }

    // ---- Factor 2: Unbounded operation ----
    if count.is_none() && output_path.is_some() {
        let factor = RiskFactor {
            name: "Unbounded write".into(),
            score: 20,
            description: "No count specified — will write until input is exhausted. If input is a device or pipe, this could be very large.".into(),
        };
        score += factor.score;
        warnings.push(factor.description.clone());
        mitigations.push("Add count=N to limit how much data is written".into());
        factors.push(factor);
    }

    // ---- Factor 3: Input is a device ----
    if let Some(path) = input_path {
        let path_str = path.to_string_lossy();
        if path_str.starts_with("/dev/") && path_str != "/dev/null" && path_str != "/dev/zero"
            && path_str != "/dev/urandom" && path_str != "/dev/random"
        {
            let factor = RiskFactor {
                name: "Device input".into(),
                score: 10,
                description: format!("Reading from '{}' — device inputs can be slow, infinite, or have side effects.", path_str),
            };
            score += factor.score;
            warnings.push(factor.description.clone());
            factors.push(factor);
        }
    }

    // ---- Factor 4: Dangerous conversions ----
    for conv in conversions {
        match conv {
            ConvOp::Notrunc => {
                let factor = RiskFactor {
                    name: "conv=notrunc".into(),
                    score: 15,
                    description: "conv=notrunc prevents truncation. Combined with seek, old data AFTER the written region is preserved — this may or may not be what you want.".into(),
                };
                score += factor.score;
                factors.push(factor);
            }
            ConvOp::Noerror => {
                let factor = RiskFactor {
                    name: "conv=noerror".into(),
                    score: 10,
                    description: "conv=noerror will SKIP corrupted blocks silently. The output may have gaps of missing data without warning.".into(),
                };
                score += factor.score;
                warnings.push(factor.description.clone());
                factors.push(factor);
            }
            _ => {}
        }
    }

    // ---- Factor 5: Suspiciously small block size ----
    if ibs < 512 {
        let factor = RiskFactor {
            name: "Tiny block size".into(),
            score: 5,
            description: format!("Block size is only {} bytes. This will be extremely slow.", ibs),
        };
        score += factor.score;
        mitigations.push("Use --auto-tune or set bs=128K for reasonable performance".into());
        factors.push(factor);
    }

    // ---- Factor 6: Writing to stdout is a pipe to something dangerous? ----
    if output_path.is_none() {
        let factor = RiskFactor {
            name: "Stdout redirect".into(),
            score: 5,
            description: "Writing to stdout — if redirected (e.g., > /dev/sda), the shell bypasses dd-rs's safety checks. Consider using of= instead.".into(),
        };
        score += factor.score;
        factors.push(factor);
    }

    // ---- Factor 7: LVM / md RAID / ZFS zvols / device-mapper ----
    if let Some(path) = output_path {
        let path_str = path.to_string_lossy();
        if path_str.starts_with("/dev/mapper/") || path_str.starts_with("/dev/dm-") {
            let factor = RiskFactor {
                name: "LVM/device-mapper".into(),
                score: 35,
                description: format!("'{}' is an LVM logical volume or device-mapper target. Writing to it affects the mapped storage — verify this is the correct LV.", path_str),
            };
            score += factor.score;
            warnings.push(factor.description.clone());
            mitigations.push("Run 'lvdisplay' or 'dmsetup ls' to verify the mapping".into());
            factors.push(factor);
        }
        if path_str.starts_with("/dev/md") {
            let factor = RiskFactor {
                name: "md RAID device".into(),
                score: 35,
                description: format!("'{}' is an md RAID device. Writing to it affects ALL member disks.", path_str),
            };
            score += factor.score;
            warnings.push(factor.description.clone());
            factors.push(factor);
        }
        if path_str.starts_with("/dev/zvol/") {
            let factor = RiskFactor {
                name: "ZFS zvol".into(),
                score: 30,
                description: format!("'{}' is a ZFS zvol — a block device backed by a ZFS pool.", path_str),
            };
            score += factor.score;
            factors.push(factor);
        }
        if path_str.starts_with("/dev/loop") {
            let factor = RiskFactor {
                name: "Loop device".into(),
                score: 15,
                description: format!("'{}' is a loop device. It may be backed by a file — check with 'losetup -l'.", path_str),
            };
            score += factor.score;
            factors.push(factor);
        }
        if path_str.starts_with("/dev/ram") {
            let factor = RiskFactor {
                name: "RAM disk".into(),
                score: 5,
                description: format!("'{}' is a RAM disk. Data will be lost on reboot.", path_str),
            };
            score += factor.score;
            factors.push(factor);
        }
    }

    // ---- Factor 8: Running as root amplifies all risks ----
    #[cfg(unix)]
    {
        if unsafe { libc::geteuid() == 0 } {
            let factor = RiskFactor {
                name: "Running as root".into(),
                score: 15,
                description: "You are running as ROOT. All file permissions are bypassed — a typo in of= could destroy any file or device.".into(),
            };
            score += factor.score;
            warnings.push(factor.description.clone());
            mitigations.push("Consider running as a regular user with sudo only when needed".into());
            factors.push(factor);
        }
    }

    // ---- Factor 9: conv=fsync/fdatasync on large transfers ----
    for conv in conversions {
        match conv {
            ConvOp::Fsync => {
                let factor = RiskFactor {
                    name: "conv=fsync".into(),
                    score: 5,
                    description: "conv=fsync forces a full sync (data + metadata) after EVERY block. This can make transfers 10-100× slower on mechanical drives.".into(),
                };
                score += factor.score;
                factors.push(factor);
            }
            ConvOp::Fdatasync => {
                let factor = RiskFactor {
                    name: "conv=fdatasync".into(),
                    score: 3,
                    description: "conv=fdatasync syncs data after every block. Significantly slower than normal writes.".into(),
                };
                score += factor.score;
                factors.push(factor);
            }
            _ => {}
        }
    }

    // ---- Factor 10: Partial overwrite (seek without truncation) ----
    if let Some(path) = output_path {
        if conversions.contains(&ConvOp::Notrunc) {
            let factor = RiskFactor {
                name: "Partial overwrite".into(),
                score: 10,
                description: format!("conv=notrunc with output '{}' — only part of the file/device will be overwritten. Old data beyond the written region is preserved. This may produce a corrupt hybrid of old and new data.", path.to_string_lossy()),
            };
            score += factor.score;
            warnings.push(factor.description.clone());
            mitigations.push("If you want a clean write, remove conv=notrunc to truncate first".into());
            factors.push(factor);
        }
    }

    // ---- Factor 11: FAT32 4GB file size limit ----
    if let Some(path) = output_path {
        if let Some(cnt) = count {
            let total = cnt * ibs;
            if total > 4_294_967_295 {
                // 4GB - 1 byte (FAT32 max file size)
                let path_str = path.to_string_lossy();
                let factor = RiskFactor {
                    name: "4GB+ file".into(),
                    score: 10,
                    description: format!("Output size ({}) exceeds 4 GB. If '{}' is on a FAT32 filesystem, the write will fail at 4 GB (FAT32 max file size). Use exFAT or NTFS for large files.", format_size(total), path_str),
                };
                score += factor.score;
                warnings.push(factor.description.clone());
                factors.push(factor);
            }
        }
    }

    // ---- Factor 12: Input and output might be on the same physical device ----
    if let (Some(in_path), Some(out_path)) = (input_path, output_path) {
        if in_path != out_path {
            let _in_str = in_path.to_string_lossy();
            let _out_str = out_path.to_string_lossy();
            // Heuristic: same /dev/disk/by-* or same /dev/mapper/ prefix suggests same physical device
            let in_parent = in_path.parent().map(|p| p.to_string_lossy().to_string());
            let out_parent = out_path.parent().map(|p| p.to_string_lossy().to_string());
            if in_parent == out_parent && in_parent.as_deref() == Some("/dev/disk/by-id") {
                let factor = RiskFactor {
                    name: "Same physical device?".into(),
                    score: 10,
                    description: "Input and output may be on the same physical device. Reading and writing simultaneously can cause severe disk thrashing and slow performance.".into(),
                };
                score += factor.score;
                factors.push(factor);
            }
        }
    }

    // ---- Factor 13: Writing to a path that resolves to the root filesystem ----
    if let Some(path) = output_path {
        let path_str = path.to_string_lossy();
        if path_str == "/" || path_str == "/root" {
            let factor = RiskFactor {
                name: "Root filesystem target".into(),
                score: 50,
                description: "Output path is the ROOT FILESYSTEM. Writing here could overwrite critical system files.".into(),
            };
            score += factor.score;
            warnings.push(factor.description.clone());
            factors.push(factor);
        }
    }

    // Clamp score
    let score = score.min(100);

    // Determine level
    let level = if score >= 75 {
        RiskLevel::Catastrophic
    } else if score >= 50 {
        RiskLevel::Dangerous
    } else if score >= 25 {
        RiskLevel::Caution
    } else {
        RiskLevel::Safe
    };

    RiskAssessment {
        level,
        score,
        warnings,
        mitigations,
        factors,
    }
}

// =============================================================================
// Sophisticated path blacklist
// =============================================================================

/// Check if a path is a raw disk device (entire disk, not a partition).
/// These are the most dangerous targets.
pub fn is_raw_disk_device(path: &Path) -> bool {
    let path_str = path.to_string_lossy();

    #[cfg(target_os = "linux")]
    {
        // /dev/sda, /dev/sdb (no trailing digit)
        if path_str.len() >= 8 {
            let name = &path_str[5..]; // strip "/dev/"
            // sda, sdb, ..., sdz (no partition number)
            if name.len() == 3 && name.starts_with("sd") && name.as_bytes()[2].is_ascii_alphabetic() {
                return true;
            }
            // hda, hdb (old IDE)
            if name.len() == 3 && name.starts_with("hd") && name.as_bytes()[2].is_ascii_alphabetic() {
                return true;
            }
            // vda, xvda (virtualized)
            if (name.starts_with("vd") || name.starts_with("xvd")) && name.len() >= 3 {
                let rest = &name[2..];
                if rest.chars().all(|c| c.is_ascii_alphabetic()) {
                    return true;
                }
            }
            // nvme0n1 (NVMe namespace, not partition like nvme0n1p1)
            if name.starts_with("nvme") {
                // nvme0n1 is raw, nvme0n1p1 is a partition
                if !name.contains('p') || name.ends_with("n1") && name.matches('p').count() == 0 {
                    // Heuristic: if it ends with n<digit> and has no 'p', it's a namespace
                    let has_partition = name.contains('p');
                    if !has_partition {
                        return true;
                    }
                }
            }
            // mmcblk0 (not mmcblk0p1)
            if name.starts_with("mmcblk") && !name.contains('p') {
                return true;
            }
            // loop devices
            if name.starts_with("loop") && name[4..].chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // /dev/disk0, /dev/disk1 (not /dev/disk0s1)
        if path_str.starts_with("/dev/disk") || path_str.starts_with("/dev/rdisk") {
            let rest = if path_str.starts_with("/dev/rdisk") {
                &path_str[10..]
            } else {
                &path_str[9..]
            };
            // disk0 = raw, disk0s1 = partition
            if !rest.contains('s') {
                return true;
            }
        }
    }

    false
}

/// Check if a path looks like a block device partition.
pub fn is_block_device_path(path: &Path) -> bool {
    let path_str = path.to_string_lossy();

    if !path_str.starts_with("/dev/") {
        return false;
    }

    #[cfg(target_os = "linux")]
    {
        let name = &path_str[5..];
        // Partition patterns: sda1, nvme0n1p1, mmcblk0p1, vda1
        if name.len() > 3 && name.starts_with("sd") && name[3..].chars().any(|c| c.is_ascii_digit()) {
            return true;
        }
        if name.contains("nvme") && name.contains('p') {
            return true;
        }
        if name.contains("mmcblk") && name.contains('p') {
            return true;
        }
        if (name.starts_with("vd") || name.starts_with("xvd")) && name[2..].chars().any(|c| c.is_ascii_digit()) {
            return true;
        }
        if name.starts_with("hd") && name[3..].chars().any(|c| c.is_ascii_digit()) {
            return true;
        }
    }

    #[cfg(target_os = "macos")]
    {
        // /dev/disk0s1, /dev/rdisk0s2
        if (path_str.starts_with("/dev/disk") || path_str.starts_with("/dev/rdisk")) && path_str.contains('s') {
            return true;
        }
    }

    false
}

/// Check if a path looks like a /dev/disk/by-id/ or /dev/disk/by-uuid/ symlink.
fn is_disk_by_path(path: &Path) -> bool {
    let path_str = path.to_string_lossy();

    #[cfg(target_os = "linux")]
    {
        path_str.starts_with("/dev/disk/by-id/")
            || path_str.starts_with("/dev/disk/by-uuid/")
            || path_str.starts_with("/dev/disk/by-path/")
            || path_str.starts_with("/dev/disk/by-label/")
            || path_str.starts_with("/dev/mapper/")
    }

    #[cfg(target_os = "macos")]
    {
        path_str.starts_with("/dev/disk/by-")
    }
}

/// Check if outputting to this /dev path is safe.
fn is_safe_dev_output(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    // These are explicitly safe to write to
    path_str == "/dev/null"
        || path_str == "/dev/zero"
        || path_str == "/dev/full"
        || path_str == "/dev/stdout"
        || path_str == "/dev/stderr"
}

/// Check if a path is in a system-critical directory.
fn is_system_path(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    let dangerous_dirs = [
        "/boot", "/boot/efi", "/efi",
        "/etc", "/lib", "/lib64", "/usr/lib", "/usr/lib64",
        "/sbin", "/bin", "/usr/sbin", "/usr/bin",
        "/System/Library", "/Library/System",
    ];

    dangerous_dirs.iter().any(|d| path_str.starts_with(d))
        || path_str == "/vmlinuz"
        || path_str == "/initrd.img"
        || path_str == "/initramfs"
        || path_str == "/mach_kernel"
}

// =============================================================================
// Safety level (unchanged)
// =============================================================================

/// Controls how aggressively dd-rs protects against dangerous operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyLevel {
    /// Full safety: block devices trigger confirmation, system paths are rejected.
    /// This is the default.
    Safe,

    /// Skip confirmation prompts for non-interactive use. Still checks and logs
    /// warnings, but won't block for input.
    NonInteractive,

    /// Skip ALL safety checks. Equivalent to GNU dd behaviour. Use at your own risk.
    ForceUnsafe,
}

impl SafetyLevel {
    pub fn from_args(yes: bool, force: bool) -> Self {
        if force {
            SafetyLevel::ForceUnsafe
        } else if yes {
            SafetyLevel::NonInteractive
        } else {
            SafetyLevel::Safe
        }
    }
}

// =============================================================================
// Device detection
// =============================================================================

/// Information about a block device target.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub path: PathBuf,
    pub device_name: String,
    pub size_bytes: u64,
    pub is_mounted: bool,
    pub mount_points: Vec<PathBuf>,
    pub device_type: DeviceType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceType {
    /// A regular file — no special warnings needed.
    RegularFile,
    /// A block device (e.g., /dev/sda, /dev/disk0).
    BlockDevice,
    /// A character device (e.g., /dev/null, /dev/zero, /dev/tty).
    CharDevice,
    /// Unknown — path doesn't exist or can't be stat'd.
    Unknown,
}

/// Check if a path refers to a block device and gather information about it.
/// Handles edge cases: empty paths, symlinks, non-existent files, permission errors.
pub fn inspect_output_target(path: &Path) -> Result<DeviceInfo> {
    // Edge case: empty path
    if path.as_os_str().is_empty() {
        return Err(Error::InvalidArgument("output path is empty".into()));
    }

    // Try to canonicalize (resolve symlinks) — but don't fail if we can't.
    // Canonicalize fails if the file doesn't exist, which is fine for new files.
    let resolved_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    let metadata = match fs::symlink_metadata(&resolved_path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(DeviceInfo {
                path: path.to_path_buf(),
                device_name: path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string()),
                size_bytes: 0,
                is_mounted: false,
                mount_points: vec![],
                device_type: DeviceType::Unknown,
            });
        }
        Err(e) => return Err(Error::Io(e)),
    };

    // Use the canonicalized path's metadata to detect device type
    let device_type = if metadata.file_type().is_block_device() {
        DeviceType::BlockDevice
    } else if metadata.file_type().is_char_device() {
        DeviceType::CharDevice
    } else if metadata.file_type().is_file() {
        DeviceType::RegularFile
    } else {
        DeviceType::Unknown
    };

    let size_bytes = metadata.len();
    let device_name = resolved_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| resolved_path.to_string_lossy().to_string());

    // Check if it's mounted (platform-specific) — use the resolved path
    let (is_mounted, mount_points) = check_if_mounted(&resolved_path);

    // If the original path was a symlink, note it in the device name
    let device_name = if path.is_symlink() {
        format!("{} → {}", path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string()), device_name)
    } else {
        device_name
    };

    Ok(DeviceInfo {
        path: path.to_path_buf(),
        device_name,
        size_bytes,
        is_mounted,
        mount_points,
        device_type,
    })
}

/// Check if a block device is currently mounted by consulting the OS mount table.
fn check_if_mounted(path: &Path) -> (bool, Vec<PathBuf>) {
    #[cfg(target_os = "linux")]
    {
        // Read /proc/mounts to find mount points
        if let Ok(contents) = fs::read_to_string("/proc/mounts") {
            let path_str = path.to_string_lossy();
            let mounts: Vec<PathBuf> = contents
                .lines()
                .filter(|line| line.starts_with(path_str.as_ref()))
                .filter_map(|line| line.split_whitespace().nth(1))
                .map(PathBuf::from)
                .collect();
            return (!mounts.is_empty(), mounts);
        }
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS, we can use libc::statfs or just check with `mount` command
        let path_str = path.to_string_lossy();
        // Simple heuristic: check if any component of the path is a disk device
        if path_str.starts_with("/dev/disk") || path_str.starts_with("/dev/rdisk") {
            // These are raw disk devices — they might be in use
            // We can't easily tell if mounted, so err on the side of caution
            return (true, vec![PathBuf::from("(unknown — raw disk device)")]);
        }
    }

    // Fallback: can't determine mount status
    (false, vec![])
}

/// Check if a path is a system-critical device.
/// These are paths that, if overwritten, could brick the system.
pub fn is_system_critical(path: &Path) -> bool {
    let path_str = path.to_string_lossy();

    #[cfg(target_os = "linux")]
    {
        let critical_prefixes = [
            "/dev/sda", "/dev/sdb", "/dev/sdc", "/dev/sdd",
            "/dev/nvme0", "/dev/nvme1",
            "/dev/hda", "/dev/hdb",
            "/dev/xvda", "/dev/vda",
            "/dev/mmcblk0",
        ];
        if critical_prefixes.iter().any(|p| path_str.starts_with(p)) {
            return true;
        }
    }

    #[cfg(target_os = "macos")]
    {
        let critical_prefixes = [
            "/dev/disk0", "/dev/disk1",
            "/dev/rdisk0", "/dev/rdisk1",
        ];
        if critical_prefixes.iter().any(|p| path_str.starts_with(p)) {
            return true;
        }
    }

    false
}

// =============================================================================
// Safety checks & user interaction
// =============================================================================

/// Result of a safety check.
#[derive(Debug)]
pub enum SafetyDecision {
    /// Proceed safely.
    Safe,
    /// Warning issued but proceeding (non-interactive mode).
    WarningIssued,
    /// Operation blocked — too dangerous.
    Blocked { reason: String },
    /// User confirmed via prompt.
    Confirmed,
}

/// Run all safety checks against an output target. Returns a decision.
pub fn check_output_safety(
    info: &DeviceInfo,
    level: SafetyLevel,
    input_size_hint: Option<u64>,
) -> Result<SafetyDecision> {
    if level == SafetyLevel::ForceUnsafe {
        return Ok(SafetyDecision::Safe);
    }

    let mut warnings: Vec<String> = Vec::new();

    // ---- Check 1: Block device ----
    if info.device_type == DeviceType::BlockDevice {
        warnings.push(format!(
            "⚠ DD-RS SAFETY WARNING ⚠\n\
             Target '{}' is a BLOCK DEVICE.\n\
             Writing to it will DESTROY ALL DATA on this device.\n\
             Device size: {} bytes ({:.1} GB)",
            info.path.display(),
            info.size_bytes,
            info.size_bytes as f64 / 1_000_000_000.0,
        ));
    }

    // ---- Check 2: Mounted device ----
    if info.is_mounted {
        warnings.push(format!(
            "CRITICAL: Target device is currently MOUNTED at:\n  {}",
            info
                .mount_points
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n  ")
        ));
    }

    // ---- Check 3: System-critical ----
    if is_system_critical(&info.path) {
        warnings.push(format!(
            "CRITICAL: '{}' appears to be a system disk.\n\
             Overwriting it will make your system UNBOOTABLE.",
            info.path.display()
        ));
    }

    // ---- Check 4: Named pipes / special files ----
    if info.device_type == DeviceType::CharDevice
        && !is_safe_char_device(&info.path)
    {
        warnings.push(format!(
            "WARNING: '{}' is a character device. Writing to it may have\n\
             unexpected effects (e.g., writing to a terminal will output garbage).",
            info.path.display()
        ));
    }

    // ---- Check 5: Size mismatch ----
    if let Some(input_size) = input_size_hint {
        if info.size_bytes > 0 && input_size > info.size_bytes {
            warnings.push(format!(
                "WARNING: Input size ({:.1} GB) exceeds output device size ({:.1} GB).\n\
                 The copy will be truncated.",
                input_size as f64 / 1_000_000_000.0,
                info.size_bytes as f64 / 1_000_000_000.0,
            ));
        }
    }

    // If there are warnings, decide what to do
    if warnings.is_empty() {
        return Ok(SafetyDecision::Safe);
    }

    // Print all warnings
    for warning in &warnings {
        eprintln!("{}", warning);
    }

    match level {
        SafetyLevel::NonInteractive => {
            eprintln!("\n⚠ Proceeding with warnings (non-interactive mode). Use --force to suppress.\n");
            Ok(SafetyDecision::WarningIssued)
        }
        SafetyLevel::Safe => {
            // Interactive confirmation
            eprintln!();
            eprint!(
                "This operation is DANGEROUS and may destroy data.\n\
                 Type 'YES' (uppercase) to proceed, anything else to abort: "
            );
            let _ = io::stderr().flush();

            let mut input = String::new();
            match io::stdin().read_line(&mut input) {
                Ok(_) => {
                    if input.trim() == "YES" {
                        eprintln!("\nProceeding with user confirmation.\n");
                        Ok(SafetyDecision::Confirmed)
                    } else {
                        Err(Error::Other(
                            "Operation cancelled by user (safety check failed). \
                             Use --yes for non-interactive mode or --force to skip checks."
                                .into(),
                        ))
                    }
                }
                Err(_) => {
                    // Can't read stdin (e.g., piped input)
                    Err(Error::Other(
                        "Cannot confirm safety — stdin is not a terminal.\n\
                         Use --yes for non-interactive mode or --force to skip checks."
                            .into(),
                    ))
                }
            }
        }
        SafetyLevel::ForceUnsafe => {
            unreachable!() // handled at the top
        }
    }
}

/// Check if a character device is "safe" to write to (like /dev/null).
fn is_safe_char_device(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    // Common safe character devices
    let safe_devices = [
        "/dev/null",
        "/dev/zero",
        "/dev/random",
        "/dev/urandom",
        "/dev/full",
    ];
    // Any path ending with these names (handles /dev/pts/ etc.)
    safe_devices.iter().any(|d| path_str.ends_with(d))
        || path_str == "/dev/stdout"
        || path_str == "/dev/stderr"
        || path_str == "/dev/fd/"
}

// =============================================================================
// Input size estimation
// =============================================================================

/// Format a byte size for human-readable display.
fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.2} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.2} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.2} kB", bytes as f64 / 1_000.0)
    } else {
        format!("{} bytes", bytes)
    }
}

/// Try to determine the size of the input (for safety checks).
pub fn estimate_input_size(path: Option<&Path>, count: Option<u64>, ibs: u64) -> Option<u64> {
    // If count is specified, we know exactly how much we'll read
    if let Some(c) = count {
        return Some(c * ibs);
    }

    // Otherwise, try to stat the input file
    if let Some(p) = path {
        if let Ok(meta) = fs::metadata(p) {
            if meta.is_file() {
                return Some(meta.len());
            }
        }
    }

    // Can't determine (stdin, pipe, device)
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_char_devices() {
        assert!(is_safe_char_device(Path::new("/dev/null")));
        assert!(is_safe_char_device(Path::new("/dev/zero")));
        assert!(is_safe_char_device(Path::new("/dev/urandom")));
        assert!(!is_safe_char_device(Path::new("/dev/tty")));
        assert!(!is_safe_char_device(Path::new("/dev/sda")));
    }

    #[test]
    fn test_system_critical_paths() {
        #[cfg(target_os = "linux")]
        {
            assert!(is_system_critical(Path::new("/dev/sda")));
            assert!(is_system_critical(Path::new("/dev/nvme0n1")));
            assert!(!is_system_critical(Path::new("/dev/null")));
        }
    }
}
