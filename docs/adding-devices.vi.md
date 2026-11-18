[English](adding-devices.md) | Tiếng Việt

# Hướng dẫn thêm hỗ trợ thiết bị mới vào Aweyuki Recovery

Tài liệu này hướng dẫn từng bước cách đưa một thiết bị Android mới vào Aweyuki Recovery: từ việc kiểm tra điều kiện phần cứng, lấy tệp mô tả từ firmware gốc, viết tệp cấu hình `devices/<codename>.toml`, chạy kiểm thử offline, gỡ lỗi trên máy thật cho đến việc đăng ký vào danh sách thiết bị hỗ trợ và luồng build/release tự động.

---

## Tổng quan & Yêu cầu tương thích

Trước khi thêm thiết bị mới, hãy kiểm tra các điều kiện sau:

1. **Kiến trúc & nền tảng**:
   - Thiết bị sử dụng CPU 64-bit `arm64-v8a` (`aarch64-linux-android`), API level >= 30 (Android 11 trở lên).
   - Hiện tại hệ thống tự động giải mã trong `crates/aweyuki-programs` và các ví dụ cấu hình tập trung vào dòng **Xiaomi / REDMI** chạy **Qualcomm** (phân vùng động A/B, `super`, `qseecomd`, `keymint`/`gatekeeper`/`weaver`). Có thể thêm nền tảng khác, nhưng cần mở rộng thêm phần phát hiện dịch vụ nếu cấu trúc init/HAL khác biệt.
2. **Bố cục phân vùng & Recovery gốc**:
   - Có phân vùng **`recovery` riêng biệt** kết thúc bằng AVB footer (`AVBf` ở 64 byte cuối — `aweyuki-pack` kiểm tra trực tiếp chân chữ ký này).
   - Ảnh gốc phải là **stock recovery** (công cụ đóng gói giữ nguyên `init`, `adbd`, SELinux policy và thư viện gốc, chỉ thay `/system/bin/recovery` bằng Aweyuki; không build đè lên custom recovery như TWRP/OrangeFox chứa thư mục `twres` hoặc `FFiles`).
   - Để hỗ trợ tính năng **Khởi động kép (Dual boot)**, thiết bị cần có phân vùng `boot`, `init_boot` và `recovery` tách riêng ở cả hai khe `_a`/`_b`.
3. **Hiển thị**: Màn hình hoạt động qua Linux **DRM/KMS** (`/dev/dri/card0`).

---

## Tóm tắt quy trình 5 bước

1. Chuẩn bị link ROM gốc (Full OTA `.zip`) và trích xuất mô tả dịch vụ bằng `aweyuki-pack`.
2. Tạo tệp `devices/<codename>.toml` khai báo màn hình, cảm ứng, phân vùng, Wi-Fi, rung, sao lưu và dọn log.
3. Đăng ký thiết bị vào bộ unit test (`crates/aweyuki-config/src/lib.rs`) và kiểm tra chuỗi dịch vụ giải mã (`crates/aweyuki-programs/tests/phones.rs`).
4. Build thử ảnh recovery, flash vào máy và chẩn đoán trực tiếp từng thành phần qua `adb shell`.
5. Thêm máy vào bảng thiết bị trong `README*.md` và đẩy lên GitHub để CI đóng gói tự động.

---

## Bước 1: Chuẩn bị gói ROM gốc & Trích xuất mô tả hệ thống

Trước hết, build công cụ đóng gói `aweyuki-pack` trên máy tính:

```sh
cargo build --release -p aweyuki-pack
```

### 1.1. Lấy thông tin mục `[stock]` (ROM gốc làm nền build)

Aweyuki build recovery dựa trên `recovery.img` trong gói Full OTA chính thức (`.zip` chứa `payload.bin`). Bạn cần:

- `version`: Chuỗi phiên bản bản dựng (ví dụ: `"OS4.0.4.0.XPACNXM"`).
- `package`: Đường dẫn **`https://`** trực tiếp tới tệp `.zip` Full OTA (tên tệp `.zip` bắt buộc có chứa chuỗi `version`, ví dụ `...-OS4.0.4.0.XPACNXM-...zip`).
- `recovery`: Mã băm **SHA-256** (đúng **64 ký tự hex chữ thường**) của phân vùng `recovery` bên trong gói OTA đó.

