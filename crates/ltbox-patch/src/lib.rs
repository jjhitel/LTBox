//! Patching engine — AVB, boot image, region patching.
//!
//! Wraps `avbtool-rs` + `magiskboot` library APIs in-process; no subprocesses.

pub mod abl_key;
pub mod apatch;
pub mod avb;
pub mod boot;
pub mod gki;
pub mod key_map;
pub mod konabess;
pub mod ksu;
pub mod magisk;
pub mod region;
pub mod rollback;
pub mod root_pipeline;
pub mod skroot;
pub(crate) mod zip_util;
