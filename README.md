![Aweyuki Recovery](assets/hero.jpg)

# Aweyuki Recovery

English | [简体中文](README.zh-CN.md) | [Tiếng Việt](README.vi.md)

Aweyuki Recovery is an Android recovery written in Rust, from scratch.

<p align="center">
  <img src="assets/screenshots/01-home.png" width="30%" alt="Aweyuki Recovery Home">
  <img src="assets/screenshots/22-settings-dark.png" width="30%" alt="Settings in dark mode">
  <img src="assets/time-zone.png" width="30%" alt="Time zone">
</p>

## Features

### Dual boot

<p align="center">
  <img src="assets/screenshots/02-dual-boot.png" width="32%" alt="Dual boot selector">
</p>

Run a second system alongside the installed ROM (HyperOS or custom ROM) with atomic `/metadata`-backed switching that rewrites only `boot` and `init_boot`. How a second system is packaged and started is in [How dual boot works](#how-dual-boot-works).

### Decryption & Recovery PIN Lock

<p align="center">
  <img src="assets/screenshots/03-decrypt.png" width="32%" alt="Decrypt data with screen lock">
</p>

Decrypt `/data` using the system's screen-lock PIN, pattern, or password, and optionally protect recovery access itself with a Recovery PIN Lock.

### Installing, OTA Payload Inspector & Batch Flash

<p align="center">
  <img src="assets/screenshots/04-install.png" width="32%" alt="Install packages">
  <img src="assets/screenshots/15-batch-flash.png" width="32%" alt="Batch flash images">
</p>

- **Install sources & tools**: Flash zips, `.img`, `.efi`, and dual-boot packages from Internal Storage, USB OTG, or ADB Sideload with optional zip signature verification. Install scripts get `7za`, `zstd`, streaming `unzip -p`, `avbctl`, and `resetprop`.
- **OTA Payload Inspector & ARB Check**: Inspect `payload.bin` and OTA `.zip` manifests, verify Anti-Rollback (ARB) security patch/timestamp levels, and extract partitions (`REPLACE`, `REPLACE_ZSTD`, `ZERO`, `DISCARD`).
- **Batch Flash & EFISP**: Queue and flash multiple `.img`/`.efi` files with partition geometry validation, including direct non-slotted ARM64 `.efi` flashing to `efisp`.

### Online updates & Wi-Fi

<p align="center">
  <img src="assets/screenshots/05-system-updates.png" width="32%" alt="System updates">
  <img src="assets/screenshots/21-wifi.png" width="32%" alt="Wi-Fi settings">
</p>

Connect to 2.4 GHz / 5 GHz Wi-Fi networks directly in recovery to check and install system OTA updates (with A/B merge status) and self-update Aweyuki Recovery.

### Backup, Restore & App Vault

<p align="center">
  <img src="assets/screenshots/06-backup.png" width="30%" alt="Backup partitions">
  <img src="assets/screenshots/07-restore.png" width="30%" alt="Restore backups">
  <img src="assets/screenshots/16-app-vault.png" width="30%" alt="App Vault">
</p>

- **Backup & Restore**: Back up and restore `boot`, `system`, `data`, Modem & EFS, and optional `efisp` to Internal Storage or USB OTG with compression and verification.
- **App Vault**: Back up and restore individual user apps as `.tar.lz4` archives in `/sdcard/Aweyuki/app-vault`.

### File Manager, Mount & Terminal

<p align="center">
  <img src="assets/screenshots/11-file-manager.png" width="30%" alt="File Manager">
  <img src="assets/screenshots/08-mount.png" width="30%" alt="Mount partitions">
  <img src="assets/screenshots/12-terminal.png" width="30%" alt="Root Terminal">
</p>

- **File Manager**: Browse `/` with file metadata, symlink targets, and a quick shortcut to the terminal.
- **Mount & USB**: Toggle dynamic partitions (`system`, `vendor`, `product`, `odm`, `mi_ext`, `system_ext`, `system_dlkm`) and manage MTP/ADB over USB.
- **Terminal**: Built-in root shell with an on-screen keyboard (`Esc`, `Tab`, `Ctrl`, and arrow keys).

### Wipe Data, SELinux & Filesystem Repair

<p align="center">
  <img src="assets/screenshots/09-wipe.png" width="30%" alt="Wipe Data">
  <img src="assets/screenshots/13-selinux.png" width="30%" alt="Fix SELinux contexts">
  <img src="assets/screenshots/14-filesystem-repair.png" width="30%" alt="Filesystem Repair">
</p>

Every wipe option states clearly what it removes. Fix SELinux contexts on `/data`, check and repair `/data` (`f2fs`), and perform byte-by-byte verified zero-fill wiping for `efisp` and `frp` partitions.

### Root Manager

<p align="center">
  <img src="assets/screenshots/17-root-manager.png" width="32%" alt="Root Manager">
</p>

Automatically resolves kernel version and KMI (e.g. `5.15.194` / `android13-5.15`), detects Magisk, KernelSU, and APatch, and lets you enable, disable, or mark `/data/adb/modules/*` modules for removal.

### HTTP File Sharing & Remote Mirror (`/mirror`)

<p align="center">
  <img src="assets/screenshots/18-network-file-sharing.png" width="32%" alt="HTTP File Sharing">
</p>

Start an HTTP/1.1 file sharing server over Wi-Fi for browser upload/download or `curl -T`, plus `/mirror` for live remote screen mirroring, touch/key control, and one-click PNG capture from a computer browser.

### Runtime Plugins & AERA `.aerap` Apps

<p align="center">
  <img src="assets/screenshots/19-plugins.png" width="32%" alt="Plugins and AERA">
</p>

Run `plugin.toml` shell plugins with live streaming output and cancellation, alongside isolated AERA `.aerap` GUI runtimes including **AERA Browser** (WPE WebKit) and **AERA Telegram** (TDLib).

### Advanced, Developer Tools & Volume-Key-Free Screenshots

<p align="center">
  <img src="assets/screenshots/10-advanced.png" width="32%" alt="Advanced menu">
  <img src="assets/screenshots/20-developer-benchmark.png" width="32%" alt="Developer and Benchmark">
</p>

- **Advanced Hub**: Quick access to Search, Logs (with recent Flash/Wipe history), File Manager, Terminal, SELinux/Filesystem repair, Root Manager, Network File Sharing, Plugins, and App Vault.
- **Developer & Storage Benchmark**: Inspect and adjust `printk` log levels, SELinux mode, USB/ADB state, CPU governor, kernel `cmdline`, and run a 64 MiB sequential storage benchmark.
- **Volume-Key-Free Screenshots**: Capture PNG screenshots either via the **3s Delayed Screenshot** action in Advanced (the toast clears automatically before capture) or remotely from `/mirror`.

### Settings, About Phone & Reboot

<p align="center">
  <img src="assets/screenshots/23-about-specs.png" width="32%" alt="About Phone and Specs">
  <img src="assets/screenshots/24-reboot.png" width="32%" alt="Reboot options">
</p>

- **Localization & Themes**: Light and dark themes; 15 languages (简体中文, 繁體中文, English, Türkçe, Русский, Українська, Español, Português (Brasil), Bahasa Indonesia, Tiếng Việt, Deutsch, Français, Italiano, Polski, and 日本語); IANA time zones including Vietnam (`Asia/Ho_Chi_Minh`, UTC+7).
- **About & Diagnostics**: View Aweyuki version, storage, SoC, RAM, battery health, and charge cycles.
- **Reboot & Slot Control**: Reboot to System, Recovery, Fastboot, Bootloader, EDL, or Power off, switch A/B slots, and rely on the pre-reboot ABL EFISP regression guard.

## Supported devices

| Device | Codename | Tested system version |
| --- | --- | --- |
| Xiaomi 17 Ultra | `nezha` | OS4.0.4.0.XPACNXM (Android 17) |
| Xiaomi 13 Ultra | `ishtar` | OS3.0.308.0.WMACNXM (Android 16) |
| REDMI K60 Pro | `socrates` | OS4.0.0.24.XMKCNXM (Android 17) |
| Xiaomi 13 Pro | `nuwa` | OS3.0.312.0.WMBCNXM (Android 16) |

### 🚧 Under construction

These images are built from each phone's stock firmware and have not been tried
on the phone. They set up the screen, touch, Wi-Fi and vibration. The programs that
decrypting the data partition takes are read on the phone from the installed
system's own init scripts and service descriptions; what is read has been checked
against each model's stock firmware, and how those programs run on these models has
not been verified.

| Device | Codename | Built on |
| --- | --- | --- |
| 🚧 Xiaomi 13 | `fuxi` | OS3.0.309.0.WMCCNXM (Android 16) |
| 🚧 Xiaomi 14 | `houji` | OS3.0.307.0.WNCCNXM (Android 16) |
| 🚧 Xiaomi 14 Pro | `shennong` | OS3.0.309.0.WNBCNXM (Android 16) |
| 🚧 Xiaomi 14 Ultra | `aurora` | OS3.0.308.0.WNACNXM (Android 16) |
| 🚧 Xiaomi 15 | `dada` | OS3.0.307.0.WOCCNXM (Android 16) |
| 🚧 Xiaomi 15 Pro | `haotian` | OS3.0.309.0.WOBCNXM (Android 16) |
| 🚧 Xiaomi 15 Ultra | `xuanyuan` | OS3.0.314.0.WOACNXM (Android 16) |
| 🚧 Xiaomi 17 | `pudding` | OS4.0.4.0.XPCCNXM (Android 17) |
| 🚧 Xiaomi 17 Pro | `pandora` | OS4.0.4.0.XBLCNXM (Android 17) |
| 🚧 Xiaomi 17 Pro Max | `popsicle` | OS4.0.4.0.XPBCNXM (Android 17) |
| 🚧 Xiaomi 17 Max | `byron` | OS3.0.310.0.WAFCNXM (Android 16) |
| 🚧 REDMI K80 | `zorn` | OS3.0.309.0.WOKCNXM (Android 16) |
| 🚧 REDMI K80 Pro | `miro` | OS3.0.305.0.WOMCNXM (Android 16) |
| 🚧 REDMI K90 | `annibale` | OS3.0.307.0.WPKCNXM (Android 16) |
| 🚧 REDMI K90 Pro Max | `myron` | OS4.0.4.0.XPMCNXM (Android 17) |
| 🚧 REDMI K90 Ultra | `warsaw` | OS3.0.309.0.WHPCNXM (Android 16) |
| 🚧 REDMI K100 Pro | `athens` | OS4.0.4.0.XPICNXM (Android 17) |
| 🚧 REDMI K100 Pro Max | `songyuan` | OS4.0.4.0.XGNCNXM (Android 17) |

### Adding a new device

GitHub Actions automatically builds and packages every `devices/*.toml` on each run and release. To add a phone not listed above:

- Create `devices/<codename>.toml` (display, touch, kernel modules, partitions, Wi-Fi, haptics, backup, logs, and the stock full OTA `[stock]` metadata).
- Extract stock init/VINTF descriptors with `aweyuki-pack described` into `crates/aweyuki-programs/tests/described/<codename>`.
- Register the expected `[stock].version` in `crates/aweyuki-config/src/lib.rs` and run `cargo test --workspace`.
- Add the device to the `🚧 Under construction` table (or the main table once verified on real hardware).

See the full walkthrough in [Adding a new device](docs/adding-devices.md) ([Tiếng Việt](docs/adding-devices.vi.md)).

## Installing Aweyuki Recovery

Aweyuki needs an unlocked bootloader. Download the zip for your phone from
[Releases](https://github.com/AweYuki/aweyuki_recovery/releases), where each shows as the
phone's model and codename, such as Xiaomi 17 Ultra (nezha). Check it against
`SHA256SUMS`, unzip it, and flash the image inside to the recovery partition of
the slot the phone runs from, with the phone in fastboot mode:

```sh
unzip aweyuki-v0.5.1-nezha-OS4.0.4.0.XPACNXM.zip
fastboot flash recovery aweyuki-v0.5.1-nezha-OS4.0.4.0.XPACNXM.img
fastboot reboot recovery
```

Afterwards Aweyuki updates itself over Wi-Fi from its newest release, on the
Aweyuki update page in Settings; a newer image, unzipped, also installs from its own
Install page. With **Flash Aweyuki again** on, a system update or zip that writes
over the recovery partitions gets Aweyuki back in them.

adb in Aweyuki is a root shell that needs no authorization on the phone: anyone
with the phone and a cable can reach its storage while it is in recovery.

## How dual boot works

Aweyuki installs a second system beside the one in the phone's own partitions,
whether that is HyperOS or a custom ROM, and switches between them from **Dual boot**
on its home page. The second system's root is a file in the data partition that the
filesystem may neither move nor delete, read and written through the partition
itself; the boot images of both systems are kept in another such file. A switch
writes only the boot and init_boot partitions of the slot that starts next, and
keeps each step on /metadata first, so a switch cut off by a loss of power is
finished when Aweyuki next starts. vendor_boot, dtbo, recovery, vbmeta, the firmware
and super stay as they are, and the second system starts with the installed
system's vendor_boot and dtbo.

Before anything else writes the boot partitions, Aweyuki puts the installed system's
boot images back: an install, a slot switch, fastboot, a factory reset, and a backup
or restore of the boot partitions. The second system waits while a system update is
still to be merged, and backups of the data partition leave it out.

A second system installs from a zip on the Install page, as any other package does:

| File | What it holds |
| --- | --- |
| `system.toml` | `id`, `name`, `version` and `filesystem`; `init`, the program the root starts with, `/sbin/init` unless given; `minimum`, the least room the root takes, in bytes |
| `boot.img` | The kernel the bootloader starts |
| `rootfs.img` | The root filesystem's image, of the `filesystem` named |
| `init_boot.img` | Only for a system with a first ramdisk of its own |

A system without an init_boot image of its own starts from one Aweyuki makes. Its
program loads the installed system's first-stage kernel modules the way that
system's first stage loads them, with their soft dependencies and the settings the
kernel command line gives them, finds the root in the data partition, mounts it and
starts `init` there. If anything stops it, it brings Aweyuki up, as a failed start
of the installed system ends in the recovery: it leaves the reason in misc, unpacks
the ramdisk of the slot's recovery partition over its own, as the kernel unpacks it
when the bootloader starts the recovery, and hands over to Aweyuki's first program,
so Aweyuki comes up however the bootloader treats a restart. Aweyuki shows why, and
the second system still starts next until the user switches on Dual boot. A start
that stops moving restarts into the recovery instead, and a phone restarted before
the hand-over finds the same request in misc at its next start, which brings
Aweyuki up before anything else. Where Aweyuki comes up neither way, the phone stays
where it stopped, and never starts the same again and again. A system with a first
ramdisk of its own finds Aweyuki's program at `/aweyuki/map`, which lays the root
out as a block device and prints its node. Each switch to the second system writes
its init_boot image with the program of the Aweyuki that switches, so an updated
Aweyuki starts it with the new program.

A running system switches by writing `boot-recovery` with
`--switch_system=installed`, or `--switch_system=` and the second system's `id`, to
misc, and restarting into recovery. Where the bootloader starts the slot again, as
the 13 Ultra's can with a USB cable in, Aweyuki's program finds the request in misc
at the next start and brings Aweyuki up before anything else.

## Building

Aweyuki builds with the version of Rust that `rust-toolchain.toml` names, with its
`aarch64-linux-android` target, the Android NDK r29 and
[cargo-ndk](https://github.com/bbqsrc/cargo-ndk). The recovery service is built for
the phone, and the packer for the computer:

```sh
cargo ndk -t arm64-v8a --platform 30 build --release -p aweyuki
cargo build --release -p aweyuki-pack
```

The second system's first program runs before any library is there to load, so it
is built as a static program:

```sh
RUSTFLAGS="-C target-feature=+crt-static" cargo ndk -t arm64-v8a --platform 30 build --release -p aweyuki-system-init --target-dir target/system-init
```

The packer takes the stock recovery partition out of the build's full update
package on Xiaomi's server, reading only the part of the package the partition is
in, and checks its SHA-256 against the device file:

```sh
target/release/aweyuki-pack stock --config devices/nezha.toml --out target/stock/nezha/recovery.img
```

Then it builds the image on top of it:

```sh
target/release/aweyuki-pack recovery \
  --stock target/stock/nezha/recovery.img \
  --binary target/aarch64-linux-android/release/aweyuki \
  --system-init target/system-init/aarch64-linux-android/release/system-init \
  --config devices/nezha.toml \
  --font resources/fonts/MiSansVF.ttf \
  --mono resources/fonts/KirkMono-Regular.ttf \
  --mono-license resources/fonts/KirkMono-OFL.txt \
  --out target/aweyuki-nezha.img
```

Tests and lints run on the computer; what runs only on the phone is linted for it:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo ndk -t arm64-v8a --platform 30 clippy --workspace --all-targets -- -D warnings
```

The tests whose names start with `on_the_phone` make filesystems, mount them and
set encryption keys, which only a phone does. With a phone in Aweyuki attached over
adb, its data partition open, `cargo ndk` pushes each test program to the phone and
runs it there, one test at a time:

```sh
cargo ndk -t arm64-v8a --platform 30 test -p aweyuki -p aweyuki-block -p aweyuki-files -p aweyuki-mtp on_the_phone -- --test-threads=1
```

`aweyuki-preview` draws the pages on the computer, as PNG files, without a phone:
each page whole, as a long screenshot shows it, in the states it can be in, and the
screen at moments of touches and motion. `--lang` picks the language by a locale
such as `en` or `zh-TW`:

```sh
cargo run --release -p aweyuki-preview -- target/preview --lang en
```

### Translations

Each language's texts are one file in `crates/aweyuki-ui/locales`, named by its
locale: `tr.toml`, `pt-BR.toml`. `en.toml` is the reference, with a comment on
where each text shows. The build reads every file and stops on a text that is
missing, or that names a value in braces the English text does not.

To correct a translation, change its text in the language's file. To check how it
looks, draw the pages in that language with `aweyuki-preview --lang`. With each
page, it says which texts did not fit their place: `cut short in` for a text that
ends in an ellipsis, `set at 80% in` for a label set smaller to fit its key, button
or slider, `past the edge in` for a text that runs off the screen, and `overlap in`
for two texts that run into each other.

## Credits

- [Skyward](https://github.com/kirklin/skyward) by Kirk Lin — original Rust recovery architecture and UI foundation.
- [AERA Recovery](https://github.com/AERA-Recovery) — inspiration and design reference for EFISP, Root Manager, network file sharing, and runtime plugins.

## License

[GPL-3.0-or-later](LICENSE)