**Mẹo lấy mã SHA-256 mà không cần tải cả gói OTA nhiều GB:**

Trong `devices/<codename>.toml` ban đầu, tạm điền 64 số `0` vào trường `recovery`:

```toml
[stock]
version = "OS4.0.4.0.XPACNXM"
package = "https://.../nezha-ota_full-OS4.0.4.0.XPACNXM-user-17.0-4cf5faf3ac.zip"
recovery = "0000000000000000000000000000000000000000000000000000000000000000"
```

Sau đó chạy:

```sh
target/release/aweyuki-pack stock \
  --config devices/<codename>.toml \
  --out target/stock/<codename>/recovery.img
```

`aweyuki-pack` chỉ tải đúng phần dữ liệu của phân vùng `recovery` bằng HTTP Range Request, rồi báo lỗi đối chiếu SHA-256 kèm **mã SHA-256 thực tế** của ảnh gốc. Bạn chỉ cần chép chuỗi SHA-256 đó dán lại vào `[stock].recovery` rồi chạy lại lệnh trên để xác nhận tải và kiểm tra hợp lệ.

*(Nếu bạn đã giải nén sẵn `recovery.img` gốc từ máy hoặc từ `payload.bin`, có thể lấy hash bằng `sha256sum recovery.img`.)*

### 1.2. Trích xuất init scripts, VINTF và tài khoản cho bộ test giải mã

Để bộ test tự động kiểm chứng việc giải mã `/data` (FBEv2, KeyMint, Gatekeeper, Weaver, TEE) và cho phép cộng đồng kiểm tra offline mà không cần tải cả ROM, hãy trích xuất các tệp mô tả từ `vendor`, `odm`, `system_ext`:

```sh
target/release/aweyuki-pack described \
  --config devices/<codename>.toml \
  --out crates/aweyuki-programs/tests/described/<codename>
```

Lệnh này cũng dùng HTTP Range để tải đúng các tệp `.rc`, VINTF XML, `passwd`, `group`, `build.prop` từ phân vùng hệ thống của gói OTA (và có thể thêm `--partition <tên>` nếu cần chỉ định phân vùng khác).

---

## Bước 2: Viết tệp cấu hình `devices/<codename>.toml`

Tên tệp bắt buộc đặt theo đúng `codename` của máy (`devices/<codename>.toml`), vì `.github/workflows/build.yml` và `aweyuki-pack` ánh xạ trực tiếp từ tên tệp này.

Bạn có thể tham khảo hai mẫu chuẩn đang chạy thực tế:
- [devices/nezha.toml](../devices/nezha.toml) (Xiaomi 17 Ultra — cảm ứng tự xử lý tọa độ qua sysfs, Wi-Fi `qca_cld3_peach_v2`, thêm thư viện cho `logd`).
- [devices/ishtar.toml](../devices/ishtar.toml) (Xiaomi 13 Ultra — cảm ứng chạy daemon userspace `touch_report`/`touchsensor`, `[[owners]]`, `click = 0`, Wi-Fi `qca_cld3_kiwi_v2`).

Dưới đây là đặc tả đầy đủ từng phần và các quy tắc kiểm tra trong `crates/aweyuki-config/src/lib.rs`:

### `[device]` — Thông tin định danh thiết bị

```toml
[device]
codename = "nezha"
brand = "Xiaomi"
model = "Xiaomi 17 Ultra"
ab = true
soc = { hans = "第五代骁龙®8至尊版移动平台", hant = "Snapdragon® 8 Elite Gen 5 行動平台", en = "Snapdragon® 8 Elite Gen 5 Mobile Platform" }
os_version_prop = "ro.mi.os.version.incremental"
```

