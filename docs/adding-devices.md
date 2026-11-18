English | [Tiếng Việt](adding-devices.vi.md)

# Adding Support for a New Device

This guide explains step by step how to port Aweyuki Recovery to another device: checking hardware and partition requirements, extracting stock firmware descriptors, writing `devices/<codename>.toml`, registering and running offline tests, bringing the recovery up on the phone over `adb`, and adding the device to the supported list and CI releases.

---

## Prerequisites & Compatibility

Before starting a device port, verify the target device meets these requirements:

1. **Architecture & platform**:
   - 64-bit `arm64-v8a` (`aarch64-linux-android`), Android API level 30+ (Android 11 or newer).
   - Dynamic partitions (`super` read via `aweyuki-lp` / `aweyuki-vab`). The automatic key-service discovery in `crates/aweyuki-programs` and current device configs target **Xiaomi / REDMI** phones on **Qualcomm** platforms (`qseecomd`, KeyMint, Gatekeeper, Weaver). Other platforms can be supported, though service discovery may need extension if init/HAL wiring differs.
2. **Partition layout & stock recovery image**:
   - A dedicated **`recovery` partition** ending with an AVB footer (`AVBf` in the final 64 bytes — enforced by `aweyuki-pack`).
   - Built on top of the **stock recovery** image (`aweyuki-pack` keeps the stock `init`, `adbd`, SELinux policy, and shared libraries while replacing `/system/bin/recovery`; custom recoveries containing `twres` or `FFiles` are rejected).
   - **Dual boot** requires separate `boot`, `init_boot`, and `recovery` partitions on both `_a` and `_b` slots.
3. **Display**: Linux DRM/KMS (`/dev/dri/card0`).

---

## 5-Step Workflow Overview

1. Obtain the stock full OTA `.zip` URL and extract system service descriptors with `aweyuki-pack`.
2. Author `devices/<codename>.toml` (display, touch, kernel modules, partitions, Wi-Fi, haptics, backup, and log paths).
3. Register the stock build in `crates/aweyuki-config/src/lib.rs` and run offline workspace tests and lints.
4. Pack the recovery image, flash via `fastboot`, and debug on-device using `aweyuki` CLI subcommands over `adb`.
5. Add the device to the device tables in `README*.md`; CI/Releases automatically pick up every `devices/*.toml`.

---

## Step 1: Prepare the Stock Build & Extract Firmware Descriptors

First, build the host packer tool:

```sh
cargo build --release -p aweyuki-pack
```

### 1.1. Populate `[stock]`

Aweyuki packs on top of the stock `recovery` partition inside an official full OTA package (a `.zip` containing `payload.bin`). You need:

- `version`: The stock build version string (e.g. `"OS4.0.4.0.XPACNXM"`).
- `package`: Direct **`https://`** URL to the full OTA `.zip` (the `.zip` filename must contain `version`).
- `recovery`: Lowercase **64-character hex SHA-256** digest of the `recovery` partition inside that package.

**Tip to get the SHA-256 without downloading the entire multi-GB OTA zip:**

Put 64 zeros as a placeholder in `[stock].recovery` in `devices/<codename>.toml`:

```toml
[stock]
version = "OS4.0.4.0.XPACNXM"
package = "https://.../nezha-ota_full-OS4.0.4.0.XPACNXM-user-17.0-4cf5faf3ac.zip"
recovery = "0000000000000000000000000000000000000000000000000000000000000000"
```

Then run:

```sh
target/release/aweyuki-pack stock \
  --config devices/<codename>.toml \
  --out target/stock/<codename>/recovery.img
```

`aweyuki-pack` fetches only the byte ranges of the `recovery` partition via HTTP Range requests, prints the mismatch error containing the **actual SHA-256**, and exits. Copy that lowercase SHA-256 back into `[stock].recovery` and re-run the command to verify and keep `recovery.img`.

*(If you already have an extracted `recovery.img` with its AVB footer intact, you can compute it directly with `sha256sum recovery.img`.)*

