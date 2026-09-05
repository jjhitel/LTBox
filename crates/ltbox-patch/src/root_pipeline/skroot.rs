//! SKRoot Lite boot-image patching.
//!
//! The core SKRoot port emits kernel byte writes. This module wraps those
//! writes in the same boot-image lifecycle as the other root flows:
//! `magiskboot unpack boot.img` → patch extracted `kernel` → `magiskboot repack`.

use std::path::{Path, PathBuf};

use fs_err as fs;

use ltbox_core::i18n::tr;
use ltbox_core::{LtboxError, Result, tr_args};

use crate::boot;
use crate::skroot::patch_bytes::PatchBytes;
use crate::skroot::patch_plan::{SkrootCorePatchPlan, build_core_patch_plan};

const ROOT_KEY_LEN: usize = 48;
const ROOT_KEY_STORED_TEXT_LEN: usize = ROOT_KEY_LEN - 1;
const ROOT_KEY_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

pub fn patch_boot(work_dir: &Path, log: &mut Vec<String>) -> Result<PathBuf> {
    let boot_in = work_dir.join("boot.img");
    let kernel_path = work_dir.join("kernel");

    ltbox_core::live!(log, "[SKRoot] {}", tr("log_skroot_unpack_boot"));
    boot::unpack(&boot_in, work_dir)?;
    if !kernel_path.exists() {
        return Err(LtboxError::Patch(
            "magiskboot unpack did not produce a `kernel` file".into(),
        ));
    }

    let mut kernel = fs::read(&kernel_path)?;
    let plan = build_core_patch_plan(&kernel)
        .map_err(|e| LtboxError::Patch(format!("SKRoot patch plan: {e}")))?;
    ltbox_core::live!(
        log,
        "[SKRoot] {}",
        tr_args!(
            "log_skroot_patch_plan",
            version = plan.version.clone(),
            writes = plan.writes.len().to_string(),
            root_key_addr = format!("0x{:x}", plan.root_key_addr),
        )
    );

    ltbox_core::live!(log, "[SKRoot] {}", tr("log_skroot_patch_kernel"));
    apply_kernel_writes(&mut kernel, &plan)?;
    let root_key = generate_root_key()?;
    write_root_key(&mut kernel, plan.root_key_addr, &root_key)?;
    fs::write(&kernel_path, &kernel)?;
    ltbox_core::live!(
        log,
        "[SKRoot] {}",
        tr_args!("log_skroot_root_key", key = root_key)
    );

    ltbox_core::live!(log, "[SKRoot] {}", tr("log_skroot_repack_boot"));
    boot::repack("boot.img", work_dir)?;
    let new_boot = work_dir.join("new-boot.img");
    if !new_boot.exists() {
        return Err(LtboxError::BootImage(
            "magiskboot repack produced no new-boot.img".into(),
        ));
    }
    ltbox_core::live!(log, "[SKRoot] {}", tr("log_patch_complete"));
    Ok(new_boot)
}

fn apply_kernel_writes(kernel: &mut [u8], plan: &SkrootCorePatchPlan) -> Result<()> {
    for write in &plan.writes {
        apply_write(kernel, write)?;
    }
    Ok(())
}

fn write_root_key(kernel: &mut [u8], root_key_addr: u64, root_key: &str) -> Result<()> {
    let key_bytes = root_key.as_bytes();
    if key_bytes.len() != ROOT_KEY_STORED_TEXT_LEN {
        return Err(LtboxError::Patch(format!(
            "SKRoot root key must be {ROOT_KEY_STORED_TEXT_LEN} bytes (NUL-terminated image slot), got {}",
            key_bytes.len()
        )));
    }
    let mut stored = [0u8; ROOT_KEY_LEN];
    // Image slot is a 48-byte C string: 47 payload bytes + trailing NUL.
    stored[..ROOT_KEY_STORED_TEXT_LEN].copy_from_slice(key_bytes);
    apply_write(
        kernel,
        &PatchBytes {
            addr: root_key_addr,
            bytes: stored.to_vec(),
        },
    )
}

fn apply_write(kernel: &mut [u8], write: &PatchBytes) -> Result<()> {
    let start = usize::try_from(write.addr).map_err(|_| {
        LtboxError::Patch(format!(
            "SKRoot write address too large: 0x{:x}",
            write.addr
        ))
    })?;
    let end = start.checked_add(write.bytes.len()).ok_or_else(|| {
        LtboxError::Patch(format!(
            "SKRoot write overflows usize: 0x{:x}+{}",
            write.addr,
            write.bytes.len()
        ))
    })?;
    if end > kernel.len() {
        return Err(LtboxError::Patch(format!(
            "SKRoot write out of bounds: 0x{:x}+{} > {}",
            write.addr,
            write.bytes.len(),
            kernel.len()
        )));
    }
    kernel[start..end].copy_from_slice(&write.bytes);
    Ok(())
}

fn generate_root_key() -> Result<String> {
    // Generate the effective C-string payload only (47 chars). The image slot is
    // 48 bytes with a trailing NUL, so logging a 48-char key would not match.
    let mut out = String::with_capacity(ROOT_KEY_STORED_TEXT_LEN);
    let mut random = [0u8; 64];
    while out.len() < ROOT_KEY_STORED_TEXT_LEN {
        getrandom::fill(&mut random)
            .map_err(|e| LtboxError::Patch(format!("generate SKRoot root key: {e}")))?;
        for byte in random {
            let limit = (u8::MAX / ROOT_KEY_ALPHABET.len() as u8) * ROOT_KEY_ALPHABET.len() as u8;
            if byte >= limit {
                continue;
            }
            let idx = (byte as usize) % ROOT_KEY_ALPHABET.len();
            out.push(ROOT_KEY_ALPHABET[idx] as char);
            if out.len() == ROOT_KEY_STORED_TEXT_LEN {
                break;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{ROOT_KEY_LEN, ROOT_KEY_STORED_TEXT_LEN, generate_root_key, write_root_key};

    #[test]
    fn generated_root_key_matches_effective_c_string_len() {
        let key = generate_root_key().expect("key");
        assert_eq!(key.len(), ROOT_KEY_STORED_TEXT_LEN);
        assert!(key.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn root_key_storage_matches_upstream_c_string_slot() {
        let key = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTU".to_string();
        assert_eq!(key.len(), ROOT_KEY_STORED_TEXT_LEN);
        let mut kernel = vec![0xff; 96];

        write_root_key(&mut kernel, 16, &key).expect("write");

        assert_eq!(&kernel[16..16 + ROOT_KEY_STORED_TEXT_LEN], key.as_bytes());
        assert_eq!(kernel[16 + ROOT_KEY_STORED_TEXT_LEN], 0);
        assert_eq!(kernel[16 + ROOT_KEY_LEN], 0xff);
    }

    #[test]
    fn generated_and_logged_root_key_matches_image_c_string() {
        let key = generate_root_key().expect("key");
        let mut kernel = vec![0xff; 96];

        write_root_key(&mut kernel, 16, &key).expect("write");

        let stored = std::ffi::CStr::from_bytes_until_nul(&kernel[16..16 + ROOT_KEY_LEN])
            .expect("NUL-terminated key slot")
            .to_str()
            .expect("ascii key");
        assert_eq!(stored, key);
        assert_eq!(stored.len(), ROOT_KEY_STORED_TEXT_LEN);
        assert_eq!(kernel[16 + ROOT_KEY_STORED_TEXT_LEN], 0);
    }
}