- `codename` *(bắt buộc)*: Tên mã thiết bị (khớp với tên tệp `.toml`).
- `brand`: Tên hãng.
- `model`: Tên thương mại; CI dùng trường này để đặt tiêu đề file trên GitHub Releases dạng `<model> (<codename>)`.
- `ab`: Mặc định `true` (thiết bị phân vùng A/B).
- `soc`: Tên SoC hiển thị trên trang **Về điện thoại (About)**, gồm 3 khóa `{ hans = "...", hant = "...", en = "..." }` (có thể bỏ trống nếu không muốn ghi cứng).
- `os_version_prop`: Thuộc tính `build.prop` chứa phiên bản OS của hãng (mặc định `"ro.build.display.id"`; với Xiaomi HyperOS/MIUI dùng `"ro.mi.os.version.incremental"`).

### `[display]` — Màn hình

```toml
[display]
width = 1200
height = 2608
density = 480
diagonal = 6.9
corner_radius = 170
```

- `width`, `height`, `density`: Độ phân giải vật lý (pixel) và DPI màn hình (kiểm tra qua `persist.sys.miui_resolution` trong `odm/etc/build.prop` đã trích xuất, hoặc lệnh `aweyuki kms` khi boot vào recovery).
- `diagonal`: Kích thước đường chéo màn hình (inch, số thực, tùy chọn) để hiển thị ở trang Thông số.
- `corner_radius`: Bán kính bo góc (pixel) giúp hiệu ứng chuyển trang khớp với góc bo phần cứng của màn hình (có thể tham khảo `rounded_corner_radius` trong `DevicesAndroidOverlay`).

### `[modules]` — Nạp kernel modules khi khởi động

```toml
[modules]
sources = ["vendor_boot", "vendor_dlkm", "vendor", "system_dlkm"]
load = ["synaptics_tcm2"]
```

- `sources`: Danh sách phân vùng tìm module kernel (thứ tự ưu tiên). Mặc định nếu bỏ qua là `["vendor_boot", "vendor_dlkm", "vendor"]`. Thêm `"system_dlkm"` nếu cần module chung của nhân (ví dụ `rfkill`).
- `load`: Tên các module cần nạp lúc khởi động **không kèm đuôi `.ko`**, thường là driver cảm ứng (xem trong `modules.load` của `vendor_dlkm`/`vendor_boot`, ví dụ `synaptics_tcm2`, `fts_touch_spi`, `goodix_core`, `focaltech_touch_3685g`). Các module phụ thuộc trong `modules.dep` sẽ tự động được nạp trước.

### Cảm ứng: `[touch]` hoặc `[[owners]]` + `[[service]]`

Cần kiểm tra cách hệ thống gốc của máy xử lý cảm ứng trong các tệp `.rc` đã trích xuất:

- **Cách 1 — IC cảm ứng tự tính tọa độ khi tắt chế độ raw** (phổ biến trên các máy HyperOS đời mới dùng Synaptics, Goodix, FocalTech):
  ```toml
  [touch]
  writes = [{ path = "/sys/class/touch/touch_dev/enable_touch_raw", value = "0" }]
  ```
  Aweyuki đợi thiết bị input cảm ứng xuất hiện rồi ghi giá trị này vào sysfs.
- **Cách 2 — Cảm ứng xuất khung thô lên userspace để daemon xử lý** (như Xiaomi 13 / 13 Pro / 13 Ultra):
  Khai báo `[[owners]]` để cấp quyền sở hữu thiết bị/node sysfs/proc (vì recovery không chạy các lệnh `chown`/`chmod` trong init boot action của ROM) và chạy daemon trong `[[service]]`:
  ```toml
  [[owners]]
  user = "system"
  group = "system"
  mode = 0o664 # số bát phân hợp lệ <= 0o7777
  paths = [
      "/dev/xiaomi-touch",
      "/proc/tp_hal_version",
      "/sys/class/touch/touch_dev/enable_touch_raw",
      # ... các node từ vendor.xiaomi.hw.touchfeature*.rc
  ]

  [[service]]
  path = "/odm/bin/touch_report"
  user = "system"
  groups = ["system", "uhid", "input"]
  capabilities = ["SYS_NICE"]
  ```

### `[firmware]` & `[partitions]` — Gắn kết phân vùng firmware và persist

```toml
[firmware]
partition = "modem"

[partitions]
mounted = ["persist"]
```

