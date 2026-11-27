![Aweyuki Recovery](assets/hero.jpg)

# Aweyuki Recovery

[English](README.md) | [简体中文](README.zh-CN.md) | Tiếng Việt

Aweyuki Recovery là một recovery Android viết lại từ đầu bằng Rust.

<p align="center">
  <img src="assets/screenshots/01-home.png" width="30%" alt="Trang chủ Aweyuki Recovery">
  <img src="assets/screenshots/22-settings-dark.png" width="30%" alt="Cài đặt ở chế độ tối">
  <img src="assets/vi/time-zone.png" width="30%" alt="Múi giờ">
</p>

## Tính năng

### Khởi động kép

<p align="center">
  <img src="assets/screenshots/02-dual-boot.png" width="32%" alt="Chọn hệ thống khởi động kép">
</p>

Cài đặt hệ thống thứ hai song song với ROM hiện tại (HyperOS hoặc ROM tùy biến) với cơ chế chuyển đổi nguyên tử qua `/metadata`, chỉ ghi vào `boot` và `init_boot`. Cách đóng gói và khởi động hệ thống thứ hai có ở [Cách khởi động kép hoạt động](#cách-khởi-động-kép-hoạt-động).

### Giải mã & Khóa PIN Recovery

<p align="center">
  <img src="assets/screenshots/03-decrypt.png" width="32%" alt="Giải mã phân vùng dữ liệu bằng khóa màn hình">
</p>

Giải mã `/data` bằng mã PIN, hình vẽ hoặc mật khẩu khóa màn hình của hệ thống, đồng thời hỗ trợ đặt khóa PIN riêng cho recovery để bảo vệ quyền truy cập.

### Cài đặt, Kiểm tra OTA Payload & Flash hàng loạt

<p align="center">
  <img src="assets/screenshots/04-install.png" width="32%" alt="Cài đặt gói">
  <img src="assets/screenshots/15-batch-flash.png" width="32%" alt="Flash hàng loạt">
</p>

- **Nguồn cài đặt & công cụ**: Flash tệp `.zip`, `.img`, `.efi` và gói khởi động kép từ Bộ nhớ trong, USB OTG hoặc ADB Sideload kèm tùy chọn kiểm tra chữ ký zip. Script cài đặt có sẵn `7za`, `zstd`, `unzip -p` đọc luồng trực tiếp, `avbctl` và `resetprop`.
- **Trình kiểm tra OTA Payload & ARB**: Đọc manifest của `payload.bin` / OTA `.zip`, kiểm tra chống hạ cấp (Anti-Rollback / ARB) và trích xuất phân vùng (`REPLACE`, `REPLACE_ZSTD`, `ZERO`, `DISCARD`).
- **Flash hàng loạt & EFISP**: Chọn và flash nhiều ảnh `.img`/`.efi` cùng lúc có kiểm tra kích thước phân vùng, hỗ trợ flash trực tiếp ảnh ARM64 `.efi` vào phân vùng `efisp` không phân khe.

### Cập nhật trực tuyến & Wi-Fi

<p align="center">
  <img src="assets/screenshots/05-system-updates.png" width="32%" alt="Cập nhật hệ thống">
  <img src="assets/screenshots/21-wifi.png" width="32%" alt="Cài đặt Wi-Fi">
</p>

Kết nối Wi-Fi 2.4 GHz / 5 GHz ngay trong recovery để kiểm tra, cài đặt cập nhật hệ thống (kèm trạng thái hợp nhất A/B) và tự cập nhật Aweyuki Recovery.

### Sao lưu, Khôi phục & Kho ứng dụng

<p align="center">
  <img src="assets/screenshots/06-backup.png" width="30%" alt="Sao lưu phân vùng">
  <img src="assets/screenshots/07-restore.png" width="30%" alt="Khôi phục bản sao lưu">
  <img src="assets/screenshots/16-app-vault.png" width="30%" alt="Kho ứng dụng">
</p>

- **Sao lưu & Khôi phục**: Sao lưu và khôi phục `boot`, `system`, `data`, Modem & EFS và tùy chọn `efisp` ra Bộ nhớ trong hoặc USB OTG với nén và kiểm tra toàn vẹn.
- **Kho ứng dụng (App Vault)**: Sao lưu và khôi phục từng ứng dụng người dùng dạng `.tar.lz4` tại `/sdcard/Aweyuki/app-vault`.

### Trình quản lý tệp, Gắn kết & Terminal

<p align="center">
  <img src="assets/screenshots/11-file-manager.png" width="30%" alt="Trình quản lý tệp">
  <img src="assets/screenshots/08-mount.png" width="30%" alt="Gắn kết phân vùng">
  <img src="assets/screenshots/12-terminal.png" width="30%" alt="Terminal root">
</p>

- **Trình quản lý tệp**: Duyệt thư mục `/` với thông tin chi tiết, đích liên kết tượng trưng (symlink) và nút mở nhanh Terminal.
- **Gắn kết & USB**: Bật/tắt gắn kết phân vùng động (`system`, `vendor`, `product`, `odm`, `mi_ext`, `system_ext`, `system_dlkm`) và quản lý kết nối MTP/ADB qua USB.
- **Terminal**: Shell root tích hợp sẵn bàn phím trên màn hình (`Esc`, `Tab`, `Ctrl`, các phím mũi tên).

### Xóa dữ liệu, SELinux & Sửa hệ thống tệp

<p align="center">
  <img src="assets/screenshots/09-wipe.png" width="30%" alt="Xóa dữ liệu">
  <img src="assets/screenshots/13-selinux.png" width="30%" alt="Sửa ngữ cảnh SELinux">
  <img src="assets/screenshots/14-filesystem-repair.png" width="30%" alt="Sửa hệ thống tệp">
</p>

Mỗi tùy chọn xóa đều ghi rõ phạm vi tác động. Hỗ trợ sửa ngữ cảnh SELinux cho `/data`, kiểm tra và sửa chữa hệ thống tệp `/data` (`f2fs`), cùng xóa ghi số 0 kiểm tra từng byte cho phân vùng `efisp` và `frp`.

### Quản lý Root

<p align="center">
  <img src="assets/screenshots/17-root-manager.png" width="32%" alt="Quản lý Root">
</p>

Tự động nhận diện phiên bản nhân và KMI (ví dụ `5.15.194` / `android13-5.15`), phát hiện Magisk, KernelSU, APatch và cho phép bật, tắt hoặc đánh dấu gỡ mô-đun trong `/data/adb/modules/*`.

### Chia sẻ tệp qua mạng & Điều khiển từ xa (`/mirror`)

<p align="center">
  <img src="assets/screenshots/18-network-file-sharing.png" width="32%" alt="Chia sẻ tệp qua mạng">
</p>

Bật máy chủ chia sẻ tệp HTTP/1.1 qua Wi-Fi để tải lên/tải xuống trên trình duyệt hoặc qua `curl -T`, kèm trang `/mirror` phản chiếu màn hình trực tiếp, điều khiển cảm ứng/phím bấm và lưu ảnh PNG từ trình duyệt máy tính.

### Tiện ích mở rộng & Ứng dụng AERA `.aerap`

<p align="center">
  <img src="assets/screenshots/19-plugins.png" width="32%" alt="Plugin và AERA">
</p>

Chạy các plugin script `plugin.toml` với luồng đầu ra trực tiếp và khả năng dừng ngang, đồng thời hỗ trợ các gói GUI AERA `.aerap` chạy cách ly như **AERA Browser** (WPE WebKit) và **AERA Telegram** (TDLib).

### Nâng cao, Công cụ nhà phát triển & Chụp màn hình không cần phím cứng

<p align="center">
  <img src="assets/screenshots/10-advanced.png" width="32%" alt="Menu Nâng cao">
  <img src="assets/screenshots/20-developer-benchmark.png" width="32%" alt="Nhà phát triển và đo tốc độ">
</p>

- **Menu Nâng cao**: Truy cập nhanh Tìm kiếm, Nhật ký (gồm lịch sử Flash/Xóa gần đây), Trình quản lý tệp, Terminal, sửa SELinux/hệ thống tệp, Quản lý Root, Chia sẻ tệp qua mạng, Plugin và Kho ứng dụng.
- **Nhà phát triển & Đo tốc độ**: Xem và chỉnh mức log `printk`, trạng thái SELinux, USB/ADB, bộ điều phối CPU, `cmdline` nhân và chạy bài đo tốc độ đọc/ghi tuần tự 64 MiB.
- **Chụp màn hình không cần phím âm lượng**: Chụp ảnh PNG qua mục **Chụp màn hình (sau 3 giây)** trong Nâng cao (thông báo tự ẩn trước khi chụp) hoặc chụp trực tiếp từ `/mirror`.

### Thiết lập, Thông tin máy & Khởi động lại

<p align="center">
  <img src="assets/screenshots/23-about-specs.png" width="32%" alt="Thông tin máy">
  <img src="assets/screenshots/24-reboot.png" width="32%" alt="Khởi động lại">
</p>

- **Ngôn ngữ & Giao diện**: Giao diện sáng/tối; 15 ngôn ngữ (简体中文, 繁體中文, English, Türkçe, Русский, Українська, Español, Português (Brasil), Bahasa Indonesia, Tiếng Việt, Deutsch, Français, Italiano, Polski và 日本語); múi giờ IANA bao gồm Việt Nam (`Asia/Ho_Chi_Minh`, UTC+7).
- **Thông tin máy**: Xem phiên bản Aweyuki, dung lượng lưu trữ, SoC, RAM, tình trạng pin và số chu kỳ sạc.
- **Khởi động lại & Chọn khe**: Khởi động lại vào Hệ thống, Recovery, Fastboot, Bootloader, EDL hoặc Tắt nguồn, chuyển khe A/B và cảnh báo ABL mất bộ nạp EFISP trước khi khởi động lại.

## Thiết bị hỗ trợ

| Thiết bị | Tên mã | Phiên bản hệ thống đã thử |
| --- | --- | --- |
| Xiaomi 17 Ultra | `nezha` | OS4.0.4.0.XPACNXM (Android 17) |
| Xiaomi 13 Ultra | `ishtar` | OS3.0.308.0.WMACNXM (Android 16) |
| REDMI K60 Pro | `socrates` | OS4.0.0.24.XMKCNXM (Android 17) |
| Xiaomi 13 Pro | `nuwa` | OS3.0.312.0.WMBCNXM (Android 16) |

### 🚧 Đang hoàn thiện

Các ảnh này được dựng từ firmware gốc của từng máy và chưa được thử trên điện thoại. Chúng
cấu hình màn hình, cảm ứng, Wi-Fi và rung. Các chương trình cần cho việc giải mã phân
vùng dữ liệu được đọc ngay trên máy từ script init và mô tả dịch vụ của hệ thống đã cài;
những gì đọc được đã đối chiếu với firmware gốc của từng mẫu, còn cách các chương
trình đó chạy trên những mẫu này thì chưa được kiểm chứng.

| Thiết bị | Tên mã | Dựng trên |
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

### Thiết bị port từ cộng đồng

Được tổng hợp từ các fork cộng đồng của `kirklin/skyward` (xem liên kết repo gốc để biết thông tin người duy trì). Các máy có `[stock]` được CI tự động build khi phát hành; các máy ghi *Tự đóng gói* có thể đóng gói thủ công bằng `aweyuki-pack` từ `recovery.img` hoặc `vendor_boot.img` gốc của máy.

<!-- COMMUNITY-DEVICES-START -->
| Thiết bị | Tên mã | Dựng trên | Nguồn |
| --- | --- | --- | --- |
| OnePlus ACE 6T | `OP6117L1` | Tự đóng gói (chưa có `[stock]`) | [servant1228/skyward-plr110](https://github.com/servant1228/skyward-plr110) |
| OnePlus PLY110 (一加 Turbo 6V) | `OP64D3L1` | Tự đóng gói (chưa có `[stock]`) | [TvTzzz/skyward-oneplus-turbo-6v](https://github.com/TvTzzz/skyward-oneplus-turbo-6v) |
| OnePlus Ace 6 | `PLQ110` | Tự đóng gói (chưa có `[stock]`) | [liuyimeng1024-ui/skyward-for-oneplusace6](https://github.com/liuyimeng1024-ui/skyward-for-oneplusace6) |
| realme RMX3551 | `RE5465` | Tự đóng gói (chưa có `[stock]`) | [partofpai/skyward-oplus-sm8475](https://github.com/partofpai/skyward-oplus-sm8475) |
| realme 真我Neo7 | `RMX5060` | Tự đóng gói (chưa có `[stock]`) | [aaabbk/Realme_neo7_skyward](https://github.com/aaabbk/Realme_neo7_skyward) |
| OPPO A57 | `a57` | Tự đóng gói (chưa có `[stock]`) | [Max1937/skyward-a57](https://github.com/Max1937/skyward-a57) |
| Redmi K40 | `alioth` | `OS1.0.10.0.TKHCNXM` | [fuxilover/skyward](https://github.com/fuxilover/skyward) |
| Redmi Note 14 Pro+ 5G | `amethyst` | `OS3.0.306.0.WOPCNXM` | [7-Ice/skyward](https://github.com/7-Ice/skyward) |
| Redmi K30S Ultra | `apollo` | Tự đóng gói (chưa có `[stock]`) | [kafuzhi/skyward](https://github.com/kafuzhi/skyward) |
| Samsung Galaxy Z Flip4 5G | `b4q` | Tự đóng gói (chưa có `[stock]`) | [yanzihan/Skyward-Galaxy](https://github.com/yanzihan/Skyward-Galaxy) |
| Xiaomi MIX Fold 3 | `babylon` | `OS3.0.304.0.WMVCNXM` | [zfgysyuef/skyward](https://github.com/zfgysyuef/skyward) |
| Xiaomi Redmi Note 8 Pro | `begonia` | Tự đóng gói (chưa có `[stock]`) | [pacman-Sy/skyward](https://github.com/pacman-Sy/skyward) |
| Xiaomi MIX Flip 2 | `bixi` | `OS4.0.0.11.XOHCNXM` | [XQAB/skyward](https://github.com/XQAB/skyward) |
| motorola ThinkPhone by motorola | `bronco` | Tự đóng gói (chưa có `[stock]`) | [iljasrb/skyward](https://github.com/iljasrb/skyward) |
| Redmi Note 9 5G | `cannon` | Tự đóng gói (chưa có `[stock]`) | [limitbeer0/skyward-cannon](https://github.com/limitbeer0/skyward-cannon) |
| Xiaomi Mi 10 Ultra | `cas` | Tự đóng gói (chưa có `[stock]`) | [kafuzhi/skyward](https://github.com/kafuzhi/skyward) |
| Xiaomi Mi MIX 2 | `chiron` | Tự đóng gói (chưa có `[stock]`) | [6ewjso6-gif/skyward](https://github.com/6ewjso6-gif/skyward) |
| REDMI Note 10 Pro | `chopin` | Tự đóng gói (chưa có `[stock]`) | [Hwhmi/skyward](https://github.com/Hwhmi/skyward) |
| Xiaomi Mi 10 Pro | `cmi` | Tự đóng gói (chưa có `[stock]`) | [kafuzhi/skyward](https://github.com/kafuzhi/skyward) |
| OnePlus Ace 3 Pro | `corvette` | Tự đóng gói (chưa có `[stock]`) | [Hotsteel2901/skyward-OPA3P](https://github.com/Hotsteel2901/skyward-OPA3P) |
| Xiaomi Mi 9 Pro 5G | `crux` | Tự đóng gói (chưa có `[stock]`) | [tao88vip/skyward](https://github.com/tao88vip/skyward) |
| Xiaomi MI 8 | `dipper` | Tự đóng gói (chưa có `[stock]`) | [yuan-shiguang/skyward](https://github.com/yuan-shiguang/skyward) |
| Redmi K50 Ultra | `diting` | `OS3.0.3.0.VLFCNXM` | [East888666/skyward-xiaomi-legacy](https://github.com/East888666/skyward-xiaomi-legacy) |
| realme GT6 (RMX3800) | `divo` | Tự đóng gói (chưa có `[stock]`) | [ChuiShui233/realme-pineapple-skyward](https://github.com/ChuiShui233/realme-pineapple-skyward) |
| OnePlus 13 | `dodge` | Tự đóng gói (chưa có `[stock]`) | [Hdy08/skyward](https://github.com/Hdy08/skyward) |
| Xiaomi Pad 5 Pro 5G | `enuma` | `OS1.0.3.0.TKZCNXM` | [kayn0922/skyward](https://github.com/kayn0922/skyward) |
| OnePlus Pad 2 Pro | `erhai` | Tự đóng gói (chưa có `[stock]`) | [Goldzxcbug/skyward](https://github.com/Goldzxcbug/skyward) |
| Redmi Note 11 5G | `evergo` | Tự đóng gói (chưa có `[stock]`) | [SkyShadowHero/skyward_for_evergo](https://github.com/SkyShadowHero/skyward_for_evergo) |
| realme GT7 | `gt7` | Tự đóng gói (chưa có `[stock]`) | [ReikoYuzuka/skyward](https://github.com/ReikoYuzuka/skyward) |
| realme GT8 | `gt8` | Tự đóng gói (chưa có `[stock]`) | [ReikoYuzuka/skyward](https://github.com/ReikoYuzuka/skyward) |
| OnePlus 7 | `guacamoleb` | Tự đóng gói (chưa có `[stock]`) | [imqinyu/skyward_guacamoleb](https://github.com/imqinyu/skyward_guacamoleb) |
| Redmi K40 Pro | `haydn` | Tự đóng gói (chưa có `[stock]`) | [qaz6750/skyward](https://github.com/qaz6750/skyward) |
| OnePlus Ace 5 Pro | `hummer` | `v13.1` | [venti1112/skyward-hummer](https://github.com/venti1112/skyward-hummer) |
| Xiaomi Mi Note 3 | `jason-los23` | Tự đóng gói (chưa có `[stock]`) | [East888666/skyward-xiaomi-legacy](https://github.com/East888666/skyward-xiaomi-legacy) |
| Xiaomi Mi Note 3 | `jason-miui12` | Tự đóng gói (chưa có `[stock]`) | [East888666/skyward-xiaomi-legacy](https://github.com/East888666/skyward-xiaomi-legacy) |
| Xiaomi Mi Note 3 | `jason` | Tự đóng gói (chưa có `[stock]`) | [East888666/skyward-xiaomi-legacy](https://github.com/East888666/skyward-xiaomi-legacy) |
| OnePlus 8T | `kebab` | Tự đóng gói (chưa có `[stock]`) | [toraidl/skyward](https://github.com/toraidl/skyward) |
| Xiaomi Redmi Note 7 | `lavender` | Tự đóng gói (chưa có `[stock]`) | [aLpHa-Git-69/skyward](https://github.com/aLpHa-Git-69/skyward) |
| OnePlus 9R | `lemonades` | Tự đóng gói (chưa có `[stock]`) | [lululuhu/skyward](https://github.com/lululuhu/skyward) |
| Xiaomi Pad 6 Pro | `liuqin-nomounts` | `OS3.0.7.0.VMYCNXM` | [qasg/skyward-Xiaomio-Pad-6-Pro](https://github.com/qasg/skyward-Xiaomio-Pad-6-Pro) |
| Xiaomi Pad 6 Pro | `liuqin` | `OS3.0.7.0.VMYCNXM` | [qasg/skyward-Xiaomio-Pad-6-Pro](https://github.com/qasg/skyward-Xiaomio-Pad-6-Pro) |
| Redmi K30 Pro | `lmi` | Tự đóng gói (chưa có `[stock]`) | [kafuzhi/skyward](https://github.com/kafuzhi/skyward) |
| Xiaomi Civi 5 Pro | `luming` | `OS4.0.2.0.XOICNXM` | [justistab3-bot/skyward](https://github.com/justistab3-bot/skyward) |
| Meizu 20 | `m2381` | Tự đóng gói (chưa có `[stock]`) | [un6666cn/skyward](https://github.com/un6666cn/skyward) |
| MEIZU 20 Pro | `m2391` | Tự đóng gói (chưa có `[stock]`) | [baizhi958216/skyward](https://github.com/baizhi958216/skyward) |
| REDMI K70 Pro | `manet` | `OS3.0.307.0.WNMCNXM` | [Kinaxie/skyward](https://github.com/Kinaxie/skyward) |
| Redmi Note 12 Turbo | `marble` | `OS3.0.5.0.VMRCNXM` | [lingdiansr/skyward-marble](https://github.com/lingdiansr/skyward-marble) |
| Redmi Note 9 (Global) | `merlinx` | Tự đóng gói (chưa có `[stock]`) | [Dracen-W/skyward-merlinx](https://github.com/Dracen-W/skyward-merlinx) |
| Xiaomi Mi-Thorium 4.19 | `mi8937_4_19` | Tự đóng gói (chưa có `[stock]`) | [aLpHa-Git-69/skyward](https://github.com/aLpHa-Git-69/skyward) |
| Redmi K60 | `mondrian` | `OS3.0.8.0.VMNCNXM` | [Kinaxie/skyward](https://github.com/Kinaxie/skyward) |
| Xiaomi Pad 7 Pro | `muyu` | `OS4.0.3.0.XOYCNXM` | [Kinaxie/skyward](https://github.com/Kinaxie/skyward) |
| Xiaomi Pad 5 | `nabu` | Tự đóng gói (chưa có `[stock]`) | [NekoMymaomao/skyward](https://github.com/NekoMymaomao/skyward) |
| Nubia Z70 Ultra | `nx733j` | Tự đóng gói (chưa có `[stock]`) | [Hdy08/skyward](https://github.com/Hdy08/skyward) |
| nubia REDMAGIC 10 Pro | `nx789j` | Tự đóng gói (chưa có `[stock]`) | [GIT-LINCC/skyward-nx789j](https://github.com/GIT-LINCC/skyward-nx789j) |
| REDMI Turbo 4 Pro | `onyx` | `OS4.0.4.0.XOLCNXM` | [Kinaxie/skyward](https://github.com/Kinaxie/skyward) |
| OPPO Find X3 | `op4e5d` | Tự đóng gói (chưa có `[stock]`) | [toraidl/skyward](https://github.com/toraidl/skyward) |
| OnePlus Ace 6 | `op6113l1` | Tự đóng gói (chưa có `[stock]`) | [lengyue632-commits/skyward](https://github.com/lengyue632-commits/skyward) |
| OnePlus Ace 5 | `ossi` | Tự đóng gói (chưa có `[stock]`) | [Ral-sei/skyward-for-oneplusace5](https://github.com/Ral-sei/skyward-for-oneplusace5) |
| EEBBK X3/X5/X6A/A6/A7 | `p21h170` | Tự đóng gói (chưa có `[stock]`) | [weikaizhi4/skyward](https://github.com/weikaizhi4/skyward) |
| EEBBK A2/X2/X2 Pro | `p22h190` | Tự đóng gói (chưa có `[stock]`) | [weikaizhi4/skyward](https://github.com/weikaizhi4/skyward) |
| Samsung Galaxy S21 Ultra 5G | `p3q` | Tự đóng gói (chưa có `[stock]`) | [yanzihan/Skyward-Galaxy](https://github.com/yanzihan/Skyward-Galaxy) |
| OPPO A9x | `pcem00` | Tự đóng gói (chưa có `[stock]`) | [Winkmoon/skyward](https://github.com/Winkmoon/skyward) |
| OPPO A11x | `pchm30` | Tự đóng gói (chưa có `[stock]`) | [hhsbjbsj/skyward](https://github.com/hhsbjbsj/skyward) |
| Xiaomi Redmi Turbo 3 | `peridot` | `OS3.0.305.0.WNPCNXM` | [hoshino114/skyward](https://github.com/hoshino114/skyward) |
| Xiaomi Pad 8 Pro | `piano` | `OS4.0.6.0.XPYCNXM` | [Kinaxie/skyward](https://github.com/Kinaxie/skyward) |
| OnePlus 15 | `plk110` | Tự đóng gói (chưa có `[stock]`) | [liyongshun114514/skyward](https://github.com/liyongshun114514/skyward) |
| Xiaomi Mi MIX 2S | `polaris` | Tự đóng gói (chưa có `[stock]`) | [6ewjso6-gif/skyward](https://github.com/6ewjso6-gif/skyward) |
| Xiaomi 12X | `psyche` | `OS3.0.309.0.WMCCNXM` | [abedmatar7/skyward](https://github.com/abedmatar7/skyward) |
| samsung Galaxy S20 FE 5G | `r8q` | Tự đóng gói (chưa có `[stock]`) | [iljasrb/skyward](https://github.com/iljasrb/skyward) |
| OPPO R9s | `r9s` | Tự đóng gói (chưa có `[stock]`) | [Max1937/skyward-a57](https://github.com/Max1937/skyward-a57) |
| Redmi K20 Pro | `raphael` | Tự đóng gói (chưa có `[stock]`) | [6ewjso6-gif/skyward](https://github.com/6ewjso6-gif/skyward) |
| Xiaomi Mi 11 Lite 5G | `renoir` | Tự đóng gói (chưa có `[stock]`) | [wannqn/skyward](https://github.com/wannqn/skyward) |
| realme RMX3888 | `rmx3888` | Tự đóng gói (chưa có `[stock]`) | [maozi-114/skyward-rmx3888](https://github.com/maozi-114/skyward-rmx3888) |
| realme GT7 Pro | `rmx5010` | Tự đóng gói (chưa có `[stock]`) | [KevinXu07/skyward](https://github.com/KevinXu07/skyward) |
| Xiaomi MIX Flip | `ruyi` | `OS3.0.305.0.WNIMIXM` | [Kinaxie/skyward](https://github.com/Kinaxie/skyward) |
| Xiaomi Mi 6 | `sagit` | Tự đóng gói (chưa có `[stock]`) | [KextNor/skyward](https://github.com/KextNor/skyward) |
| Xiaomi Pad 6S Pro 12.4 | `sheng` | `OS3.0.303.0.WNXCNXM` | [Kinaxie/skyward](https://github.com/Kinaxie/skyward) |
| Redmi K60 Pro | `socrates-decrypt` | `OS3.0.304.0.WMKCNXM` | [dhhdheh/skyward](https://github.com/dhhdheh/skyward) |
| Nothing Phone (1) | `spacewar` | Tự đóng gói (chưa có `[stock]`) | [BruxinCore/Skyward-spacewar](https://github.com/BruxinCore/Skyward-spacewar) |
| Xiaomi 11 Ultra | `star` | Tự đóng gói (chưa có `[stock]`) | [LiAA-CN/skyward-for-Mi11P-U](https://github.com/LiAA-CN/skyward-for-Mi11P-U) |
| Redmi Note 12 5G | `sunstone-2` | Tự đóng gói (chưa có `[stock]`) | [hss2671/skyward](https://github.com/hss2671/skyward) |
| Redmi Note 12R Pro | `sunstone` | `OS2.0.6.0.UMQCNXM` | [hss2671/skyward](https://github.com/hss2671/skyward) |
| Lenovo Xiaoxin Pad Pro GT | `tb710fu` | Tự đóng gói (chưa có `[stock]`) | [WASDDestroy/skyward](https://github.com/WASDDestroy/skyward) |
| Xiaomi Mi 10S | `thyme` | Tự đóng gói (chưa có `[stock]`) | [Flicker-Android-Devices/skyward-mikona](https://github.com/Flicker-Android-Devices/skyward-mikona) |
| REDMI K Pad | `turner` | Tự đóng gói (chưa có `[stock]`) | [xiaozhou0307/skyward](https://github.com/xiaozhou0307/skyward) |
| Xiaomi 10 | `umi` | Tự đóng gói (chưa có `[stock]`) | [Alittlejelly/skyward-for-MI10](https://github.com/Alittlejelly/skyward-for-MI10) |
| Xiaomi 12S Pro | `unicorn` | `OS3.0.4.0.VLECNXM` | [Kinaxie/skyward](https://github.com/Kinaxie/skyward) |
| Xiaomi 11 | `venus` | Tự đóng gói (chưa có `[stock]`) | [cvbyisme/skyward-venus-archive](https://github.com/cvbyisme/skyward-venus-archive) |
| Redmi K70 | `vermeer` | `OS3.0.307.0.WNKCNXM` | [luoxunyong/skyward-by-Redmi-K70](https://github.com/luoxunyong/skyward-by-Redmi-K70) |
| Xiaomi Pad 7S Pro 12.5 | `violin` | `OS4.0.5.0.XOTCNXM` | [shu-chong/skyward-for-violin](https://github.com/shu-chong/skyward-for-violin) |
| REDMI K90 Ultra | `warsaw-4.0.4.0` | Tự đóng gói (chưa có `[stock]`) | [wjljh/skyward-for-warsaw](https://github.com/wjljh/skyward-for-warsaw) |
| REDMI K90 Ultra | `warsaw-4.0.8.0.nfchal` | `OS4.0.8.0.XHPCNXM` | [wjljh/skyward-for-warsaw](https://github.com/wjljh/skyward-for-warsaw) |
| REDMI K90 Ultra | `warsaw-4.0.8.0` | `OS4.0.8.0.XHPCNXM` | [wjljh/skyward-for-warsaw](https://github.com/wjljh/skyward-for-warsaw) |
| Xiaomi MI 6X | `wayne` | Tự đóng gói (chưa có `[stock]`) | [East888666/skyward-xiaomi-legacy](https://github.com/East888666/skyward-xiaomi-legacy) |
| Redmi Note 11T Pro | `xaga` | `OS1.0.3.0.ULOCNXM` | [BkaNeko/xaga-skyward](https://github.com/BkaNeko/xaga-skyward) |
| Samsung Galaxy S20+ 5G | `y2q` | Tự đóng gói (chưa có `[stock]`) | [yanzihan/Skyward-Galaxy](https://github.com/yanzihan/Skyward-Galaxy) |
| Xiaomi Pad 6 Max 14 | `yudi` | `OS3.0.6.0.VMHCNXM` | [HuberyBrook/skyward](https://github.com/HuberyBrook/skyward) |
| Xiaomi Pad 8 | `yupei` | `OS4.0.12.0.XPZCNXM` | [d2su8/skyward](https://github.com/d2su8/skyward) |
| Xiaomi MIX Fold 2 | `zizhan` | Tự đóng gói (chưa có `[stock]`) | [Hdy08/skyward](https://github.com/Hdy08/skyward) |
<!-- COMMUNITY-DEVICES-END -->

### Thêm thiết bị mới

GitHub Actions tự động phát hiện và đóng gói mọi tệp `devices/*.toml` có khai báo `[stock]` khi phát hành bản mới (hỗ trợ `recovery`, `vendor_boot`, giải mã Linux 4.19 / HIDL `keymaster@4.0`, rung preset-only và khởi động kép không cần `init_boot`). Để thêm một mẫu máy ngoài danh sách trên:

- Tạo tệp `devices/<codename>.toml` (khai báo màn hình, cảm ứng, module kernel, phân vùng, Wi-Fi, rung, sao lưu và đường dẫn log) cùng `[stock]` trỏ tới gói Full OTA gốc.
- Trích xuất mô tả init/VINTF từ gói OTA bằng `aweyuki-pack described` vào `crates/aweyuki-programs/tests/described/<codename>`.
- Đăng ký phiên bản gốc vào test trong `crates/aweyuki-config/src/lib.rs` và chạy `cargo test --workspace`.
- Đưa máy vào bảng `🚧 Đang hoàn thiện` (hoặc bảng chính sau khi đã xác minh đầy đủ trên máy thật).

Xem chi tiết tại [Hướng dẫn thêm thiết bị mới](docs/adding-devices.vi.md) ([English](docs/adding-devices.md)).

## Cài Aweyuki Recovery

Aweyuki cần bootloader đã mở khóa. Tải zip cho máy của bạn từ
[Releases](https://github.com/AweYuki/aweyuki_recovery/releases), mỗi bản ghi rõ mẫu
máy và tên mã, chẳng hạn Xiaomi 17 Ultra (nezha). Đối chiếu với `SHA256SUMS`, giải
nén, rồi flash ảnh bên trong vào phân vùng recovery của khe máy đang chạy, khi máy ở
chế độ fastboot:

```sh
unzip aweyuki-v0.5.1-nezha-OS4.0.4.0.XPACNXM.zip
fastboot flash recovery aweyuki-v0.5.1-nezha-OS4.0.4.0.XPACNXM.img
fastboot reboot recovery
```

Sau đó Aweyuki tự cập nhật qua Wi-Fi từ bản phát hành mới nhất, trong trang cập nhật
Aweyuki ở Cài đặt; một ảnh mới hơn, đã giải nén, cũng cài được từ trang Cài đặt của
nó. Khi bật **Flash lại Aweyuki**, một bản cập nhật hệ thống hay zip ghi đè lên các
phân vùng recovery sẽ được Aweyuki ghi trở lại vào đó.

adb trong Aweyuki là shell root không cần cấp quyền trên máy: ai có máy và cáp đều
truy cập được bộ nhớ của máy khi nó đang ở recovery.

## Cách khởi động kép hoạt động

Aweyuki cài một hệ thống thứ hai bên cạnh hệ thống trong các phân vùng của máy, dù đó
là HyperOS hay một ROM tùy chỉnh, và chuyển đổi giữa hai hệ thống từ **Khởi động kép**
ở trang chủ. Gốc của hệ thống thứ hai là một tệp trong phân vùng dữ liệu mà hệ thống tệp
không được phép di chuyển hay xóa, đọc và ghi xuyên qua chính phân vùng đó; ảnh khởi
động của cả hai hệ thống nằm trong một tệp như vậy khác. Mỗi lần chuyển chỉ ghi vào
các phân vùng boot và init_boot của khe khởi động kế tiếp, và ghi từng bước vào
/metadata trước, nên một lần chuyển bị mất điện cắt ngang sẽ được hoàn tất khi Aweyuki
khởi động lần sau. vendor_boot, dtbo, recovery, vbmeta, firmware và super giữ nguyên,
và hệ thống thứ hai khởi động bằng vendor_boot và dtbo của hệ thống đã cài.

Trước khi bất cứ thứ gì khác ghi vào các phân vùng boot, Aweyuki ghi lại ảnh khởi động
của hệ thống đã cài: một lần cài đặt, một lần chuyển khe, fastboot, khôi phục cài đặt
gốc, và sao lưu hay khôi phục các phân vùng boot. Hệ thống thứ hai chờ trong khi một
bản cập nhật hệ thống còn chờ hợp nhất, và bản sao lưu phân vùng dữ liệu bỏ nó ra ngoài.

Hệ thống thứ hai cài từ một zip ở trang Cài đặt, như mọi gói khác:

| Tệp | Nội dung |
| --- | --- |
| `system.toml` | `id`, `name`, `version` và `filesystem`; `init`, chương trình mà gốc khởi động bằng, `/sbin/init` nếu không ghi; `minimum`, dung lượng tối thiểu gốc cần, tính bằng byte |
| `boot.img` | Kernel mà bootloader khởi động |
| `rootfs.img` | Ảnh của hệ thống tệp gốc, theo `filesystem` đã ghi |
| `init_boot.img` | Chỉ cho hệ thống có ramdisk đầu tiên của riêng nó |

Một hệ thống không có init_boot riêng khởi động từ ảnh Aweyuki tạo. Chương trình trong
đó nạp các module kernel giai đoạn đầu của hệ thống đã cài, đúng như giai đoạn đầu
của hệ thống đó nạp, kèm phụ thuộc mềm và tham số dòng lệnh kernel dành cho chúng,
tìm gốc trong phân vùng dữ liệu, gắn kết và chạy `init` ở đó. Nếu có gì chặn nó, nó đưa
Aweyuki lên, như một lần khởi động hỏng của hệ thống đã cài kết thúc trong recovery:
nó để lý do trong misc, giải nén ramdisk của phân vùng recovery thuộc khe đè lên
ramdisk của chính nó, đúng như kernel giải nén khi bootloader khởi động recovery, rồi
chuyển cho chương trình đầu tiên của Aweyuki, nên Aweyuki vẫn lên dù bootloader xử lý
việc khởi động lại thế nào. Aweyuki hiện lý do, và hệ thống thứ hai vẫn khởi động ở
lần tới cho đến khi người dùng chuyển trong Khởi động kép. Một lần khởi động không
tiến triển thì thay vào đó khởi động lại vào recovery, và một máy bị khởi động lại
trước lúc bàn giao sẽ thấy cùng yêu cầu đó trong misc ở lần khởi động sau, đưa Aweyuki
lên trước mọi thứ khác. Khi Aweyuki không lên được bằng cả hai cách, máy dừng ở chỗ nó
dừng, và không khởi động lại cùng một hệ thống hết lần này đến lần khác. Một hệ thống
có ramdisk đầu tiên riêng tìm chương trình của Aweyuki ở `/aweyuki/map`, chương trình
này dựng gốc thành một thiết bị khối và in ra nút của nó. Mỗi lần chuyển sang hệ thống
thứ hai đều ghi ảnh init_boot của nó bằng chương trình của chính Aweyuki đang chuyển,
nên một Aweyuki đã cập nhật sẽ khởi động nó bằng chương trình mới.

Một hệ thống đang chạy chuyển đổi bằng cách ghi `boot-recovery` với
`--switch_system=installed`, hoặc `--switch_system=` và `id` của hệ thống thứ hai, vào
misc, rồi khởi động lại vào recovery. Khi bootloader khởi động lại chính khe đó, như
máy 13 Ultra có thể làm khi cắm cáp USB, chương trình của Aweyuki thấy yêu cầu trong
misc ở lần khởi động sau và đưa Aweyuki lên trước mọi thứ khác.

## Xây dựng

Aweyuki được xây bằng phiên bản Rust mà `rust-toolchain.toml` ghi, với target
`aarch64-linux-android`, Android NDK r29 và
[cargo-ndk](https://github.com/bbqsrc/cargo-ndk). Dịch vụ recovery được xây cho điện
thoại, còn công cụ đóng gói cho máy tính:

```sh
cargo ndk -t arm64-v8a --platform 30 build --release -p aweyuki
cargo build --release -p aweyuki-pack
```

Chương trình đầu tiên của hệ thống thứ hai chạy trước khi có bất kỳ thư viện nào để
nạp, nên được xây thành chương trình tĩnh:

```sh
RUSTFLAGS="-C target-feature=+crt-static" cargo ndk -t arm64-v8a --platform 30 build --release -p aweyuki-system-init --target-dir target/system-init
```

Công cụ đóng gói lấy phân vùng recovery gốc ra khỏi gói cập nhật đầy đủ của bản dựng
trên máy chủ Xiaomi, chỉ đọc phần của gói chứa phân vùng đó, và đối chiếu SHA-256 với
tệp thiết bị:

```sh
target/release/aweyuki-pack stock --config devices/nezha.toml --out target/stock/nezha/recovery.img
```

Rồi dựng ảnh trên nền nó:

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

Test và lint chạy trên máy tính; phần chỉ chạy trên điện thoại được lint theo target
điện thoại:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo ndk -t arm64-v8a --platform 30 clippy --workspace --all-targets -- -D warnings
```

Các test có tên bắt đầu bằng `on_the_phone` tạo hệ thống tệp, gắn kết và đặt khóa mã
hóa, những việc chỉ điện thoại làm được. Với một máy đang ở Aweyuki nối qua adb và
phân vùng dữ liệu đã mở, `cargo ndk` đẩy từng chương trình test sang máy và chạy ở đó,
mỗi lần một test:

```sh
cargo ndk -t arm64-v8a --platform 30 test -p aweyuki -p aweyuki-block -p aweyuki-files -p aweyuki-mtp on_the_phone -- --test-threads=1
```

`aweyuki-preview` vẽ các trang trên máy tính thành tệp PNG, không cần điện thoại: mỗi
trang trọn vẹn như ảnh chụp dài, ở mọi trạng thái có thể có, và màn hình tại những
thời điểm chạm và chuyển động. `--lang` chọn ngôn ngữ theo locale như `en`, `vi` hay
`zh-TW`:

```sh
cargo run --release -p aweyuki-preview -- target/preview --lang vi
```

### Bản dịch

Văn bản của mỗi ngôn ngữ nằm trong một tệp riêng tại `crates/aweyuki-ui/locales`, đặt tên theo mã vùng như `vi.toml`, `tr.toml`, `pt-BR.toml`. `en.toml` là tệp chuẩn đối chiếu. Quá trình biên dịch sẽ kiểm tra toàn bộ các tệp và dừng lại nếu thiếu khóa hoặc sai khác tên biến trong dấu ngoặc nhọn so với bản tiếng Anh.

Để sửa bản dịch, hãy chỉnh trực tiếp trong tệp ngôn ngữ tương ứng và chạy `aweyuki-preview --lang` để kiểm tra hiển thị: `cut short in` báo chữ bị cắt dấu ba chấm, `set at 80% in` báo chữ bị thu nhỏ để vừa nút/thanh trượt, `past the edge in` báo chữ tràn mép màn hình, và `overlap in` báo hai đoạn chữ đè lên nhau.

## Ghi công

- [Skyward](https://github.com/kirklin/skyward) (Kirk Lin) — kiến trúc recovery Rust và nền tảng giao diện ban đầu.
- [AERA Recovery](https://github.com/AERA-Recovery) — tham khảo thiết kế cho các tính năng EFISP, Root Manager, chia sẻ tệp qua mạng và runtime plugins.

## Giấy phép

[GPL-3.0-or-later](LICENSE)
