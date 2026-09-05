//! KernelSU LKM patching — replaces `init` in the selected root ramdisk with
//! KernelSU's bootstrap binary and stages `kernelsu.ko` so the stock kernel
//! loads the module at boot. Works for KernelSU, KSU-Next, and forks.

use std::path::{Path, PathBuf};

use ltbox_core::i18n::tr;
use ltbox_core::{LtboxError, Result, tr_args};

use crate::boot;
use crate::root_pipeline::RootImageTarget;

/// Patch the selected root image with KernelSU. `work_dir` must contain the
/// image, `init` (ksuinit), and `kernelsu.ko`.
/// Writes `work_dir/new-boot.img`; caller handles AVB resign + flash.
pub fn patch_root_image(
    work_dir: &Path,
    target: RootImageTarget,
    log: &mut Vec<String>,
) -> Result<PathBuf> {
    let img_name = target.filename();
    let img_path = work_dir.join(img_name);
    if !img_path.exists() {
        return Err(LtboxError::Patch(format!(
            "{img_name} not found in {}",
            work_dir.display()
        )));
    }
    for needed in ["init", "kernelsu.ko"] {
        if !work_dir.join(needed).exists() {
            return Err(LtboxError::Patch(format!(
                "KSU payload '{needed}' missing from {}",
                work_dir.display()
            )));
        }
    }

    ltbox_core::live!(
        log,
        "[KSU] {}",
        tr_args!("log_root_unpack_image", image = img_name)
    );
    boot::unpack(&img_path, work_dir)?;

    let ramdisk_name = boot::root_ramdisk_name(work_dir)?;

    // Refuse to double-patch: init.real only exists after a prior KSU run.
    let existing_real = boot::cpio(work_dir, ramdisk_name, &["exists init.real"])?;
    if existing_real == 0 {
        return Err(LtboxError::Patch(format!(
            "{img_name} is already KernelSU-patched — flash stock first"
        )));
    }

    // Move stock init → init.real so ksuinit can chain to it. Loose-ramdisk
    // images have no top-level init, so skip the rename there.
    let has_init = boot::cpio(work_dir, ramdisk_name, &["exists init"])?;
    if has_init == 0 {
        ltbox_core::live!(log, "[KSU] {}", tr("log_ksu_cpio_mv_init"));
        boot::cpio_checked(work_dir, ramdisk_name, &["mv init init.real"])?;
    } else {
        ltbox_core::live!(log, "[KSU] {}", tr("log_ksu_no_stock_init"));
    }

    ltbox_core::live!(log, "[KSU] {}", tr("log_ksu_cpio_add"));
    boot::cpio_checked(work_dir, ramdisk_name, &["add 0755 init init"])?;
    boot::cpio_checked(
        work_dir,
        ramdisk_name,
        &["add 0755 kernelsu.ko kernelsu.ko"],
    )?;

    ltbox_core::live!(
        log,
        "[KSU] {}",
        tr_args!("log_root_repack_image", image = img_name)
    );
    boot::repack(img_name, work_dir)?;

    let new_boot = work_dir.join("new-boot.img");
    if !new_boot.exists() {
        return Err(LtboxError::Patch(
            "magiskboot repack produced no new-boot.img".into(),
        ));
    }
    ltbox_core::live!(log, "[KSU] {}", tr("log_patch_complete"));
    Ok(new_boot)
}