### 1.2. Extract init scripts, VINTF manifests, and accounts

Extract the service descriptions, init `.rc` scripts, `passwd`/`group` files, and `build.prop` from `vendor`, `odm`, and `system_ext` into the test fixture directory:

```sh
target/release/aweyuki-pack described \
  --config devices/<codename>.toml \
  --out crates/aweyuki-programs/tests/described/<codename>
```

Like `stock`, `described` uses HTTP Range requests to download only the relevant files from the OTA payload (pass `--partition <name>` to override the default `vendor`, `odm`, `system_ext` list).

---

## Step 2: Write `devices/<codename>.toml`

The filename must match `device.codename` (`devices/<codename>.toml`), which both `.github/workflows/build.yml` and `aweyuki-pack` use to locate the config.

Two working reference configs show the main hardware patterns:
- [devices/nezha.toml](../devices/nezha.toml) (Xiaomi 17 Ultra — in-controller touch reporting via sysfs, `qca_cld3_peach_v2` Wi-Fi, extra shared libraries for `logd`).
- [devices/ishtar.toml](../devices/ishtar.toml) (Xiaomi 13 Ultra — userspace raw-frame touch daemons with `[[owners]]` and `[[service]]`, driver waveform `click = 0`, `qca_cld3_kiwi_v2` Wi-Fi).

Below is the schema and validation enforced by `crates/aweyuki-config/src/lib.rs`:

### `[device]` — Identity

```toml
[device]
codename = "nezha"
brand = "Xiaomi"
model = "Xiaomi 17 Ultra"
ab = true
soc = { hans = "第五代骁龙®8至尊版移动平台", hant = "Snapdragon® 8 Elite Gen 5 行動平台", en = "Snapdragon® 8 Elite Gen 5 Mobile Platform" }
os_version_prop = "ro.mi.os.version.incremental"
```

- `codename` *(required)*: Device codename matching the filename.
- `brand`, `model`: Brand and marketing model (`model` is used in GitHub Release asset labels as `<model> (<codename>)`).
- `ab`: Defaults to `true`.
- `soc`: Optional localized SoC name shown on the About page (`{ hans, hant, en }`).
- `os_version_prop`: Property carrying the vendor OS version (defaults to `"ro.build.display.id"`; Xiaomi HyperOS uses `"ro.mi.os.version.incremental"`).

### `[display]` — Panel Geometry

```toml
[display]
width = 1200
height = 2608
density = 480
diagonal = 6.9
corner_radius = 170
```

- `width`, `height`, `density`: Native panel width, height, and DPI (check `persist.sys.miui_resolution` in `odm/etc/build.prop` from Step 1.2 or run `aweyuki kms` on the phone).
- `diagonal`: Optional marketed screen diagonal in inches.
- `corner_radius`: Screen corner radius in pixels so page slide transitions match hardware rounded corners (`rounded_corner_radius` in the system overlay).

### `[modules]` — Startup Kernel Modules

```toml
[modules]
sources = ["vendor_boot", "vendor_dlkm", "vendor", "system_dlkm"]
load = ["synaptics_tcm2"]
```

- `sources`: Partitions searched in order for `.ko` modules and `modules.dep` (defaults to `["vendor_boot", "vendor_dlkm", "vendor"]`; include `"system_dlkm"` if drivers depend on GKI modules such as `rfkill`).
- `load`: Module names without `.ko` (typically the touchscreen driver, e.g. `synaptics_tcm2`, `fts_touch_spi`, `goodix_core`, `focaltech_touch_3685g`). Dependencies in `modules.dep` load automatically first.

### Touchscreen: `[touch]` vs. `[[owners]]` + `[[service]]`

Check the extracted `.rc` scripts for how the stock OS drives touch:

- **Pattern A — Controller reports coordinates directly** (common on newer HyperOS phones with Synaptics/Goodix/FocalTech):
  ```toml
  [touch]
  writes = [{ path = "/sys/class/touch/touch_dev/enable_touch_raw", value = "0" }]
  ```
  Aweyuki waits for the touchscreen input device to appear, then writes each sysfs switch.
