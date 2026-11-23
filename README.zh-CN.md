![Aweyuki Recovery](assets/hero.jpg)

# Aweyuki Recovery

[English](README.md) | 简体中文 | [Tiếng Việt](README.vi.md)

Aweyuki Recovery 是使用 Rust 从零编写的 Android recovery。

<p align="center">
  <img src="assets/screenshots/01-home.png" width="30%" alt="Aweyuki Recovery 主界面">
  <img src="assets/screenshots/22-settings-dark.png" width="30%" alt="深色模式下的设置">
  <img src="assets/zh-CN/time-zone.png" width="30%" alt="时区">
</p>

## 功能

### 双系统

<p align="center">
  <img src="assets/screenshots/02-dual-boot.png" width="32%" alt="双系统切换">
</p>

可在主系统（HyperOS 或第三方 ROM）旁安装并切换第二系统，切换过程由 `/metadata` 原子记录，仅改写 `boot` 与 `init_boot` 分区。第二系统的安装包格式和启动过程见下文的[双系统的工作方式](#双系统的工作方式)。

### 解密与恢复模式 PIN 锁

<p align="center">
  <img src="assets/screenshots/03-decrypt.png" width="32%" alt="使用锁屏密码解密 data 分区">
</p>

支持使用系统的锁屏图案、PIN 或密码解密 `/data` 分区，并可为 Recovery 本身设置独立的恢复模式 PIN 锁。

### 安装、OTA Payload 检查与批量刷入

<p align="center">
  <img src="assets/screenshots/04-install.png" width="32%" alt="安装界面">
  <img src="assets/screenshots/15-batch-flash.png" width="32%" alt="批量刷入">
</p>

- **安装来源与工具**：支持从内部存储、USB OTG 或 ADB Sideload 刷入 `.zip`、`.img`、`.efi` 及双系统安装包，支持 zip 签名校验。刷机脚本可使用 `7za`、`zstd`、流式 `unzip -p`、`avbctl` 和 `resetprop`。
- **OTA Payload 检查与 ARB 检测**：解析 `payload.bin` / OTA `.zip` 清单，执行防回滚（ARB）安全补丁与时间戳校验，并支持提取指定分区（`REPLACE`、`REPLACE_ZSTD`、`ZERO`、`DISCARD`）。
- **批量刷入与 EFISP**：支持带分区几何预校验的多镜像 `.img`/`.efi` 批量刷入，包括向非 A/B 的 `efisp` 分区直接刷入并校验 ARM64 `.efi` 镜像。

### 在线更新与 WLAN

<p align="center">
  <img src="assets/screenshots/05-system-updates.png" width="32%" alt="系统在线更新">
  <img src="assets/screenshots/21-wifi.png" width="32%" alt="WLAN 设置">
</p>

直接在 Recovery 中连接 2.4 GHz / 5 GHz WLAN，在线检查并安装系统更新（支持显示 A/B 合并状态）以及在线自更新 Aweyuki Recovery。

### 备份、恢复与应用保险库

<p align="center">
  <img src="assets/screenshots/06-backup.png" width="30%" alt="备份分区">
  <img src="assets/screenshots/07-restore.png" width="30%" alt="恢复备份">
  <img src="assets/screenshots/16-app-vault.png" width="30%" alt="应用保险库">
</p>

- **备份与恢复**：可将 `boot`、`system`、`data`、基带与 EFS 以及可选的 `efisp` 备份到内部存储或 USB OTG，支持压缩与校验。
- **应用保险库（App Vault）**：以 `.tar.lz4` 格式将单个用户应用备份和恢复至 `/sdcard/Aweyuki/app-vault`。

### 文件管理、挂载与终端

<p align="center">
  <img src="assets/screenshots/11-file-manager.png" width="30%" alt="文件管理">
  <img src="assets/screenshots/08-mount.png" width="30%" alt="分区挂载">
  <img src="assets/screenshots/12-terminal.png" width="30%" alt="Root 终端">
</p>

- **文件管理**：浏览 `/` 目录，显示文件元信息、符号链接目标以及终端快捷入口。
- **挂载与 USB**：手动挂载/卸载动态分区（`system`、`vendor`、`product`、`odm`、`mi_ext`、`system_ext`、`system_dlkm`），并管理 USB MTP 与 ADB 连接。
- **终端**：内置 Root Shell，配备屏幕虚拟按键（`Esc`、`Tab`、`Ctrl` 与方向键）。

### 清除数据、SELinux 与文件系统修复

<p align="center">
  <img src="assets/screenshots/09-wipe.png" width="30%" alt="清除数据">
  <img src="assets/screenshots/13-selinux.png" width="30%" alt="修复 SELinux 上下文">
  <img src="assets/screenshots/14-filesystem-repair.png" width="30%" alt="文件系统修复">
</p>

每一项清除操作均明确列出清除范围。支持修复 `/data` 的 SELinux 上下文、检查并修复 `/data`（`f2fs`）文件系统，以及对 `efisp` 和 `frp` 分区执行逐字节校验的全零擦除。

### Root 管理

<p align="center">
  <img src="assets/screenshots/17-root-manager.png" width="32%" alt="Root 管理">
</p>

自动解析内核版本与 KMI（如 `5.15.194` / `android13-5.15`），识别 Magisk、KernelSU 和 APatch 框架，并支持开关或标记卸载 `/data/adb/modules/*` 模块。

### 网络文件共享与远程投屏 (`/mirror`)

<p align="center">
  <img src="assets/screenshots/18-network-file-sharing.png" width="32%" alt="网络文件共享">
</p>

通过 Wi-Fi 启动 HTTP/1.1 文件共享服务，支持电脑浏览器上传/下载或 `curl -T`，同时提供 `/mirror` 远程实时投屏、触控/按键控制以及一键保存 PNG 截图。

### 运行时插件与 AERA `.aerap` 应用

<p align="center">
  <img src="assets/screenshots/19-plugins.png" width="32%" alt="插件与 AERA">
</p>

支持运行带实时流式输出与中止功能的 `plugin.toml` 脚本插件，并支持隔离沙箱化的 AERA `.aerap` GUI 运行时插件，包括 **AERA Browser**（WPE WebKit 浏览器）和 **AERA Telegram**（TDLib 客户端）。

### 高级工具、开发者选项与无音量键截图

<p align="center">
  <img src="assets/screenshots/10-advanced.png" width="32%" alt="高级菜单">
  <img src="assets/screenshots/20-developer-benchmark.png" width="32%" alt="开发者选项与性能测试">
</p>

- **高级工具中心**：集中提供搜索、日志（含近期刷写/擦除历史）、文件管理、终端、SELinux/文件系统修复、Root 管理、网络文件共享、插件和应用保险库。
- **开发者与存储基准测试**：查看和调整 `printk` 日志级别、SELinux 模式、USB/ADB 状态、CPU 调度器、内核 `cmdline`，并运行 64 MiB 顺序存储读写基准测试。
- **无音量键截图**：可通过高级菜单中的**延迟 3 秒截图**（截取前自动隐藏提示条）或在 `/mirror` 网页端一键保存 PNG 截图。

### 设置、关于本机与重启

<p align="center">
  <img src="assets/screenshots/23-about-specs.png" width="32%" alt="关于本机与硬件规格">
  <img src="assets/screenshots/24-reboot.png" width="32%" alt="重启选项">
</p>

- **语言与主题**：浅色与深色模式；15 种语言（简体中文、繁體中文、English、Türkçe、Русский、Українська、Español、Português (Brasil)、Bahasa Indonesia、Tiếng Việt、Deutsch、Français、Italiano、Polski 和日本語）；支持含越南时区（`Asia/Ho_Chi_Minh`, UTC+7）在内的 IANA 时区。
- **关于本机**：查看 Aweyuki 版本、存储空间、处理器、内存、电池健康度与充电循环次数。
- **重启与槽位切换**：重启至系统、Recovery、Fastboot、Bootloader、EDL 或关机，切换 A/B 启动槽位，并在重启前执行 ABL EFISP 加载器回归检查。

## 支持的机型

| 机型 | 代号 | 测试的系统版本 |
| --- | --- | --- |
| 小米 17 Ultra | `nezha` | OS4.0.4.0.XPACNXM（Android 17） |
| 小米 13 Ultra | `ishtar` | OS3.0.308.0.WMACNXM（Android 16） |
| 小米 13 Pro | `nuwa` | OS3.0.312.0.WMBCNXM（Android 16） |

### 🚧 施工中

这些机型的镜像按照各自的原厂固件构建，还没有在真机上试过。屏幕、触摸、WLAN 和振动已经配置好。解密 data 分区所需的程序由 Aweyuki 在手机上从已安装系统的 init 脚本和服务描述中读出，每个机型的读取结果都用原厂固件核对过，这些程序在这些机型上的运行情况还没有验证。

| 机型 | 代号 | 构建所用的系统版本 |
| --- | --- | --- |
| 🚧 小米 13 | `fuxi` | OS3.0.309.0.WMCCNXM（Android 16） |
| 🚧 小米 14 | `houji` | OS3.0.307.0.WNCCNXM（Android 16） |
| 🚧 小米 14 Pro | `shennong` | OS3.0.309.0.WNBCNXM（Android 16） |
| 🚧 小米 14 Ultra | `aurora` | OS3.0.308.0.WNACNXM（Android 16） |
| 🚧 小米 15 | `dada` | OS3.0.307.0.WOCCNXM（Android 16） |
| 🚧 小米 15 Pro | `haotian` | OS3.0.309.0.WOBCNXM（Android 16） |
| 🚧 小米 15 Ultra | `xuanyuan` | OS3.0.314.0.WOACNXM（Android 16） |
| 🚧 小米 17 | `pudding` | OS4.0.4.0.XPCCNXM（Android 17） |
| 🚧 小米 17 Pro | `pandora` | OS4.0.4.0.XBLCNXM（Android 17） |
| 🚧 小米 17 Pro Max | `popsicle` | OS4.0.4.0.XPBCNXM（Android 17） |
| 🚧 小米 17 Max | `byron` | OS3.0.310.0.WAFCNXM（Android 16） |
| 🚧 REDMI K80 | `zorn` | OS3.0.309.0.WOKCNXM（Android 16） |
| 🚧 REDMI K80 Pro | `miro` | OS3.0.305.0.WOMCNXM（Android 16） |
| 🚧 REDMI K90 | `annibale` | OS3.0.307.0.WPKCNXM（Android 16） |
| 🚧 REDMI K90 Pro Max | `myron` | OS4.0.4.0.XPMCNXM（Android 17） |
| 🚧 REDMI K90 Ultra | `warsaw` | OS3.0.309.0.WHPCNXM（Android 16） |
| 🚧 REDMI K100 Pro | `athens` | OS4.0.4.0.XPICNXM（Android 17） |
| 🚧 REDMI K100 Pro Max | `songyuan` | OS4.0.4.0.XGNCNXM（Android 17） |

### 添加新机型

GitHub Actions 会自动发现并构建 `devices/*.toml` 中的所有机型配置。要为以上列表之外的机型添加支持：

- 新建 `devices/<codename>.toml`（填写屏幕、触摸、内核模块、分区、WLAN、振动、备份与日志路径，以及 `[stock]` 官方完整更新包信息）。
- 使用 `aweyuki-pack described` 将官方包中的 init/VINTF 描述提取到 `crates/aweyuki-programs/tests/described/<codename>`。
- 在 `crates/aweyuki-config/src/lib.rs` 中登记 `[stock].version` 测试项并运行 `cargo test --workspace`。
- 在 README 支持列表中先加入 `🚧 施工中` 表格，在真机上验证完整后再移入主支持列表。

详细步骤请查看[添加新机型指南（英文）](docs/adding-devices.md)或[越南语版本](docs/adding-devices.vi.md)。

## 安装 Aweyuki Recovery

Aweyuki 需要已解锁的 bootloader。从 [Releases](https://github.com/AweYuki/aweyuki_recovery/releases) 下载对应机型的 zip，每个文件都标有机型和代号，例如 Xiaomi 17 Ultra (nezha)。使用 `SHA256SUMS` 核对并解压之后，在手机处于 fastboot 模式的时候，把其中的镜像刷入当前运行槽位的 recovery 分区：

```sh
unzip aweyuki-v0.4.0-nezha-OS4.0.4.0.XPACNXM.zip
fastboot flash recovery aweyuki-v0.4.0-nezha-OS4.0.4.0.XPACNXM.img
fastboot reboot recovery
```

之后连接 WLAN，就可以在“设置”的“Aweyuki 更新”页面更新到最新版本，也可以在 Aweyuki 自己的“安装”页面安装解压出来的新版镜像。打开 **自动重新刷入 Aweyuki** 之后，系统更新或者刷机包覆盖 recovery 分区的时候，Aweyuki 会重新刷回这些分区。

Aweyuki 中的 adb 是 root shell，手机上无需授权：手机处于 recovery 的时候，任何人拿到手机和数据线都能访问其中的存储。

## 双系统的工作方式

Aweyuki 可以在手机自身分区中的系统旁边安装第二系统，主系统是 HyperOS 或者第三方 ROM 都可以，并在首页的“双系统”中切换。第二系统的根文件系统是 data 分区中的一个文件，文件系统不能移动或删除它，Aweyuki 直接通过分区读写其中的内容；两个系统的启动镜像保存在另一个这样的文件中。切换系统只改写下次启动槽位的 boot 和 init_boot 分区，每一步都先记录在 /metadata 中，切换过程中断电的话，Aweyuki 下次启动时会完成这次切换。vendor_boot、dtbo、recovery、vbmeta、固件和 super 都保持不变，第二系统使用主系统的 vendor_boot 和 dtbo 启动。

其他操作写入启动分区之前，Aweyuki 会先把主系统的启动镜像写回：安装、切换启动槽位、fastboot、恢复出厂设置，以及备份或恢复启动分区。系统更新尚未合并的时候，暂不切换到第二系统；备份 data 分区的时候不包含第二系统。

第二系统的安装包是 zip 文件，和其他安装包一样在“安装”页面安装：

| 文件 | 内容 |
| --- | --- |
| `system.toml` | `id`、`name`、`version` 和 `filesystem`；`init` 是根文件系统中第一个运行的程序，默认为 `/sbin/init`；`minimum` 是根文件系统至少需要的空间，单位为字节 |
| `boot.img` | bootloader 启动的内核 |
| `rootfs.img` | 根文件系统的镜像，格式为 `filesystem` 指定的文件系统 |
| `init_boot.img` | 只有自带第一阶段 ramdisk 的系统需要 |

没有自带 init_boot 镜像的系统，从 Aweyuki 生成的 init_boot 镜像启动。其中的程序按照主系统第一阶段的方式加载第一阶段的内核模块，包括模块之间的软依赖和内核命令行中给模块的参数，然后在 data 分区中找到根文件系统，挂载之后运行其中的 `init`。启动过程中出现任何错误，这个程序都会进入 Aweyuki，和主系统启动失败时进入 recovery 一样：它在 misc 中留下原因，把当前槽位 recovery 分区中的 ramdisk 按照 bootloader 启动 recovery 时内核解包的方式解包到自己的 ramdisk 上，然后交给 Aweyuki 的第一个程序运行，所以无论 bootloader 怎样处理重启，都能进入 Aweyuki。Aweyuki 显示原因；用户在“双系统”中切换之前，下次启动的仍是第二系统。启动长时间没有进展时，这个程序改为重启到 recovery；交给第二系统之前手机重启的话，下次启动时它会在 misc 中看到同样的请求，先进入 Aweyuki。两种方式都无法进入 Aweyuki 时，手机停留在出错的地方，不会反复启动同一个系统。自带第一阶段 ramdisk 的系统可以运行 Aweyuki 放在 `/aweyuki/map` 的程序，它把根文件系统映射为块设备，并输出设备节点的路径。每次切换到第二系统，Aweyuki 都用自己带的程序写入第二系统的 init_boot 镜像，所以更新 Aweyuki 以后，第二系统也用新的程序启动。

正在运行的系统也可以请求切换：在 misc 中写入 `boot-recovery` 和 `--switch_system=installed`，或者 `--switch_system=` 加上第二系统的 `id`，然后重启到 recovery。bootloader 仍然启动这个槽位的话（小米 13 Ultra 连着 USB 线时可能这样），Aweyuki 的程序会在下次启动时在 misc 中看到这个请求，先进入 Aweyuki。

## 构建

Aweyuki 使用 `rust-toolchain.toml` 指定版本的 Rust 和它的 `aarch64-linux-android` 目标、Android NDK r29 和 [cargo-ndk](https://github.com/bbqsrc/cargo-ndk) 构建。recovery 服务针对手机构建，打包工具针对电脑构建：

```sh
cargo ndk -t arm64-v8a --platform 30 build --release -p aweyuki
cargo build --release -p aweyuki-pack
```

第二系统启动时运行的第一个程序在任何动态库可用之前运行，因此构建为静态程序：

```sh
RUSTFLAGS="-C target-feature=+crt-static" cargo ndk -t arm64-v8a --platform 30 build --release -p aweyuki-system-init --target-dir target/system-init
```

打包工具从小米服务器上这个版本的完整更新包中取出原厂 recovery 分区，只读取这个分区所在的部分，并且按照设备配置文件核对 SHA-256：

```sh
target/release/aweyuki-pack stock --config devices/nezha.toml --out target/stock/nezha/recovery.img
```

然后在它的基础上构建镜像：

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

测试和代码检查在电脑上运行；只在手机上运行的代码按照手机目标检查：

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo ndk -t arm64-v8a --platform 30 clippy --workspace --all-targets -- -D warnings
```

名称以 `on_the_phone` 开头的测试会创建文件系统、挂载分区、设置加密密钥，这些操作只有手机能够完成。把一台已经进入 Aweyuki、数据分区已经打开的手机通过 adb 连接到电脑，`cargo ndk` 会把每个测试程序推送到手机上运行，测试逐个执行：

```sh
cargo ndk -t arm64-v8a --platform 30 test -p aweyuki -p aweyuki-block -p aweyuki-files -p aweyuki-mtp on_the_phone -- --test-threads=1
```

`aweyuki-preview` 无需手机，在电脑上把页面绘制成 PNG 文件：每个页面完整绘制成长截图的样子，覆盖它可能处于的各种状态，另外还有触摸和动画过程中某些时刻的屏幕。`--lang` 按照地区代码选择语言，例如 `en` 或者 `zh-TW`：

```sh
cargo run --release -p aweyuki-preview -- target/preview --lang en
```

### 翻译

每种语言的文字是 `crates/aweyuki-ui/locales` 里的一个文件，以地区代码命名，例如 `tr.toml`、`pt-BR.toml`。`en.toml` 是参照文件，每条文字上方的注释说明它显示在哪里。构建时会读取全部文件，缺少某条文字，或者花括号里的名称和英文不一致，构建就会停止。

修正翻译时，直接修改对应语言文件里的文字。用 `aweyuki-preview --lang` 把页面按这种语言绘制出来检查效果：每绘制完一个页面，它会列出这个页面上显示不下的文字：`cut short in` 是以省略号结尾的文字，`set at 80% in` 是为了放进按键、按钮或滑动条而缩小了字号的文字，`past the edge in` 是超出屏幕的文字，`overlap in` 是互相重叠的两段文字。

## 致谢

- [Skyward](https://github.com/kirklin/skyward)（Kirk Lin）— 原始 Rust Recovery 架构与界面基础。
- [AERA Recovery](https://github.com/AERA-Recovery) — EFISP、Root Manager、网络文件共享与运行时插件的设计参考。

## 许可证

[GPL-3.0-or-later](LICENSE)