- `[firmware].partition`: Tên phân vùng thuần (không chứa `/`, không kèm `_a`/`_b`) chứa firmware của Wi-Fi, chip khác hoặc Trusted Apps mà `ueventd` không tự thấy trong recovery.
- `[partitions].mounted`: Các phân vùng cần gắn sẵn ở điểm gắn theo bảng `fstab` của máy trước khi chạy dịch vụ (chẳng hạn `persist` để `qseecomd` lưu dữ liệu chống rollback của khóa mã hóa và cung cấp MAC Wi-Fi).

### `[logd]` — Thư viện bổ sung cho `logd` (nếu thiếu)

```toml
[logd]
libraries = [
    "/mnt/system/system/lib64/libsysutils.so",
    "/mnt/system/system/lib64/libcap.so",
]
```

Chỉ cần thiết nếu ramdisk recovery gốc của máy thiếu thư viện mà `logd` liên kết động tới (khiến `logd` thoát liên tục và logcat trống trơn). Đường dẫn phải tuyệt đối và trỏ tới tệp cụ thể.

### `[[service]]` — Các chương trình nền từ hệ thống cài đặt

**Lưu ý quan trọng:** **KHÔNG cần** khai báo `qseecomd`, `keymint`, `gatekeeper`, `weaver` hay `secure_element` vào `[[service]]`. Crate `aweyuki-programs` sẽ tự động đọc và phân tích trực tiếp các tệp `.rc` + VINTF manifest trên phân vùng đang cài của điện thoại lúc chạy để khởi động đúng chuỗi dịch vụ bảo mật của phiên bản ROM hiện tại!

Mục `[[service]]` chỉ dùng cho:
1. **Script hậu khởi động phần cứng/CPU** (rất khuyến nghị trên máy Qualcomm để tắt governor `performance` và bật chế độ nghỉ CPU, tránh nóng máy khi ở recovery):
   ```toml
   [[service]]
   path = "/vendor/bin/sh"
   args = ["/vendor/bin/init.kernel.post_boot.sh"]
   libraries = ["/system/lib64", "/vendor/lib64"]
   oneshot = true
   user = "root"
   groups = ["root", "system", "wakelock", "graphics"]
   ```
2. **Các daemon riêng ngoài chuỗi giải mã** như daemon cảm ứng (`touch_report`, `touchsensor`) hoặc dịch vụ đặc thù theo biến thể SKU phần cứng:
   - `when = { property = "ro.boot.product.hardware.sku", value = "..." }`: Chỉ chạy nếu thuộc tính hệ thống khớp giá trị.
   - Ràng buộc kiểm tra của parser:
     - Nếu `oneshot = true`, **không** được đặt `ready_property`, `ready_service`, `ready_hardware_service` hay `registers`.
     - Nếu có `user`, **bắt buộc** phải khai báo `groups` (không được rỗng).
     - `capabilities` ghi đúng tên kernel **không có tiền tố `CAP_`** (ví dụ `"SYS_NICE"`, `"WAKE_ALARM"`, xem danh sách 41 capability trong `crates/aweyuki-config/src/lib.rs`).

### `[wlan]` & `[update]` — Wi-Fi và kiểm tra cập nhật hệ thống

```toml
[wlan]
modules = ["rfkill", "qca_cld3_peach_v2"]
writes = [{ path = "/sys/kernel/cnss/fs_ready", value = "1" }]
# Tùy chọn nếu firmware loader trong recovery không tìm thấy cấu hình/MAC từ /vendor:
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

- Kiểm tra module driver Wi-Fi thực tế trong `modules.load` / `modules.blocklist` và node dtbo (ví dụ `qca_cld3_peach_v2` hay `qca_cld3_kiwi_v2`).
- Với `[wlan].firmware`: `name` là đường dẫn tương đối mà kernel firmware loader yêu cầu (không bắt đầu bằng `/`, không chứa `..`); nếu tệp nằm trên phân vùng không gắn sẵn (như `persist`), ghi rõ `partition` (tên thuần) và `path` tương đối từ gốc phân vùng đó; nếu nằm trên phân vùng đã gắn, `path` phải là đường dẫn tuyệt đối.
- `[wlan].probe` **bắt buộc dùng `http://` thường** (không phải `https://`), để phát hiện mạng yêu cầu đăng nhập captive portal.

### `[haptics]` — Động cơ rung phản hồi

