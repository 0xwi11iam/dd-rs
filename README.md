# dd-rs — A Safe, Modern alternative to `dd`

## Inspiration
Numerous devs have struggled with the annoying commands in dd that is standard in Unix and many databases and SSDs have been wrecked (me included) by the pure destructive potential of the tool with no safety confirmations built in 1970s. I built this for safety and easier commands so new devs can use the power of the tool with appropriate safety guardrails so no more SSDs get wiped by mistake!

**dd-rs** is a Rust+C reimplementation of the Unix `dd` command — the standard tool for copying and converting data. It matches every feature of GNU dd while adding safety guards, performance optimizations, and a modern CLI that won't let you accidentally destroy your hard drive.

```
$ dd-rs if=/dev/zero of=test.bin bs=1M count=100 status=progress
100+0 records in  ·  100+0 records out  ·  104857600 bytes (100 MB) copied, 0.042s, 2.5 GB/s

$ dd-rs copy ubuntu.iso /dev/sda --size 4M
⚠ Target '/dev/sda' is a BLOCK DEVICE. Writing will DESTROY ALL DATA.
Type 'YES' to proceed: _
```

---

## Table of Contents

1. [Why dd-rs?](#why-dd-rs)
2. [Installation](#installation)
3. [Quick Start](#quick-start)
4. [Dual Command Syntax](#dual-command-syntax)
5. [Complete Operand Reference](#complete-operand-reference)
6. [Conversion Reference (conv=)](#conversion-reference-conv)
7. [I/O Flags Reference (iflag=/oflag=)](#io-flags-reference-iflogoflag)
8. [Size Suffix Reference](#size-suffix-reference)
9. [Status Levels](#status-levels)
10. [Safety System](#safety-system)
11. [Performance](#performance)
12. [Architecture](#architecture)
13. [Signal Handling](#signal-handling)
14. [Exit Codes](#exit-codes)
15. [Environment Variables](#environment-variables)
16. [GNU dd Compatibility](#gnu-dd-compatibility)
17. [Examples Cookbook](#examples-cookbook)
18. [FAQ](#faq)

---

## Why dd-rs?

GNU dd is powerful but dangerous. It was designed in the 1970s for tape drives and has barely changed since. The result:

| dd problem | dd-rs solution |
|---|---|
| **Silently destroys disks** — `dd if=img of=/dev/sda` runs with zero warnings | **5-layer safety system** — detects block devices, mounted partitions, system disks, and prompts for confirmation |
| **512-byte default block size** — causes millions of syscalls/sec on modern NVMe drives, wasting CPU | **Auto-tuned block sizes** — defaults to 128 KiB, uses `copy_file_range(2)` for zero-copy kernel transfers |
| **Cryptic `key=value` syntax** — hard to remember, easy to mistype | **Dual syntax** — legacy `if=/dev/zero of=out bs=1M` AND modern `dd-rs copy /dev/zero out --size 1M` |
| **No progress** — you don't know if it's working or hung | **Progress by default** — shows speed, ETA, bytes transferred. JSON output for scripting. `SIGUSR1` for on-demand stats. |
| **No explanation** — what does `conv=swab,noerror,sync` actually do? | **`--explain` mode** — explains every operand, shows data flow, assesses risk, estimates time |
| **2500 lines of 1970s C** — hard to audit, no memory safety | **Memory-safe Rust** — no buffer overflows, no use-after-free, with C only for lookup tables |

---

## Installation

### One-liner (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/0xwi11iam/dd-rs/main/install.sh | bash
```

This clones the repo, checks for Rust + a C compiler, builds, and installs to `/usr/local/bin/dd-rs`.

### From source

```bash
git clone https://github.com/0xwi11iam/dd-rs.git
cd dd-rs
cargo build --release
sudo cp target/release/dd-rs /usr/local/bin/
```

**Requirements:** Rust 1.70+, a C compiler (gcc/clang), Linux or macOS.

### Verify installation

```bash
$ dd-rs --version
dd-rs 0.1.0

$ dd-rs info /dev/null
╔══════════════════════════════════════════════════════════════╗
║                    DD-RS DEVICE INFO                         ║
╚══════════════════════════════════════════════════════════════╝
  Path:        /dev/null
  Type:        CharDevice
  Risk:        0/100 (Safe)
```

---

## Quick Start

```bash
# Copy a file (modern syntax)
dd-rs copy input.dat output.dat

# Copy a file (legacy dd syntax — 100% compatible)
dd-rs if=input.dat of=output.dat bs=1M

# Create a 1 GB file of zeros
dd-rs zero disk.img --size 1G

# Generate a random 32-byte key
dd-rs random key.bin --bytes 32

# Check what a command will do before running it
dd-rs explain if=/dev/zero of=/dev/sda bs=4M

# Inspect a device
dd-rs info /dev/sda

# Wipe a USB drive (with safety confirmation)
dd-rs wipe /dev/sdb
```

---

## Dual Command Syntax

dd-rs accepts **both** traditional dd syntax and modern human-friendly commands. The preprocessor automatically converts `key=value` pairs to flags — every existing dd script works unchanged.

### Legacy dd syntax (100% backward compatible)

```bash
dd-rs if=FILE of=FILE [ibs=N] [obs=N] [bs=N] [count=N] [skip=N] [seek=N] [conv=...] [iflag=...] [oflag=...] [status=LEVEL]
```

Examples:
```bash
dd-rs if=/dev/zero of=test.bin bs=1M count=100
dd-rs if=input.dat of=output.dat conv=swab,noerror,sync
dd-rs if=/dev/urandom of=key.bin bs=32 count=1 status=none
dd-rs if=/dev/sda of=disk.img bs=4M status=progress
```

### Modern subcommands

| Command | Purpose | Example |
|---------|---------|---------|
| `dd-rs copy <IN> <OUT>` | Basic file/device copy | `dd-rs copy data.bin backup.bin --size 4M` |
| `dd-rs zero <OUT>` | Fill output with zeros | `dd-rs zero empty.img --size 1G` |
| `dd-rs random <OUT>` | Fill output with random bytes | `dd-rs random key.bin --bytes 32` |
| `dd-rs wipe <DEVICE>` | Securely erase a device | `dd-rs wipe /dev/sdb --passes 3` |
| `dd-rs info <PATH>` | Inspect a file or device | `dd-rs info /dev/nvme0n1` |
| `dd-rs explain <CMD>` | Explain what a command will do | `dd-rs explain if=/dev/zero of=/dev/sda` |

All subcommands accept these flags:

| Flag | Alias | Purpose |
|------|-------|---------|
| `--size N` | `--bs N` | Block/chunk size (default: 1M) |
| `--count N` | | Number of blocks to copy |
| `--explain` | `-E` | Explain instead of executing |
| `--yes` | `-y` | Skip confirmation prompts |
| `--force` | | Skip ALL safety checks (dangerous) |

---

## Complete Operand Reference

Every dd operand is supported. Operands can be specified in either form: `key=value` (legacy) or `--key value` (GNU style).

### Core I/O Operands

| Operand | Description | Default |
|---------|-------------|---------|
| `if=FILE` | Read from FILE instead of stdin | stdin |
| `of=FILE` | Write to FILE instead of stdout | stdout |
| `ibs=BYTES` | Input block size — how many bytes to read per `read(2)` call | 512 |
| `obs=BYTES` | Output block size — how many bytes to write per `write(2)` call | 512 |
| `bs=BYTES` | Set both `ibs` and `obs` to BYTES (overrides both) | — |
| `cbs=BYTES` | Conversion buffer size — used by `block`/`unblock`/`ascii`/`ebcdic` conversions | 0 |
| `count=N` | Copy only N input blocks (NOT N bytes — use `NB` for byte count) | unlimited |
| `skip=N` | Skip N input blocks before starting to copy | 0 |
| `iseek=N` | Alias for `skip` | 0 |
| `seek=N` | Skip N output blocks before writing (writes start at offset `N × obs`) | 0 |
| `oseek=N` | Alias for `seek` | 0 |

### Status Control

| Operand | Description |
|---------|-------------|
| `status=none` | Suppress all output except errors |
| `status=noxfer` | Suppress final transfer statistics (errors still shown) |
| `status=progress` | Show periodic progress during transfer (default in dd-rs) |
| `status=json` | Output final statistics as JSON to stderr |

### dd-rs Extras (beyond GNU dd)

| Flag | Description |
|------|-------------|
| `--explain`, `-E` | Explain the command and assess risk without executing |
| `--dry-run` | Validate all arguments without transferring data |
| `--auto-tune` | Automatically select optimal block sizes for your hardware |
| `--progress-bar` | Show a visual progress bar |
| `--yes`, `-y` | Skip interactive confirmation prompts (warnings still shown) |
| `--force` | Skip ALL safety checks (equivalent to GNU dd behaviour) |

### Count, Skip, and Seek: Blocks vs Bytes

By default, `count`, `skip`, and `seek` operate on **blocks** (not bytes):

```bash
# Copy 10 blocks of 512 bytes = 5120 bytes
dd-rs if=input of=output count=10

# Copy 10 blocks of 1 MiB = 10 MiB
dd-rs if=input of=output bs=1M count=10
```

**GNU extension:** Append `B` to count in bytes:

```bash
# Copy exactly 1000 bytes (not 1000 blocks)
dd-rs if=input of=output count=1000B

# Skip 4096 bytes
dd-rs if=input of=output skip=4096B
```

---

## Conversion Reference (conv=)

Conversions transform data as it passes from input to output. Multiple conversions can be combined with commas: `conv=swab,noerror,sync`.

> **Important:** Conversions are applied in a **fixed canonical order** regardless of the order you specify. This matches GNU dd behaviour.

### Conversion Pipeline Order

```
ebcdic / ascii / ibm  →  block / unblock  →  lcase / ucase  →  swab  →  sync
```

### All Conversions

| Conversion | Category | Description |
|-----------|----------|-------------|
| `ascii` | Character set | Convert EBCDIC → ASCII. **Implies `unblock`.** |
| `ebcdic` | Character set | Convert ASCII → EBCDIC (CP037). **Implies `block`.** |
| `ibm` | Character set | Convert ASCII → alternate EBCDIC (IBM1047). Differs from CP037 in `~`, `[`, `]` mapping. **Implies `block`.** |
| `block` | Record | Pad newline-terminated variable-length records with spaces to `cbs` bytes. Longer records are truncated. |
| `unblock` | Record | Replace trailing spaces in `cbs`-sized fixed-length blocks with a newline. |
| `lcase` | Case | Map uppercase A–Z to lowercase a–z. Non-ASCII bytes pass through unchanged. |
| `ucase` | Case | Map lowercase a–z to uppercase A–Z. Non-ASCII bytes pass through unchanged. |
| `swab` | Binary | Swap every pair of input bytes. If the block has an odd number of bytes, the last byte is unchanged. Useful for endianness conversion. |
| `sync` | Padding | Pad every input block with NUL bytes (or spaces, if `block`/`unblock` is active) to `ibs` size. Ensures fixed-size output blocks even on short reads. |
| `sparse` | Output | Detect all-NUL output blocks and `seek(2)` past them instead of `write(2)`-ing zeros. Creates sparse files that use less disk space. |
| `noerror` | Error handling | Continue processing after read errors instead of aborting. The failed block is skipped (lost). Use with `sync` to pad failed blocks with NULs. |
| `notrunc` | File mode | Do not truncate the output file before writing. Combined with `seek`, data past the written region is preserved. |
| `excl` | File mode | Fail with an error if the output file already exists. Like `O_EXCL`. |
| `nocreat` | File mode | Fail with an error if the output file does NOT already exist. The file must be created beforehand. |
| `fdatasync` | Sync | Force a physical write of output data (not metadata) to storage before exiting. |
| `fsync` | Sync | Force a physical write of both output data and metadata to storage before exiting. |

### Conversion Examples

```bash
# Convert EBCDIC mainframe data to ASCII
dd-rs if=mainframe.dat of=ascii.txt conv=ascii cbs=80

# Swap byte order (big-endian → little-endian)
dd-rs if=be.dat of=le.dat conv=swab

# Lowercase a text file
dd-rs if=UPPER.txt of=lower.txt conv=lcase

# Clone a failing disk: skip errors, pad with NULs, don't stop
dd-rs if=/dev/failing-disk of=recovery.img conv=noerror,sync bs=4M

# Create a sparse 10 GB file (uses almost no actual disk space)
dd-rs if=/dev/zero of=sparse.img bs=1M count=10240 conv=sparse

# Write at offset 1 MiB without truncating the rest of the file
dd-rs if=patch.dat of=existing.file bs=1K seek=1024 conv=notrunc
```

---

## I/O Flags Reference (iflag= / oflag=)

I/O flags control **how** files are opened and **how** read/write system calls behave. They map to `open(2)` flags and `fcntl(2)` options.

| Flag | `iflag=` | `oflag=` | System call | Description |
|------|:---:|:---:|-------------|-------------|
| `append` | — | ✅ | `O_APPEND` | Open output in append mode (all writes go to end of file) |
| `direct` | ✅ | ✅ | `O_DIRECT` | Use direct I/O — bypass the kernel buffer cache. Requires aligned buffers. Linux only. |
| `directory` | ✅ | ✅ | — | Fail if the path is not a directory |
| `dsync` | ✅ | ✅ | `O_DSYNC` | Synchronized I/O for data integrity on every write. Linux only. |
| `sync` | ✅ | ✅ | `O_SYNC` | Synchronized I/O for data + metadata integrity on every write. Linux only. |
| `nonblock` | ✅ | ✅ | `O_NONBLOCK` | Use non-blocking I/O — `read(2)` returns immediately if no data is available |
| `noatime` | ✅ | ✅ | `O_NOATIME` | Do not update the file's access time on read. Linux only. |
| `nocache` | ✅ | ✅ | `posix_fadvise` | Request the kernel to drop cached pages after I/O (best-effort). Linux only. |
| `noctty` | ✅ | ✅ | `O_NOCTTY` | Do not assign the opened device as the controlling terminal |
| `nofollow` | ✅ | ✅ | `O_NOFOLLOW` | Do not follow symbolic links — fail if the path is a symlink |
| `fullblock` | ✅ | — | — | Accumulate full input blocks — retry `read(2)` until `ibs` bytes are read or EOF. **Critical for pipes and sockets.** |

### I/O Flag Examples

```bash
# Direct I/O (bypass cache) with sparse output — fast disk imaging
dd-rs if=/dev/sda of=disk.img bs=4M iflag=direct conv=sparse

# Non-blocking read from a named pipe
dd-rs if=./mypipe of=output.dat iflag=nonblock,fullblock

# Append to a log file without updating access time
dd-rs if=/dev/zero of=log.dat bs=1K count=10 oflag=append,noatime

# Don't follow symlinks (safety)
dd-rs if=/dev/zero of=/dev/disk/by-id/symlink oflag=nofollow
```

---

## Size Suffix Reference

All operands that accept a byte count support the full GNU dd suffix syntax.

### Basic Suffixes

| Suffix | Multiplier | Example | Bytes |
|--------|-----------|---------|-------|
| `c` | 1 | `10c` | 10 |
| `w` | 2 | `10w` | 20 |
| `b` | 512 | `10b` | 5,120 |

### Binary Suffixes (Powers of 1024)

| Suffix | Multiplier | Example | Bytes |
|--------|-----------|---------|-------|
| `K` | 1,024 | `4K` | 4,096 |
| `M` | 1,048,576 | `4M` | 4,194,304 |
| `G` | 1,073,741,824 | `1G` | 1,073,741,824 |
| `T` | 1024⁴ | `1T` | 1,099,511,627,776 |
| `P` | 1024⁵ | — | — |
| `E` | 1024⁶ | — | — |

### Explicit Binary Suffixes (IEC)

| Suffix | Multiplier | Example | Bytes |
|--------|-----------|---------|-------|
| `KiB` | 1,024 | `4KiB` | 4,096 |
| `MiB` | 1,048,576 | `4MiB` | 4,194,304 |
| `GiB` | 1,073,741,824 | `1GiB` | 1,073,741,824 |
| `TiB` | 1024⁴ | — | — |
| `PiB` | 1024⁵ | — | — |
| `EiB` | 1024⁶ | — | — |

### Decimal Suffixes (Powers of 1000)

| Suffix | Multiplier | Example | Bytes |
|--------|-----------|---------|-------|
| `kB` | 1,000 | `4kB` | 4,000 |
| `MB` | 1,000,000 | `4MB` | 4,000,000 |
| `GB` | 1,000,000,000 | `1GB` | 1,000,000,000 |
| `TB` | 1000⁴ | — | — |
| `PB` | 1000⁵ | — | — |
| `EB` | 1000⁶ | — | — |

### Multiplication Syntax

| Syntax | Meaning | Example | Bytes |
|--------|---------|---------|-------|
| `xM` | Times M (1,048,576) | `4xM` | 4,194,304 |
| `xK` | Times K (1,024) | `10xK` | 10,240 |

### GNU Byte-Count Mode

Append `B` to make `count=`, `skip=`, and `seek=` count **bytes** instead of **blocks**:

```bash
dd-rs if=input of=output count=1000B    # 1000 bytes, not 1000 blocks
dd-rs if=input of=output skip=4096B     # skip 4096 bytes
```

### Hexadecimal

```bash
dd-rs if=input of=output bs=0x1000      # 4096 bytes
dd-rs if=input of=output count=0xFF     # 255 blocks
```

---

## Status Levels

The `status=` operand controls what dd-rs prints to stderr.

| Level | Behaviour |
|-------|-----------|
| `none` | Suppress all output except fatal errors. Like `dd status=none`. |
| `noxfer` | Suppress final transfer statistics. Errors are still shown. |
| `progress` | **Default in dd-rs.** Shows periodic progress: bytes transferred, speed, elapsed time. |
| `json` | Output final statistics as a single JSON object to stderr. Ideal for scripting. |

### Progress Output

```
104857600 bytes (100.0 MB) copied, 0.042s, 2.5 GB/s
100+0 records in
100+0 records out
104857600 bytes transferred in 0.042000 secs (2.5 GB/s)
```

### JSON Output

```json
{
  "bytes_read": 104857600,
  "bytes_written": 104857600,
  "full_blocks_in": 100,
  "partial_blocks_in": 0,
  "full_blocks_out": 100,
  "partial_blocks_out": 0,
  "read_errors": 0,
  "elapsed_seconds": 0.042000,
  "throughput_bytes_per_sec": 2496610000.0
}
```

---

## Safety System

This is dd-rs's most important feature. GNU dd will silently overwrite your boot sector without asking. dd-rs won't.

### How It Works

Before opening any output file, dd-rs inspects the target:

1. **Device type detection** — Is it a regular file? A block device? A symlink to a device?
2. **Mount check** — Is the device currently mounted? Writing to a mounted filesystem corrupts it.
3. **System-critical path check** — Is it `/dev/sda`, `/dev/nvme0n1`, `/dev/disk0`?
4. **Risk scoring** — A numeric 0–100 score is computed from 13+ risk factors.
5. **Confirmation prompt** — If the risk level is `Caution` or higher, dd-rs asks for confirmation.

### Risk Levels

| Score | Level | Meaning |
|-------|-------|---------|
| 0–24 | ✅ Safe | Regular file copy, no concerns |
| 25–49 | ⚠️ Caution | Minor issues — small blocks, unknown sizes, stdout redirect |
| 50–74 | 🔶 Dangerous | Could cause data loss — block device partition, LVM volume, system path |
| 75–100 | ☠️ Catastrophic | Will destroy your system — raw disk device, root filesystem |

### Risk Factors

| Risk Factor | Score | Example Trigger |
|-------------|:-----:|-----------------|
| Raw disk device (no partition number) | +60 | `/dev/sda`, `/dev/nvme0n1` |
| Root filesystem target | +50 | `of=/` |
| Block device partition | +40 | `/dev/sda1` |
| Same file as input and output | +40 | `if=data of=data` |
| LVM logical volume / device-mapper | +35 | `/dev/mapper/vg-lv` |
| md RAID device | +35 | `/dev/md0` |
| Device symlink | +30 | `/dev/disk/by-id/...` |
| ZFS zvol | +30 | `/dev/zvol/pool/vol` |
| /dev/ path (not null/zero/random) | +20 | `/dev/tty` as output |
| Unbounded write (no count) | +20 | No `count=` on device target |
| Loop device | +15 | `/dev/loop0` |
| Running as root | +15 | `geteuid() == 0` |
| System directory target | +15 | `/boot`, `/etc`, `/usr/bin` |
| conv=notrunc with output file | +10 | Partial overwrite risk |
| Device as input | +10 | `if=/dev/sda` |
| conv=noerror | +10 | Silent data corruption |
| FAT32 4GB file size limit | +10 | Output > 4 GB |
| Same physical device I/O | +10 | Read+write thrashing |
| Tiny block size (< 512 B) | +5 | Performance catastrophe |
| conv=fsync on every block | +5 | 10–100× slower than normal |
| Stdout redirect risk | +5 | Shell bypasses safety checks |
| RAM disk target | +5 | Data lost on reboot |

### Controlling Safety Behaviour

```bash
# Default: prompt for confirmation on dangerous operations
dd-rs if=image.iso of=/dev/sda

# Non-interactive: show warnings but don't prompt (for scripts)
dd-rs --yes if=image.iso of=/dev/sda

# Force unsafe: skip ALL checks (like GNU dd — use at your own risk)
dd-rs --force if=image.iso of=/dev/sda

# Preview: check what would happen without doing it
dd-rs --dry-run if=image.iso of=/dev/sda
```

### Example: Blocked Operation

```
$ dd-rs if=ubuntu.iso of=/dev/sda bs=4M

⚠ DD-RS SAFETY WARNING ⚠
Target '/dev/sda' is a BLOCK DEVICE.
Writing to it will DESTROY ALL DATA on this device.
Device size: 500107862016 bytes (500.1 GB)

This operation is DANGEROUS and may destroy data.
Type 'YES' (uppercase) to proceed, anything else to abort: no
dd-rs: Operation cancelled by user (safety check failed).
```

---

## Performance

dd-rs uses a **tiered execution model** that automatically selects the fastest transfer method:

| Tier | Condition | Mechanism | Speed vs GNU dd |
|:----:|-----------|-----------|:---------------:|
| 1 | No conversions, regular files, Linux 4.5+ | `copy_file_range(2)` — kernel copies FD→FD directly, zero userspace copies | **1.5–3×** |
| 2 | No conversions, macOS | `fcopyfile(3)` — similar zero-copy kernel primitive | **1.5–3×** |
| 3 | No conversions, any Unix | `sendfile(2)` — kernel-space pipe | **1.2–2×** |
| 4 | Conversions needed | Double-buffered read/write with read-ahead — overlaps I/O with CPU | ~1× |
| 5 | Complex (sparse, noerror, fullblock) | Standard dd-compatible loop | ~1× |

### Why dd Is Slow

GNU dd's default block size is **512 bytes** — designed for 1970s 9-track tape drives:

| Block size | Syscalls to copy 10 GB | CPU overhead | Result |
|-----------|------------------------|:------------:|--------|
| 512 B (dd default) | 20,971,520 | ~2–10 seconds | **Catastrophic** |
| 128 KiB (dd-rs auto-tune) | 81,920 | ~8 ms | Negligible |
| 1 MiB | 10,240 | ~1 ms | None |

dd-rs warns about small block sizes and offers `--auto-tune`:

```bash
$ dd-rs if=/dev/zero of=/dev/null bs=512 count=1000000
dd-rs: note: block size is 512 bytes. For better performance, try --auto-tune
or set bs=128K or larger. Small block sizes cause millions of syscalls/second.
```

---

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                         main.rs                                  │
│  ┌─────────────┐   ┌────────────────┐   ┌────────────────────┐  │
│  │ key=value   │   │ Subcommand     │   │ Safety check       │  │
│  │ preprocessor│──▶│ dispatch       │──▶│ (risk score +      │  │
│  │ if= → --if  │   │ copy/zero/...  │   │  confirmation)     │  │
│  └─────────────┘   └────────────────┘   └────────────────────┘  │
│                                                  │               │
│                    ┌─────────────────────────────▼──────────┐    │
│                    │           io_engine.rs                 │    │
│                    │  detect_tier() → dispatch              │    │
│                    │                                        │    │
│                    │  Tier 1: copy_file_range()  ← zero-copy│    │
│                    │  Tier 4: double-buffered     ← overlap │    │
│                    │  Tier 5: standard loop       ← dd-compat│   │
│                    └────────┬─────────────────────┬─────────┘    │
│                             │                     │              │
│              ┌──────────────▼──┐       ┌─────────▼──────────┐   │
│              │  conv/mod.rs    │       │  flags.rs          │   │
│              │  Pipeline:      │       │  O_DIRECT, O_SYNC, │   │
│              │  ebcdic→block→  │       │  O_NONBLOCK, etc.  │   │
│              │  lcase→swab→sync│       │  excl, nocreat     │   │
│              └──────┬──────────┘       └────────────────────┘   │
│                     │                                           │
│     ┌───────────────┼───────────────┐                           │
│     │               │               │                           │
│  ┌──▼──────┐  ┌─────▼────┐  ┌──────▼──────┐                    │
│  │ebcdic.rs│  │ block.rs │  │ case.rs      │                    │
│  │ FFI → C │  │ cbs pad  │  │ lcase/ucase  │                    │
│  └─────────┘  └──────────┘  │ swab.rs      │                    │
│                             │ byte swap    │                    │
│                             └─────────────┘                    │
│                                                                  │
│  ┌────────────┐  ┌───────────┐  ┌────────────┐  ┌───────────┐  │
│  │ safety.rs  │  │explain.rs │  │ signal.rs  │  │ status.rs │  │
│  │ 13-factor  │  │ 7-section │  │ SIGUSR1    │  │ progress  │  │
│  │ risk score │  │ explainer │  │ handler    │  │ JSON/etc  │  │
│  └────────────┘  └───────────┘  └────────────┘  └───────────┘  │
│                                                                  │
│  c_src/ (C layer)                                               │
│  ┌─────────────────┐  ┌──────────────────┐                      │
│  │ ebcdic_tables.c │  │ conv_helpers.c   │                      │
│  │ 4×256 lookup    │  │ swab, lcase,     │                      │
│  │ CP037 + IBM1047 │  │ ucase (C loops)  │                      │
│  └─────────────────┘  └──────────────────┘                      │
└──────────────────────────────────────────────────────────────────┘
```

---

## Signal Handling

Send `SIGUSR1` to a running dd-rs process to print current I/O statistics:

```bash
$ dd-rs if=/dev/zero of=/dev/null bs=1M count=100000 &
[1] 12345

$ kill -USR1 12345
4500+0 records in
4500+0 records out
4718592000 bytes
```

`SIGUSR2` does the same thing. This matches GNU dd's behaviour exactly.

---

## Exit Codes

| Code | Meaning |
|:----:|---------|
| 0 | Success — all requested data was copied without errors |
| 1 | General error — invalid argument, file not found, safety block, user cancelled |
| 2 | I/O error — a read or write error occurred (or `conv=noerror` recovered from errors) |
| 3 | Conversion error — data conversion problem (e.g., invalid EBCDIC sequence) |

---

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `POSIXLY_CORRECT` | If set, dd-rs behaves closer to POSIX dd (fewer GNU extensions) |
| `RUST_LOG` | Control log verbosity: `error`, `warn` (default), `info`, `debug`, `trace` |

---

## GNU dd Compatibility

dd-rs aims for **full GNU dd compatibility** for all features.

| Feature | Status |
|---------|:------:|
| All core operands (`if`, `of`, `ibs`, `obs`, `bs`, `cbs`, `count`, `skip`, `seek`, `status`) | ✅ |
| All 16 conversions (`ascii`, `ebcdic`, `ibm`, `block`, `unblock`, `lcase`, `ucase`, `swab`, `sync`, `sparse`, `noerror`, `notrunc`, `excl`, `nocreat`, `fdatasync`, `fsync`) | ✅ |
| All 11 I/O flags (`append`, `direct`, `directory`, `dsync`, `sync`, `nonblock`, `noatime`, `nocache`, `noctty`, `nofollow`, `fullblock`) | ✅ |
| All size suffixes (`c`, `w`, `b`, `K`, `M`, `G`, `kB`, `MB`, `KiB`, `MiB`, `xM`, hex, `B` byte-count) | ✅ |
| `SIGUSR1` progress dump | ✅ |
| Exit codes 0/1/2/3 | ✅ |
| Canonical conversion ordering | ✅ |
| `status=progress` periodic output | ✅ (default) |
| `status=none` / `status=noxfer` | ✅ |
| `conv=sparse` hole-punching | ✅ |
| `iflag=fullblock` accumulation | ✅ (all tiers) |
| `conv=noerror` recovery with `sync` padding | ✅ |

### Differences from GNU dd

| Aspect | GNU dd | dd-rs |
|--------|--------|--------|
| Default block size | 512 bytes | 512 bytes (warns, suggests `--auto-tune`) |
| Default status | `none` | `progress` |
| Safety checks | None | Full risk assessment + confirmation |
| Block device writes | Silent | Warns and confirms |
| Performance | Single read/write loop | Tiered: zero-copy → double-buffered → standard |
| `status=json` | Not available | ✅ |
| `--explain` mode | Not available | ✅ |
| Modern subcommands | Not available | ✅ |

---

## Examples Cookbook

### Basic File Operations

```bash
# Copy a file
dd-rs if=input.dat of=output.dat bs=1M

# Copy with modern syntax
dd-rs copy input.dat output.dat

# Copy only the first 10 MiB
dd-rs if=bigfile.dat of=chunk.dat bs=1M count=10

# Skip the first 1 MiB, then copy 512 KiB
dd-rs if=bigfile.dat of=tail.dat bs=1K skip=1024 count=512
```

### Creating Test Files

```bash
# Create a 1 GB file of zeros
dd-rs zero empty.img --size 1G

# Create a 10 GB sparse file (uses almost no disk space)
dd-rs if=/dev/zero of=sparse.img bs=1M count=10240 conv=sparse

# Create a file with a specific pattern
echo "HELLO WORLD" | dd-rs of=pattern.bin bs=11 count=100
```

### Disk Operations

```bash
# Clone a disk (with confirmation)
dd-rs if=/dev/sda of=/dev/sdb bs=4M status=progress

# Clone a disk to an image file
dd-rs if=/dev/sda of=disk.img bs=4M conv=sparse status=progress

# Restore an image to a disk
dd-rs if=disk.img of=/dev/sda bs=4M status=progress

# Wipe a disk with zeros
dd-rs wipe /dev/sdc

# Rescue a failing disk (skip errors)
dd-rs if=/dev/failing-disk of=recovery.img conv=noerror,sync bs=4M status=progress
```

### Data Conversion

```bash
# Convert EBCDIC to ASCII (mainframe data)
dd-rs if=mainframe.dat of=ascii.txt conv=ascii cbs=80

# Swap byte order (endianness)
dd-rs if=big-endian.dat of=little-endian.dat conv=swab

# Lowercase a text file
dd-rs if=UPPERCASE.txt of=lowercase.txt conv=lcase

# Convert fixed-width records to newline-delimited
dd-rs if=fixed.dat of=lines.txt conv=unblock cbs=132

# Convert newline-delimited to fixed-width records
dd-rs if=lines.txt of=fixed.dat conv=block cbs=132
```

### Cryptography & Security

```bash
# Generate a random 256-bit key (32 bytes)
dd-rs random encryption.key --bytes 32

# Overwrite a file before deletion
dd-rs if=/dev/urandom of=secret.txt bs=1K count=$(wc -c < secret.txt)
rm secret.txt
```

### Advanced I/O

```bash
# Direct I/O (bypass kernel cache) for benchmarking
dd-rs if=/dev/zero of=/dev/null bs=4M iflag=direct status=progress

# Non-blocking read from a FIFO
mkfifo mypipe
dd-rs if=mypipe of=output.dat iflag=nonblock,fullblock

# Append without updating access time
dd-rs if=/dev/zero of=log.dat bs=1K count=10 oflag=append,noatime

# Write at a specific offset without truncating
dd-rs if=patch.dat of=existing.dat seek=1024 conv=notrunc
```

### Scripting

```bash
# Get JSON stats
dd-rs if=input.dat of=output.dat bs=1M status=json 2>stats.json

# Check if a command is safe before running
dd-rs explain if=/dev/sda of=/dev/sdb bs=4M
if [ $? -eq 0 ]; then
    dd-rs if=/dev/sda of=/dev/sdb bs=4M status=progress
fi

# Monitor progress from another terminal
dd-rs if=/dev/zero of=/dev/null bs=1M count=100000 &
while kill -USR1 $! 2>/dev/null; do sleep 5; done
```

---

## FAQ

### Is dd-rs a drop-in replacement for dd?

**Yes, for all practical purposes.** Every dd operand, conversion, flag, and size suffix is supported. The key=value syntax (`if=file`, `of=file`, `bs=1M`) works exactly as you expect.

### Will my existing dd scripts break?

**No.** The `key=value` preprocessor converts dd syntax to internal flags transparently.

### Is it faster than dd?

**Yes, for most common cases.** For simple copies, dd-rs uses `copy_file_range(2)` — a kernel-level zero-copy operation that's 1.5–3× faster. Even in the standard path, auto-tuned block sizes (128 KiB vs dd's 512 bytes) eliminate syscall overhead.

### What platforms are supported?

**Linux** (primary) and **macOS**. Some I/O flags (`direct`, `dsync`, `sync`, `noatime`, `nocache`) are Linux-only.

### How do I skip the safety prompts?

Use `--yes` for non-interactive mode or `--force` to skip all checks.

### What's with the name?

I just like it. dd made in Rust

### Why Rust + C?

EBCDIC tables are large `const` arrays best expressed in C. Everything else is safe Rust.

---

## License

MIT OR Apache-2.0

<p align="center">
  <sub>Built to make <code>dd</code> safe. Never accidentally destroy a disk again.</sub>
</p>
