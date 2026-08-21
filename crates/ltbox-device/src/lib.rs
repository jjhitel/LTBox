//! Device communication without external executables.
//!
//! - ADB via `adb_client`
//! - Fastboot via `nusb` (minimal protocol)
//! - EDL via `qdl`

pub mod adb;
pub mod controller;
pub mod driver;
pub mod edl;
pub mod fastboot;
