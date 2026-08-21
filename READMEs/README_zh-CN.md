# LTBox

[🇺🇸 English](../README.md) / [🇰🇷 한국어](README_ko-KR.md)

[![License: GPLv3][gpl-shield]][gpl]
[![Rust][rust-shield]][rust]
[![构建][ci-shield]][ci]
[![最新发布][release-shield]][releases]
[![下载量][downloads-shield]][releases]

## ⚠️ 免责声明

**仅供教育用途。** 修改固件可能导致设备变砖、数据丢失或保修失效。作者**不承担任何责任**，一切后果由用户自行承担。**使用风险自负。**

---

## 🚀 快速开始

![Windows](https://img.shields.io/badge/Windows-0078D6?logo=windows&logoColor=white) ![Linux](https://img.shields.io/badge/Linux-FCC624?logo=linux&logoColor=black) ![macOS](https://img.shields.io/badge/macOS-000000?logo=apple&logoColor=white)

请参阅英文文档中的 **[快速开始](https://miner7222.github.io/ltbox/en/index.html#windows)**。

---

## 📋 功能介绍

LTBox 是以侧边栏为核心的桌面 GUI，每个入口都会打开一个引导式向导。

| 侧边栏条目 | 说明 |
|---|---|
| **仪表盘** | 设备状态、区域、最近文件夹、一键操作 |
| **刷写固件** | 区域 → 目标 → 清除/保留 → 刷写一气呵成，区域转换与回滚全程自动处理 |
| **系统更新** | 禁用或重新启用 OTA 更新；**启动恢复**可救回区域转换后因 OTA 而无法启动的设备 |
| **获取 Root** | 使用 KernelSU / KernelSU Next / SukiSU Ultra / ReSukiSU / APatch / FolkPatch / Magisk（及分支）获取 Root |
| **取消 Root** | 从之前的 Root 备份恢复原始引导镜像 |
| **GPU 频率/电压** | 使用 KonaBess 修改设备 GPU 表，并重新构建和刷写受 AVB 保护的镜像 |
| **重启** | 跳转到 System / Recovery / Bootloader / EDL |
| **高级** | 手动逐步执行流水线步骤 — 见下方 |
| **设置** | 语言（en/ko/zh/ru/ja）、主题（系统/浅色/深色）、强调色、默认 EDL 加载器路径 |

### 高级

<details>
<summary>逐步手动控制流水线，分为三个部分</summary>

<br>

**区域/国家修改**
- 区域转换 — 改写 `vendor_boot` 区域码（PRC ↔ ROW）并重建 vbmeta
- 修补国家码 — 转储该机型的国家码分区，改写后刷写

**AVB 镜像**
- 获取镜像信息 — 显示一个或多个 `.img` 文件的 AVB 元数据
- 检测回滚保护 — 比较设备与固件的回滚索引
- 绕过回滚保护 — 修补链式分区镜像中的回滚索引
- 重建 vbmeta — 以更新后的哈希描述符重建 `vbmeta.img`

**EDL 操作**
- X 转 XML — 将 `.x` 固件文件解密为 rawprogram `.xml`
- 分区读取 / 写入 — 按名称导出或刷写分区（GPT-by-name）
- 物理存储导出 / 刷写 — 对整个 LUN 进行导出或刷写
- 固件简易刷写 — 仅刷写，不做检查或修改（尽量贴近原厂刷机脚本）

</details>

---

## 🏗️ 项目结构

| Crate | 职责 |
|---|---|
| `ltbox-core` | 基础原语 — 错误、设置、日志、HTTP 客户端（GitHub、nightly.link、联想）、加密、XML 解密、实时日志接收器 |
| `ltbox-device` | 传输层 — ADB、Fastboot、EDL / QDL、串口探测、Windows 高通 USB 驱动检测 + 自动安装 |
| `ltbox-patch` | 镜像流水线 — AVB（内置 AOSP testkey 规范）、引导镜像 ramdisk 补丁、区域转换、回滚索引处理、Root 方案集成 |
| `ltbox-gui` | `iced` 桌面应用 — 构建 `ltbox` 二进制（Windows 上为 `ltbox.exe`） |

---

## 🙏 致谢

- **Anonymous [ㅇㅇ](https://gall.dcinside.com/board/lists?id=tabletpc)**
- **[갓파더](https://ppomppu.co.kr/zboard/view.php?id=androidtab&page=1&divpage=38&no=197457)**
- **[limzei89](https://note.com/limzei89/n/nd5217eb57827)**
- **[hitin911](https://xdaforums.com/m/hitin911.12861404/)**

---

## 💛 支持

LTBox 是个人业余项目，不接受任何形式的资金支持 —— 不接受捐赠，也不接受赞助。但欢迎随时参与贡献：提交 issue、pull request 或翻译都会很有帮助。

---

## 📄 许可证

本作品基于 [GPL-3.0-or-later][gpl] 许可证发布。

[![GPLv3][gpl-image]][gpl]

[gpl]: https://www.gnu.org/licenses/gpl-3.0
[gpl-image]: https://www.gnu.org/graphics/gplv3-127x51.png
[gpl-shield]: https://img.shields.io/badge/License-GPLv3-blue.svg
[rust]: https://www.rust-lang.org
[rust-shield]: https://img.shields.io/badge/Rust-2024_edition-000000?logo=rust&logoColor=white
[ci]: https://github.com/miner7222/LTBox/actions/workflows/rust-ci.yml
[ci-shield]: https://img.shields.io/github/actions/workflow/status/miner7222/LTBox/rust-ci.yml?branch=main&label=build&logo=github
[releases]: https://github.com/miner7222/LTBox/releases/latest
[release-shield]: https://img.shields.io/github/v/release/miner7222/LTBox?logo=github
[downloads-shield]: https://img.shields.io/github/downloads/miner7222/LTBox/total?logo=github