- **Pattern B — Controller streams raw frames to a userspace touch daemon** (e.g. Xiaomi 13 / 13 Pro / 13 Ultra):
  Grant device node/sysfs ownership via `[[owners]]` (since recovery does not run normal init boot actions) and run the touch services under `[[service]]`:
  ```toml
  [[owners]]
  user = "system"
  group = "system"
  mode = 0o664 # octal <= 0o7777
  paths = [
      "/dev/xiaomi-touch",
      "/proc/tp_hal_version",
      "/sys/class/touch/touch_dev/enable_touch_raw",
      # ... nodes required by touch_report / touchsensor
  ]

  [[service]]
  path = "/odm/bin/touch_report"
  user = "system"
  groups = ["system", "uhid", "input"]
  capabilities = ["SYS_NICE"]
  ```

### `[firmware]` & `[partitions]` — Extra Mounts

```toml
[firmware]
partition = "modem"

[partitions]
mounted = ["persist"]
```

- `[firmware].partition`: Plain partition name (no `/`, no `_a`/`_b` slot suffix) mounted so drivers and TEE apps find firmware images when stock recovery leaves `/vendor/firmware_mnt` unmounted.
- `[partitions].mounted`: Partitions from the device fstab to mount at their normal paths before starting services (e.g. `"persist"` for `qseecomd` anti-rollback storage and Wi-Fi MAC data).

### `[logd]` — Missing Shared Libraries for `logd`

```toml
[logd]
libraries = [
    "/mnt/system/system/lib64/libsysutils.so",
    "/mnt/system/system/lib64/libcap.so",
]
```

Add this only if the stock recovery ramdisk omits libraries dynamically linked by `logd`, causing `logd` to crash-loop. Each path must be absolute and point to a file.

### `[[service]]` — Device Programs & Automatic Key-Service Discovery

**Important:** Do **NOT** manually list `qseecomd`, `keymint`, `gatekeeper`, `weaver`, or `secure_element` in `[[service]]`. `aweyuki-programs` dynamically inspects the installed system's `.rc` scripts and VINTF manifests at boot time and starts the exact dependency chain for the installed OS.

Use `[[service]]` only for:
1. **CPU post-boot tuning script** (recommended on Qualcomm devices so CPUs leave the startup `performance` governor and enter idle states instead of overheating in recovery):
   ```toml
   [[service]]
   path = "/vendor/bin/sh"
   args = ["/vendor/bin/init.kernel.post_boot.sh"]
   libraries = ["/system/lib64", "/vendor/lib64"]
   oneshot = true
   user = "root"
   groups = ["root", "system", "wakelock", "graphics"]
   ```
2. **Non-key daemons** such as touch frame processors or hardware-SKU-conditional helpers:
   - `when = { property = "...", value = "..." }` restricts execution to matching build properties.
   - Validation rules enforced by `DeviceConfig::parse`:
     - When `oneshot = true`, `ready_property`, `ready_service`, `ready_hardware_service`, and `registers` must all be unset/empty.
     - Setting `user` requires a non-empty `groups` list.
     - `capabilities` use kernel names **without** the `CAP_` prefix (e.g. `"SYS_NICE"`, `"WAKE_ALARM"`).

### `[wlan]` & `[update]` — Wi-Fi & Stock Update Check

```toml
[wlan]
modules = ["rfkill", "qca_cld3_peach_v2"]
writes = [{ path = "/sys/kernel/cnss/fs_ready", value = "1" }]
# Optional firmware loader mappings if stock recovery obscures /vendor/firmware links:
# firmware = [
#     { name = "wlan/qca_cld/kiwi_v2/WCNSS_qcom_cfg.ini", path = "/vendor/etc/wifi/kiwi_v2/WCNSS_qcom_cfg.ini" },
#     { name = "wlan/qca_cld/kiwi_v2/wlan_mac.bin", partition = "persist", path = "wlan/wlan_mac.bin" },
# ]
interface = "wlan0"
supplicant = "/vendor/bin/hw/wpa_supplicant"
libraries = ["/system/lib64", "/vendor/lib64"]
service = "android.hardware.wifi.supplicant.ISupplicant/default"
probe = "http://connect.rom.miui.com/generate_204"

[update]
server = "https://update.miui.com/updates/miotaV3.php"
```

