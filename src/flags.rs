/// I/O flags (iflag= / oflag=) handling and file opening.
///
/// These flags control how input and output files are opened, and influence
/// the behaviour of read/write system calls (O_DIRECT, O_SYNC, O_NONBLOCK, etc.).
///
/// On Linux, dd-rs uses `libc` for the raw `open(2)` flags. On macOS some
/// flags (like O_DIRECT) are not available; those are compiled out with a
/// warning.

// ---------------------------------------------------------------------------
// Platform support for I/O flags:
//
//   Flag        Linux   macOS   Notes
//   ─────────   ─────   ─────   ─────────────────────────────────────────
//   append      ✅      ✅      O_APPEND (POSIX)
//   direct      ✅      ❌      O_DIRECT (Linux-only, silently ignored on macOS)
//   directory   ✅      ✅      Path check (no open flag needed)
//   dsync       ✅      ❌      O_DSYNC (Linux-only)
//   sync        ✅      ❌      O_SYNC (Linux-only)
//   nonblock    ✅      ✅      O_NONBLOCK (POSIX)
//   noatime     ✅      ❌      O_NOATIME (Linux-only)
//   nocache     ✅      ❌      posix_fadvise (Linux-only, best-effort)
//   noctty      ✅      ✅      O_NOCTTY (POSIX)
//   nofollow    ✅      ✅      O_NOFOLLOW (POSIX)
//   fullblock   ✅      ✅      Userspace accumulation (no syscall needed)
// ---------------------------------------------------------------------------

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::FromRawFd;
use std::path::Path;

use crate::error::{Error, Result};

// =============================================================================
// Flag definitions
// =============================================================================

/// I/O flag: controls how a file descriptor is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoFlag {
    Append,    // O_APPEND
    Direct,    // O_DIRECT (Linux only)
    Directory, // Fail if not a directory
    Dsync,     // O_DSYNC (synchronized data)
    Sync,      // O_SYNC (synchronized data + metadata)
    Nonblock,  // O_NONBLOCK
    Noatime,   // O_NOATIME (Linux only)
    Nocache,   // posix_fadvise(..., POSIX_FADV_DONTNEED) — best-effort
    Noctty,    // O_NOCTTY
    Nofollow,  // O_NOFOLLOW

    // Extended: dd-rs-only flags
    Fullblock, // Accumulate full blocks on short reads (critical for pipes)
}

impl IoFlag {
    /// Parse a flag string (case-insensitive).
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "append" => Ok(Self::Append),
            "direct" => Ok(Self::Direct),
            "directory" => Ok(Self::Directory),
            "dsync" => Ok(Self::Dsync),
            "sync" => Ok(Self::Sync),
            "nonblock" => Ok(Self::Nonblock),
            "noatime" => Ok(Self::Noatime),
            "nocache" => Ok(Self::Nocache),
            "noctty" => Ok(Self::Noctty),
            "nofollow" => Ok(Self::Nofollow),
            "fullblock" => Ok(Self::Fullblock),
            other => Err(Error::InvalidFlag {
                flag: "iflag/oflag".into(),
                value: other.into(),
            }),
        }
    }

    /// Apply this flag to an `OpenOptions` builder.
    fn apply_to_open_options(&self, opts: &mut OpenOptions) {
        let mut custom_flags: i32 = 0;
        match self {
            IoFlag::Append => {
                opts.append(true);
            }
            IoFlag::Nonblock => {
                custom_flags |= libc::O_NONBLOCK;
            }
            IoFlag::Noctty => {
                custom_flags |= libc::O_NOCTTY;
            }
            IoFlag::Nofollow => {
                custom_flags |= libc::O_NOFOLLOW;
            }
            #[cfg(target_os = "linux")]
            IoFlag::Direct => {
                custom_flags |= libc::O_DIRECT;
            }
            #[cfg(target_os = "linux")]
            IoFlag::Noatime => {
                custom_flags |= libc::O_NOATIME;
            }
            #[cfg(target_os = "linux")]
            IoFlag::Dsync => {
                custom_flags |= libc::O_DSYNC;
            }
            #[cfg(target_os = "linux")]
            IoFlag::Sync => {
                custom_flags |= libc::O_SYNC;
            }
            #[cfg(not(target_os = "linux"))]
            IoFlag::Direct | IoFlag::Noatime | IoFlag::Dsync | IoFlag::Sync => {
                log::warn!("Flag {:?} is not supported on this platform; ignoring", self);
            }
            // Directory, Nocache, Fullblock are handled at a higher level
            IoFlag::Directory | IoFlag::Nocache | IoFlag::Fullblock => {}
        }
        opts.custom_flags(custom_flags);
    }
}