```toml
[haptics]
# click = 0 # ID waveform có sẵn trong driver (tùy chọn; bỏ nếu tạo xung từ tần số cộng hưởng)
resonance = "/sys/class/qcom-haptics/lra_frequency_hz"
strength = { lightest = 0.25, strongest = 0.75 }
```

- `resonance`: Đường dẫn tuyệt đối tới tệp sysfs đọc tần số cộng hưởng.
- `strength`: Phải thỏa mãn điều kiện **`0.0 < lightest < strongest <= 1.0`**.

### `[backup]` & `[wipe]` — Sao lưu EFS/Data và Xóa nhật ký hệ thống

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

Chú ý sự khác biệt định dạng đường dẫn giữa hai mục này:
- `[backup].efs`: Tên phân vùng thuần (không `/`).
- `[backup].data_skipped`: Đường dẫn **tương đối tính từ gốc `/data`** (KHÔNG có `/` ở đầu, không `..`); hỗ trợ ký tự đại diện `*` khớp đúng một tên thư mục (ví dụ `"vendor_de/*/fpdata"`). Đây là các thư mục khóa theo trạng thái phần cứng bảo mật mà nếu chép đè bản sao lưu cũ sẽ làm hỏng dữ liệu sinh trắc/TEE.
- `[wipe].logs`: Đường dẫn **tuyệt đối bắt đầu bằng `/data/`** (không chứa `..`, không phải chính `/data`).

---

## Bước 3: Đăng ký vào bộ kiểm thử & Chạy kiểm tra Offline

1. Mở [crates/aweyuki-config/src/lib.rs](../crates/aweyuki-config/src/lib.rs), tìm hàm test `each_phone_names_the_stock_build_its_recovery_is_made_on` (khoảng dòng 776) và bổ sung thiết bị mới cùng mã `[stock].version`:

   ```rust
   (include_str!("../../../devices/<codename>.toml"), "<STOCK_VERSION>"),
   ```

2. Xác nhận thư mục `crates/aweyuki-programs/tests/described/<codename>/` đã được trích xuất ở Bước 1.2. Test `every_phone_names_one_program_for_each_service` trong [crates/aweyuki-programs/tests/phones.rs](../crates/aweyuki-programs/tests/phones.rs) tự động quét mọi thư mục con trong `tests/described/` để kiểm chứng chuỗi khởi chạy KeyMint/Gatekeeper/Weaver/VINTF qua tất cả các biến thể phần cứng `manifest_<sku>.xml`. *(Lưu ý: `phones.rs` hiện mặc định truyền `ro.boot.hardware = "qcom"`; nếu porting cho SoC không phải Qualcomm, cần cập nhật thuộc tính phần cứng tương ứng trong `phones.rs`.)*

3. Chạy đầy đủ kiểm thử và lint trên máy tính (cần Rust đúng phiên bản trong `rust-toolchain.toml`, Android NDK r29 và `cargo-ndk`):

   ```sh
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo ndk -t arm64-v8a --platform 30 clippy --workspace --all-targets -- -D warnings
   bash .claude/skills/upstream-sync/scripts/check-invariants.sh
   ```

---

## Bước 4: Đóng gói ảnh, Flash & Gỡ lỗi trực tiếp trên máy thật

### 4.1. Build nhị phân và đóng gói `.img`

