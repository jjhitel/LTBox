# LTBox

[🇰🇷 한국어](READMEs/README_ko-KR.md) / [🇨🇳 简体中文](READMEs/README_zh-CN.md)

[![License: GPLv3][gpl-shield]][gpl]
[![Rust][rust-shield]][rust]
[![Build][ci-shield]][ci]
[![Latest release][release-shield]][releases]
[![Downloads][downloads-shield]][releases]

## ⚠️ Disclaimer

**Educational purposes only.** Modifying firmware can brick your device, cause data loss, or void your warranty. The developer assumes **no liability**. You are solely responsible. **Use at your own risk.**

---

## 🚀 Quick Start

![Windows](https://img.shields.io/badge/Windows-0078D6?logo=windows&logoColor=white) ![Linux](https://img.shields.io/badge/Linux-FCC624?logo=linux&logoColor=black) ![macOS](https://img.shields.io/badge/macOS-000000?logo=apple&logoColor=white)

See **[Quick Start](https://miner7222.github.io/ltbox/en/index.html#windows)** in the documentation.

---

## 📋 What Can It Do?

LTBox is a sidebar-driven desktop GUI; each entry opens a guided wizard.

| Sidebar entry | What it does |
|---|---|
| **Dashboard** | Device status, region, recent folders, one-click actions |
| **Flash Firmware** | One flow from region → target → wipe/keep → flash, with region conversion and rollback handled end-to-end |
| **System Updates** | Disable or re-enable OTA updates; **Boot Recovery** revives a region-converted device that won't boot after an OTA |
| **Root Device** | Root with KernelSU / KernelSU Next / SukiSU Ultra / ReSukiSU / APatch / FolkPatch / Magisk (+ forks) |
| **Unroot Device** | Restore the stock boot image from an earlier root backup |
| **GPU Clock/Voltage** | Edit the device GPU table with KonaBess, then rebuild and flash the AVB-protected images |
| **Reboot** | Jump to System, Recovery, Bootloader, or EDL |
| **Advanced** | Run individual pipeline steps by hand — see below |
| **Settings** | Language (en/ko/zh/ru/ja), theme (system/light/dark), accent color, default EDL loader path |

### Advanced

<details>
<summary>Step-by-step manual control over the pipeline, grouped into three sections</summary>

<br>

**Region/Country Edit**
- Convert Region — rewrite the `vendor_boot` region code (PRC ↔ ROW) and rebuild vbmeta
- Change Country Code — dump the model's country partitions, rewrite the code, flash

**AVB Image**
- Obtain Image Info — show AVB metadata for one or more `.img` files
- Detect Rollback Protection — compare the rollback index on the device against the firmware
- Bypass Rollback Protection — patch the rollback index in chained partition images
- Rebuild vbmeta — rebuild `vbmeta.img` with updated hash descriptors

**EDL Operations**
- Convert X to XML — decrypt `.x` firmware files to rawprogram `.xml`
- Read / Write Partitions — dump or flash partitions by name (GPT-by-name)
- Dump / Flash Physical Storage — dump or flash whole LUNs
- Firmware Simple Flasher — flash only, no checks or edits (as close as possible to the stock flash script)

</details>

---

## 🏗️ Project Layout

| Crate | Role |
|---|---|
| `ltbox-core` | Primitives — errors, settings, logging, HTTP clients (GitHub, nightly.link, Lenovo), crypto, XML decrypt, live-log sink |
| `ltbox-device` | Transport — ADB, Fastboot, EDL / QDL, serial-port discovery, Windows Qualcomm USB driver probe + auto-install |
| `ltbox-patch` | Image pipeline — AVB (bundled AOSP test-key specs), boot-image ramdisk patching, region conversion, rollback-index handling, root-provider integration |
| `ltbox-gui` | `iced` desktop app — builds the `ltbox` binary (`ltbox.exe` on Windows) |

---

## 🙏 Credits

- **Anonymous [ㅇㅇ](https://gall.dcinside.com/board/lists?id=tabletpc)**
- **[갓파더](https://ppomppu.co.kr/zboard/view.php?id=androidtab&page=1&divpage=38&no=197457)**
- **[limzei89](https://note.com/limzei89/n/nd5217eb57827)**
- **[hitin911](https://xdaforums.com/m/hitin911.12861404/)**
- **[corrupt3904](https://gall.dcinside.com/mgallery/board/view/?id=andtabcus&no=20290)**

---

## 💛 Support

LTBox is a personal hobby project and does not accept financial support of any kind — no donations, no sponsorships. Contributions, on the other hand, are always welcome: issues, pull requests, and translations all help.

---

## 📄 License

This work is licensed under [GPL-3.0-or-later][gpl].

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