- Check `modules.load`, `modules.blocklist`, and device-tree/dtbo nodes to determine the exact Wi-Fi module name (e.g. `qca_cld3_peach_v2` vs. `qca_cld3_kiwi_v2`).
- In `firmware`, `name` is the relative path requested by the kernel firmware loader; `path` is either relative from `partition` (when `partition` is set) or an absolute path on an already-mounted filesystem (when `partition` is omitted).
- `probe` **must** use plain `http://` (not `https://`) so captive-portal logins are detected accurately.

### `[haptics]` — Vibration Motor

```toml
[haptics]
# click = 0 # Optional driver waveform ID; omit to synthesize from resonance
resonance = "/sys/class/qcom-haptics/lra_frequency_hz"
strength = { lightest = 0.25, strongest = 0.75 }
```

- `resonance`: Absolute sysfs path reporting resonant frequency.
- `strength`: Must satisfy **`0.0 < lightest < strongest <= 1.0`**.

### `[backup]` & `[wipe]` — EFS, Excluded Data, & Vendor Logs

```toml
[backup]
efs = ["modemst1", "modemst2", "fsg", "fsc", "persist"]
data_skipped = [
    "vendor/tzstorage",
    "vendor/qtee",
    "vendor/mitee",
    "vendor/goodix",
    "vendor/fingerprint",
    "vendor/fpdata",
    "vendor/misc/mi_fp",
]

[wipe]
logs = [
    "/data/miuilog",
    "/data/vendor/diag",
    "/data/vendor/bsplog",
    "/data/vendor/log",
    "/data/vendor/ramdump",
    "/data/vendor/ramdump_ssr",
    "/data/vendor/ssrdump",
    "/data/vendor/tombstones",
    "/data/vendor/wlan_logs",
]
```

Mind the path format difference enforced by validation:
- `[backup].efs`: Plain partition names (no `/`).
- `[backup].data_skipped`: Paths **relative to `/data`** (NO leading `/`, no `..`; single-segment `*` wildcards like `"vendor_de/*/fpdata"` are allowed) for hardware-bound TEE/fingerprint state that breaks if restored from an older backup.
- `[wipe].logs`: **Absolute** directory paths inside `/data/` (not `/data` itself, no `..`).

---

## Step 3: Register Tests & Validate Offline

1. Add your new device and expected `[stock].version` to `each_phone_names_the_stock_build_its_recovery_is_made_on` in [crates/aweyuki-config/src/lib.rs](../crates/aweyuki-config/src/lib.rs):

   ```rust
   (include_str!("../../../devices/<codename>.toml"), "<STOCK_VERSION>"),
   ```

2. Ensure `crates/aweyuki-programs/tests/described/<codename>/` exists from Step 1.2. The test `every_phone_names_one_program_for_each_service` in [crates/aweyuki-programs/tests/phones.rs](../crates/aweyuki-programs/tests/phones.rs) automatically discovers every subdirectory there and verifies KeyMint/Gatekeeper/Weaver/VINTF resolution across every hardware SKU manifest. *(Note: `phones.rs` currently passes `ro.boot.hardware = "qcom"`; adjust if adding a non-Qualcomm SoC.)*

3. Run workspace tests, clippy (host + Android), and fork invariants (requires the Rust toolchain from `rust-toolchain.toml`, Android NDK r29, and `cargo-ndk`):

   ```sh
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo ndk -t arm64-v8a --platform 30 clippy --workspace --all-targets -- -D warnings
   bash .claude/skills/upstream-sync/scripts/check-invariants.sh
   ```

---

## Step 4: Pack, Flash, and Debug on Device via `adb`

### 4.1. Build & pack the recovery image