```sh
# 1. Build dịch vụ recovery cho arm64-v8a
cargo ndk -t arm64-v8a --platform 30 build --release -p aweyuki

# 2. Build aweyuki-system-init (liên kết tĩnh cho Khởi động kép)
RUSTFLAGS="-C target-feature=+crt-static" cargo ndk -t arm64-v8a --platform 30 build --release -p aweyuki-system-init --target-dir target/system-init

# 3. Đóng gói ảnh recovery trên nền ảnh gốc
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

Flash vào thiết bị ở chế độ fastboot (thiết bị phải đã mở khóa bootloader):

```sh
fastboot flash recovery target/aweyuki-<codename>.img
fastboot reboot recovery
```

### 4.2. Công cụ chẩn đoán qua `adb shell`

Do `aweyuki-pack` tự động đặt `ro.secure=0` và `ro.adb.secure=0` trong `prop.default`, ngay cả khi màn hình đen hoặc cảm ứng chưa nhận, bạn vẫn có thể vào root shell qua `adb shell` để kiểm tra từng thành phần bằng chính CLI của `aweyuki`:

| Mục tiêu kiểm tra | Lệnh chạy trong `adb shell` |
| --- | --- |
| Nhật ký khởi tạo recovery | `dmesg \| grep -i aweyuki` và `logcat -d` |
| Thông tin máy & biến thể SKU | `aweyuki info --config /aweyuki/device.toml` |
| Khe A/B & metadata `super` | `aweyuki slot` và `aweyuki lp` |
| Gắn kết các phân vùng `super` | `aweyuki mount` (gỡ bằng `aweyuki umount`) |
| Màn hình DRM/KMS & độ phân giải | `aweyuki kms` và `aweyuki capture /tmp/shots` |
| Thứ tự nạp & kiểm tra module | `aweyuki modules --source /vendor_dlkm/lib/modules` và `aweyuki load` |
| Sự kiện chạm cảm ứng | `getevent -lt` |
| Chuỗi dịch vụ giải mã từ ROM gốc | `aweyuki keys` |
| Chẩn đoán KeyMint / Gatekeeper / Weaver | `aweyuki service <tên_dịch_vụ>`, `aweyuki keymint`, `aweyuki gatekeeper`, `aweyuki weaver` |
| Thử giải mã `/data` từ dòng lệnh | `aweyuki data --credential "<mã_pin_hoặc_mật_khẩu>"` |
| Căn chỉnh motor rung `lightest`/`strongest` | `aweyuki haptic --position 0.5` và `aweyuki haptic --ticks` |

Khi `/data` đã giải mã và mở thành công, bạn cũng có thể chạy trực tiếp bộ test tích hợp phần cứng trên chính điện thoại (từ máy tính đang nối `adb`):

```sh
cargo ndk -t arm64-v8a --platform 30 test -p aweyuki -p aweyuki-block -p aweyuki-files -p aweyuki-mtp on_the_phone -- --test-threads=1
```

---

## Bước 5: Đưa vào danh sách hỗ trợ & Tự động phát hành (CI/Release)

### 5.1. Build & Phát hành tự động trên GitHub Actions

File workflow [.github/workflows/build.yml](../.github/workflows/build.yml) **tự động phát hiện mọi tệp `devices/*.toml`** (không cần sửa thủ công danh sách thiết bị trong workflow):
- Khi push nhánh hoặc tag `v*`, job `devices` liệt kê toàn bộ `devices/*.toml`, tải & cache `recovery.img` gốc dựa theo `[stock]`, đóng gói `.zip` và kiểm tra SHA-256.
- Khi tạo tag phát hành khớp với `version` trong `Cargo.toml` (ví dụ `v0.4.0`), từng máy đều phải đóng gói thành công và được đính kèm lên GitHub Releases dưới tên `<device.model> (<device.codename>)`.
- Bạn có thể chạy thử build riêng một thiết bị trên GitHub Actions bằng **Run workflow (`workflow_dispatch`)** và nhập `codename` vào ô `device`.

### 5.2. Cập nhật bảng thiết bị trong `README*.md`

Cập nhật bảng thiết bị tại cả ba tệp [README.vi.md](../README.vi.md), [README.md](../README.md) và [README.zh-CN.md](../README.zh-CN.md):

- **Trạng thái `🚧 Đang hoàn thiện` (`Under construction` / `施工中`)**: Dùng cho thiết bị đã viết `devices/<codename>.toml`, đã trích xuất `tests/described/<codename>`, vượt qua toàn bộ `cargo test --workspace` và build thành công trên CI, nhưng chưa được xác minh trực tiếp trên máy thật.
- **Trạng thái chính thức (`Thiết bị hỗ trợ` / `Supported devices` / `支持的机型`)**: Chuyển lên bảng chính sau khi đã thử nghiệm ổn định trên máy thật (hiển thị, cảm ứng, Wi-Fi, rung, giải mã phân vùng dữ liệu, cài gói/cập nhật, sao lưu/khôi phục và chuyển khe/khởi động kép nếu hỗ trợ).
