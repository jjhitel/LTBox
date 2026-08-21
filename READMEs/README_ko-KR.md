# LTBox

[🇺🇸 English](../README.md) / [🇨🇳 简体中文](README_zh-CN.md)

[![License: GPLv3][gpl-shield]][gpl]
[![Rust][rust-shield]][rust]
[![빌드][ci-shield]][ci]
[![최신 릴리스][release-shield]][releases]
[![다운로드][downloads-shield]][releases]

## ⚠️ 면책 조항

**교육 목적으로만 사용하세요.** 펌웨어를 수정하면 기기가 벽돌이 되거나 데이터가 사라지거나 보증이 무효화될 수 있습니다. 제작자는 **어떠한 책임도 지지 않으며**, 모든 책임은 사용자 본인에게 있습니다. **위험을 감수하고 사용하세요.**

---

## 🚀 빠른 시작

![Windows](https://img.shields.io/badge/Windows-0078D6?logo=windows&logoColor=white) ![Linux](https://img.shields.io/badge/Linux-FCC624?logo=linux&logoColor=black) ![macOS](https://img.shields.io/badge/macOS-000000?logo=apple&logoColor=white)

문서의 [**빠른 시작**](https://miner7222.github.io/ltbox/ko/index.html#windows)을 참고하세요.

---

## 📋 무엇을 할 수 있나요?

LTBox는 사이드바 중심의 데스크톱 GUI입니다. 각 항목을 열면 단계별 위저드가 안내합니다.

| 사이드바 항목 | 설명 |
|---|---|
| **대시보드** | 기기 상태, 지역, 최근 폴더, 원클릭 작업 |
| **펌웨어 플래싱** | 지역 → 대상 → 초기화/유지 → 플래싱을 한 흐름으로. 지역 변환과 롤백까지 한 번에 처리 |
| **시스템 업데이트** | OTA 업데이트 비활성화/활성화; 지역 변환된 기기가 OTA 후 부팅에 실패하면 **부팅 복구**로 되살리기 |
| **루팅** | KernelSU / KernelSU Next / SukiSU Ultra / ReSukiSU / APatch / FolkPatch / Magisk(+포크)로 루팅 |
| **언루팅** | 이전 루팅 백업에서 순정 부트 이미지 복원 |
| **GPU 클럭/전압** | KonaBess로 기기 GPU 테이블을 수정하고 AVB 보호 이미지를 다시 빌드해 플래싱 |
| **재부팅** | System / Recovery / Bootloader / EDL로 이동 |
| **고급** | 파이프라인 단계를 직접 하나씩 실행 — 아래 참조 |
| **설정** | 언어(en/ko/zh/ru/ja), 테마(시스템/라이트/다크), 강조 색상, 기본 EDL 로더 경로 |

### 고급

<details>
<summary>파이프라인 단계를 수동으로 제어, 세 섹션으로 구성</summary>

<br>

**지역/국가 수정**
- 지역 변환 — `vendor_boot` 지역 코드(PRC ↔ ROW)를 재작성하고 vbmeta 재빌드
- 국가 코드 변경 — 모델별 국가 파티션을 덤프·수정·플래싱

**AVB 이미지**
- 이미지 정보 — `.img` 파일의 AVB 메타데이터 표시
- 롤백 보호 감지 — 기기와 펌웨어의 롤백 인덱스 비교
- 롤백 보호 우회 — 체인 파티션 이미지의 롤백 인덱스 패치
- vbmeta 재빌드 — 해시 디스크립터를 갱신해 `vbmeta.img` 재빌드

**EDL 작업**
- X → XML 변환 — `.x` 펌웨어 파일을 rawprogram `.xml`로 복호화
- 파티션 읽기 / 쓰기 — 이름 기준으로 파티션 덤프/플래싱 (GPT-by-name)
- 물리 저장소 덤프 / 플래싱 — LUN 전체 덤프/플래싱
- 펌웨어 단순 플래싱 — 검사·수정 없이 플래싱만 (순정 플래시 스크립트에 최대한 가깝게)

</details>

---

## 🏗️ 프로젝트 구조

| 크레이트 | 역할 |
|---|---|
| `ltbox-core` | 프리미티브 — 에러, 설정, 로깅, HTTP 클라이언트(GitHub, nightly.link, Lenovo), 암호화, XML 복호화, 라이브 로그 싱크 |
| `ltbox-device` | 전송 계층 — ADB, Fastboot, EDL / QDL, 시리얼 포트 탐지, Windows 퀄컴 USB 드라이버 감지 + 자동 설치 |
| `ltbox-patch` | 이미지 파이프라인 — AVB(내장 AOSP testkey 스펙), 부트 이미지 ramdisk 패치, 지역 변환, 롤백 인덱스 처리, 루트 프로바이더 통합 |
| `ltbox-gui` | `iced` 데스크톱 앱 — `ltbox` 바이너리 빌드(Windows에서는 `ltbox.exe`) |

---

## 🙏 크레딧

- **익명의 [ㅇㅇ](https://gall.dcinside.com/board/lists?id=tabletpc)**
- **[갓파더](https://ppomppu.co.kr/zboard/view.php?id=androidtab&page=1&divpage=38&no=197457)**
- **[limzei89](https://note.com/limzei89/n/nd5217eb57827)**
- **[hitin911](https://xdaforums.com/m/hitin911.12861404/)**

---

## 💛 후원

LTBox는 개인 취미 프로젝트이며, 후원이나 스폰서 등 어떠한 금전적인 지원도 받지 않습니다. 대신 기여는 언제나 환영합니다. 이슈, 풀 리퀘스트, 번역 모두 큰 도움이 됩니다.

---

## 📄 라이선스

이 저작물은 [GPL-3.0-or-later][gpl] 라이선스를 따릅니다.

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
