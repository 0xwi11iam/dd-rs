# dd-rs Reference Card

> Quick syntax reference. Full docs: [README.md](README.md)

## Legacy dd Syntax (100% compatible)

```
dd-rs if=FILE of=FILE [ibs=N] [obs=N] [bs=N] [cbs=N] [count=N] [skip=N] [seek=N] [conv=LIST] [iflag=LIST] [oflag=LIST] [status=LEVEL]
```

## Modern Subcommands

```
dd-rs copy   <INPUT> <OUTPUT>  [--size N] [--count N]   Basic copy
dd-rs zero   <OUTPUT>          --size N                  Fill with zeros
dd-rs random <OUTPUT>          --bytes N                 Random data
dd-rs wipe   <DEVICE>          [--passes N]              Secure erase
dd-rs info   <PATH>                                      Device info + risk
dd-rs explain <DD_COMMAND>                               Explain command
```

## Conversions (`conv=`)

| Conv | Effect |
|------|--------|
| `ascii` | EBCDIC→ASCII (implies unblock) |
| `ebcdic` | ASCII→EBCDIC CP037 (implies block) |
| `ibm` | ASCII→EBCDIC IBM1047 (implies block) |
| `block` | Pad newline records to cbs with spaces |
| `unblock` | Trailing spaces → newline |
| `lcase` | A-Z → a-z |
| `ucase` | a-z → A-Z |
| `swab` | Swap byte pairs |
| `sync` | Pad blocks to ibs with NULs |
| `sparse` | Seek past NUL blocks |
| `noerror` | Continue after read errors |
| `notrunc` | Don't truncate output |
| `excl` | Fail if output exists |
| `nocreat` | Fail if output missing |
| `fdatasync` | Sync data before exit |
| `fsync` | Sync data+metadata before exit |

Pipeline order: `ebcdic/ascii/ibm → block/unblock → lcase/ucase → swab → sync`

## I/O Flags (`iflag=` / `oflag=`)

| Flag | `iflag` | `oflag` | Effect |
|------|:---:|:---:|--------|
| `append` | — | ✅ | Append mode |
| `direct` | ✅ | ✅ | Bypass kernel cache (Linux) |
| `directory` | ✅ | ✅ | Fail if not directory |
| `dsync` | ✅ | ✅ | Sync data per write (Linux) |
| `sync` | ✅ | ✅ | Sync data+metadata per write (Linux) |
| `nonblock` | ✅ | ✅ | Non-blocking I/O |
| `noatime` | ✅ | ✅ | Don't update access time (Linux) |
| `nocache` | ✅ | ✅ | Drop cache after I/O (Linux) |
| `noctty` | ✅ | ✅ | No controlling terminal |
| `nofollow` | ✅ | ✅ | Don't follow symlinks |
| `fullblock` | ✅ | — | Accumulate full blocks |

## Size Suffixes

| Suffix | × | Suffix | × | Suffix | × |
|--------|---|--------|---|--------|---|
| `c` | 1 | `K` | 1024 | `kB` | 1000 |
| `w` | 2 | `M` | 1024² | `MB` | 1000² |
| `b` | 512 | `G` | 1024³ | `GB` | 1000³ |
| | | `KiB` | 1024 | `xM` | ×1024² |

`count=512B` = 512 bytes (GNU byte-count). `0xFF` = hex.

## Status Levels

| Level | Output |
|-------|--------|
| `none` | No output except errors |
| `noxfer` | No final stats |
| `progress` | **Indicatif progress bar** (default) — shows ████, speed, ETA |
| `json` | Final stats as JSON |

## Progress Bar

`status=progress` (default) shows a visual progress bar:

- **Bounded** (count specified): `⏳ [00:02] [████████░░░░] 2.1 GB/5.0 GB (1.2 GB/s, 3s)`
- **Unbounded** (no count): `⏳ [00:05] 4.2 GB (980 MB/s)`

## Safety Flags

| Flag | Effect |
|------|--------|
| `--explain`, `-E` | Explain + assess risk, don't execute |
| `--dry-run` | Validate args, don't transfer |
| `--auto-tune` | Auto-select optimal block size |
| `--yes`, `-y` | Skip confirmation prompts |
| `--force` | Skip ALL safety checks (dangerous) |

## Exit Codes

| Code | Meaning |
|:----:|---------|
| 0 | Success |
| 1 | Argument/safety error |
| 2 | I/O error |
| 3 | Conversion error |

## Common Recipes

```bash
# Simple copy
dd-rs if=input of=output bs=1M

# First 10 MiB
dd-rs if=big of=chunk bs=1M count=10

# Clone disk (with safety confirmation)
dd-rs if=/dev/sda of=/dev/sdb bs=4M status=progress

# Rescue failing disk
dd-rs if=/dev/bad of=recovery.img conv=noerror,sync bs=4M

# Sparse file (uses almost no disk space)
dd-rs if=/dev/zero of=sparse.img bs=1M count=10240 conv=sparse

# Byte swap (endianness)
dd-rs if=be.dat of=le.dat conv=swab

# EBCDIC→ASCII (mainframe data)
dd-rs if=ebcdic.dat of=ascii.txt conv=ascii cbs=80

# Random 32-byte key
dd-rs random key.bin --bytes 32

# Write at offset (don't truncate)
dd-rs if=patch of=file seek=1024 conv=notrunc

# Explain before running
dd-rs explain if=/dev/zero of=/dev/sda bs=4M

# Modern syntax
dd-rs copy /dev/zero test.bin --size 1M --count 100
dd-rs zero empty.img --size 1G
dd-rs info /dev/nvme0n1
```

## Install

```bash
cargo install dd-rs
```