/// Parse a comma-separated list of flags: "nonblock,noatime,direct"
pub fn parse_flags(input: &str) -> Result<Vec<IoFlag>> {
    if input.is_empty() {
        return Ok(vec![]);
    }
    input.split(',').map(|s| IoFlag::parse(s.trim())).collect()
}

// =============================================================================
// File opening
// =============================================================================

/// Options for opening input files.
pub struct InputOptions {
    pub path: Option<String>,
    pub flags: Vec<IoFlag>,
    pub must_be_directory: bool,
}

/// Options for opening output files.
pub struct OutputOptions {
    pub path: Option<String>,
    pub flags: Vec<IoFlag>,
    pub excl: bool,    // conv=excl: fail if file exists
    pub nocreat: bool, // conv=nocreat: don't create, must exist
    pub notrunc: bool, // conv=notrunc: don't truncate
    pub must_be_directory: bool,
}

/// Open the input file (or use stdin).
pub fn open_input(opts: &InputOptions) -> Result<File> {
    match &opts.path {
        None => {
            // stdin — dup fd 0 so we own an independent handle
            let raw_fd = io::stdin().as_raw_fd();
            let dup_fd = unsafe { libc::dup(raw_fd) };
            if dup_fd < 0 {
                return Err(Error::Io(io::Error::last_os_error()));
            }
            Ok(unsafe { File::from_raw_fd(dup_fd) })
        }
        Some(path) => {
            let path = Path::new(path);
            let mut open_opts = OpenOptions::new();
            open_opts.read(true);

            for flag in &opts.flags {
                flag.apply_to_open_options(&mut open_opts);
            }

            if opts.must_be_directory || opts.flags.contains(&IoFlag::Directory) {
                let md = std::fs::metadata(path).map_err(|e| Error::Io(e))?;
                if !md.is_dir() {
                    return Err(Error::InvalidArgument(format!(
                        "{} is not a directory",
                        path.display()
                    )));
                }
            }

            open_opts
                .open(path)
                .map_err(|e| Error::Io(e))
        }
    }
}

/// Open the output file (or use stdout).
pub fn open_output(opts: &OutputOptions) -> Result<File> {
    match &opts.path {
        None => {
            // stdout — dup fd 1 so we own an independent handle
            let raw_fd = io::stdout().as_raw_fd();
            let dup_fd = unsafe { libc::dup(raw_fd) };
            if dup_fd < 0 {
                return Err(Error::Io(io::Error::last_os_error()));
            }
            Ok(unsafe { File::from_raw_fd(dup_fd) })
        }
        Some(path) => {
            let path = Path::new(path);

            // conv=excl: fail if exists
            if opts.excl && path.exists() {
                return Err(Error::FileExists {
                    path: path.to_path_buf(),
                });
            }

            // conv=nocreat: file must exist
            if opts.nocreat && !path.exists() {
                return Err(Error::FileNotFound {
                    path: path.to_path_buf(),
                });
            }

            let mut open_opts = OpenOptions::new();
            open_opts.write(true).create(!opts.nocreat);

            // Truncate unless notrunc or append
            if !opts.notrunc && !opts.flags.contains(&IoFlag::Append) {
                open_opts.truncate(true);
            }

            for flag in &opts.flags {
                flag.apply_to_open_options(&mut open_opts);
            }

            if opts.must_be_directory || opts.flags.contains(&IoFlag::Directory) {
                if path.exists() {
                    let md = std::fs::metadata(path).map_err(|e| Error::Io(e))?;
                    if !md.is_dir() {
                        return Err(Error::InvalidArgument(format!(
                            "{} is not a directory",
                            path.display()
                        )));
                    }
                }
            }

            open_opts
                .open(path)
                .map_err(|e| Error::Io(e))
        }
    }
}

/// Synchronize file data and/or metadata (conv=fsync / conv=fdatasync).
pub fn fsync_output(file: &File, data_only: bool) -> Result<()> {
    if data_only {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::io::AsRawFd;
            let fd = file.as_raw_fd();
            let ret = unsafe { libc::fdatasync(fd) };
            if ret != 0 {
                return Err(Error::Io(io::Error::last_os_error()));
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            // macOS/BSD: fdatasync is not available, fall back to fsync
            file.sync_all().map_err(|e| Error::Io(e))?;
        }
    } else {
        file.sync_all().map_err(|e| Error::Io(e))?;
    }
    Ok(())
}

/// Drop kernel cache for a file (iflag= / oflag=nocache). Best-effort.
pub fn drop_cache(file: &File) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();
        // POSIX_FADV_DONTNEED — tell kernel we won't need these pages again
        unsafe {
            libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_DONTNEED);
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = file;
        log::debug!("nocache flag not supported on this platform; ignoring");
    }
}