```sh
cargo ndk -t arm64-v8a --platform 30 build --release -p aweyuki
RUSTFLAGS="-C target-feature=+crt-static" cargo ndk -t arm64-v8a --platform 30 build --release -p aweyuki-system-init --target-dir target/system-init

target/release/aweyuki-pack recovery \
  --stock target/stock/<codename>/recovery.img \
  --binary target/aarch64-linux-android/release/aweyuki \
  --system-init target/system-init/aarch64-linux-android/release/system-init \
  --config devices/<codename>.toml \
  --font resources/fonts/MiSansVF.ttf \
  --mono resources/fonts/KirkMono-Regular.ttf \
  --mono-license resources/fonts/KirkMono-OFL.txt \
  --out target/aweyuki-<codename>.img
```

Flash in fastboot mode (unlocked bootloader required):

```sh
fastboot flash recovery target/aweyuki-<codename>.img
fastboot reboot recovery
```

### 4.2. Built-in `aweyuki` CLI diagnostic commands

Because `aweyuki-pack` sets `ro.secure=0` and `ro.adb.secure=0` in `prop.default`, `adb shell` gives you an immediate root shell even if the screen or touchscreen has not come up yet:

| Check | Command in `adb shell` |
| --- | --- |
| Startup logs & crashes | `dmesg \| grep -i aweyuki` and `logcat -d` |
| Device properties & `About` readout | `aweyuki info --config /aweyuki/device.toml` |
| Current slot & `super` partitions | `aweyuki slot` and `aweyuki lp` |
| Mount / unmount `super` partitions | `aweyuki mount` / `aweyuki umount` |
| DRM/KMS display connectors & capture | `aweyuki kms` and `aweyuki capture /tmp/shots` |
| Kernel module load order & loading | `aweyuki modules --source /vendor_dlkm/lib/modules` and `aweyuki load` |
| Touchscreen input events | `getevent -lt` |
| Detected key/decryption service chain | `aweyuki keys` |
| Service registration, KeyMint, Gatekeeper, Weaver | `aweyuki service <name>`, `aweyuki keymint`, `aweyuki gatekeeper`, `aweyuki weaver` |
| Test `/data` decryption from CLI | `aweyuki data --credential "<pin_or_password>"` |
| Tune `[haptics]` `lightest` / `strongest` | `aweyuki haptic --position 0.5` and `aweyuki haptic --ticks` |

Once `/data` decrypts and mounts, run the on-device hardware test suite from your host over `adb`:

```sh
cargo ndk -t arm64-v8a --platform 30 test -p aweyuki -p aweyuki-block -p aweyuki-files -p aweyuki-mtp on_the_phone -- --test-threads=1
```

---

## Step 5: Add to the Supported List & CI/Release

### 5.1. Automatic CI & Releases

[.github/workflows/build.yml](../.github/workflows/build.yml) automatically builds every `devices/*.toml` without editing the workflow file:
- The `devices` job scans `devices/*.toml`, extracts & caches each stock `recovery.img` verified by `[stock].recovery`, packs the zip, and uploads it.
- Tagging a release matching `Cargo.toml`'s version (e.g. `v0.4.0`) attaches `<model> (<codename>)` zips for every device in `devices/` along with `SHA256SUMS`.
- To test-build only your new device in GitHub Actions before submitting, trigger **`workflow_dispatch`** with the `device` input set to `<codename>`.

### 5.2. Update the device tables in `README*.md`

Update the tables in [README.md](../README.md), [README.vi.md](../README.vi.md), and [README.zh-CN.md](../README.zh-CN.md):

- **List under `### 🚧 Under construction` (`Đang hoàn thiện` / `施工中`)** if the config is built from stock firmware, `tests/described/<codename>` is committed, and workspace tests pass, but real-device verification is not yet complete.
- **Promote to `## Supported devices` (`Thiết bị hỗ trợ` / `支持的机型`)** once verified end-to-end on real hardware (display, touch, Wi-Fi, haptics, `/data` decryption, installs, backup/restore, and slot switching / dual boot where applicable).
