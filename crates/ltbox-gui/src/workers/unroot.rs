//! Unroot worker: restore a stock root image — plus vbmeta when the root run
//! rebuilt it — from a backup folder over EDL. Extracted from the update_unroot
//! handler.

use crate::{
    ConnectionStatus, LiveLabels, PhaseReporter, UnrootType, find_edl_loader, open_edl_session,
    root_skips_avb_postprocess, transition_to_edl,
};
use ltbox_core::{i18n::tr, live, tr_args};

use std::path::{Path, PathBuf};

use super::root_backup::{BackupContents, BackupRootTarget, resolve_backup_contents};

pub(crate) fn unroot_worker(
    folder: String,
    unroot_type: UnrootType,
    loader_override: Option<String>,
    device_model: String,
    conn: ConnectionStatus,
    ll: LiveLabels,
    phases: PhaseReporter,
) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    // Where an abort has to leave the device. Captured before `transition_to_edl`
    // moves it: a user who was already in EDL stays there, anyone else goes back
    // to system.
    let edl_start = matches!(conn, ConnectionStatus::Edl);
    if ltbox_core::model::is_xiaoxin_pro13_model(&device_model) {
        return Err(tr_args!("model_unsupported", model = "TB376FC / TB390FU"));
    }
    let dir = std::path::Path::new(&folder);

    live!(log, "[Unroot] {}", phases.marker(1));

    let backup = resolve_backup_contents(dir, unroot_type, &device_model)?;
    let root_target = backup.root_target;
    let root_image_name = root_target.filename();
    let base_part = root_target.partition_base();
    let root_image_path = dir.join(root_image_name);
    // A root run that left vbmeta untouched backed none up, and the stock
    // vbmeta is still on the device — restoring the root image alone puts the
    // pair back in sync.
    let vbmeta_path = backup.restore_vbmeta.then(|| dir.join("vbmeta.img"));
    if !root_image_path.exists() {
        return Err(tr_args!(
            "err_unroot_image_missing",
            image = root_image_name
        ));
    }
    if vbmeta_path.as_ref().is_some_and(|path| !path.exists()) {
        return Err(tr("err_unroot_vbmeta_missing"));
    }
    if let Ok(info) = ltbox_patch::avb::extract_image_avb_info(&root_image_path)
        && let Some(fingerprint) = ltbox_patch::avb::build_fingerprint(&info)
        // Bidirectional SKU equivalence makes the TB376FC token match TB390FU too.
        && ltbox_core::model::fingerprint_model_match(
            &fingerprint,
            ltbox_core::model::TB376FC_MODEL,
        )
    {
        return Err(tr_args!("model_unsupported", model = "TB376FC / TB390FU"));
    }
    // Names the images this run will write, so the pre-flight line and the
    // Phase 4 header agree with what actually gets flashed.
    let restored_label = if vbmeta_path.is_some() {
        tr_args!("live_unroot_backup_pair", root_image = root_image_name)
    } else {
        root_image_name.to_string()
    };
    live!(log, "[Unroot] {}", restored_label);

    // Slot resolution must succeed —
    // unroot writes the filename-resolved root target +
    // vbmeta_<slot> from the user's
    // backup folder. Defaulting to `_a`
    // when the device was on `_b`
    // restored stale stock blobs to the
    // wrong slot and left the active
    // slot still rooted, with no clear
    // signal to the user.
    let slot =
        ltbox_device::controller::poll_active_slot(std::time::Duration::from_secs(30), &mut log)
            .map_err(|e| tr_args!("err_unroot_slot_resolve_failed", error = e))?;

    // Decoupled loader — explicit picker /
    // Settings default takes priority. Fall back
    // to scanning the backup folder only when no
    // override was set, preserving v3-pre-decouple
    // behaviour for users who still ship a loader
    // alongside the backup images.
    let loader = match loader_override.clone() {
        Some(p) => std::path::PathBuf::from(p),
        None => find_edl_loader(dir)
            .or_else(|| dir.parent().and_then(find_edl_loader))
            .ok_or_else(|| tr_args!("err_unroot_loader_missing_under", path = dir.display()))?,
    };
    live!(
        log,
        "[Unroot] {}",
        tr_args!(
            "live_unroot_loader_path",
            path = loader.display().to_string()
        )
    );

    // The root image + vbmeta resolve through the
    // hardcoded LUN map; GPT-by-name reads
    // the slot's start sector from the
    // device. No rawprogram parse needed —
    // the loader's parent dir may not even
    // contain a firmware XML pair.
    let root_image_label = format!("{base_part}{slot}");
    let vbm_label = format!("vbmeta{slot}");
    let root_image_lun = ltbox_core::partition_lun::lun_for_partition(base_part)
        .ok_or_else(|| tr_args!("err_no_hardcoded_lun", partition = base_part))?;
    let vbm_lun = ltbox_core::partition_lun::lun_for_partition("vbmeta")
        .ok_or_else(|| tr_args!("err_no_hardcoded_lun", partition = "vbmeta"))?;
    if vbmeta_path.is_some() {
        live!(
            log,
            "[Unroot] {}",
            tr_args!(
                "log_unroot_lun_resolved",
                root_image_label = root_image_label,
                root_image_lun = root_image_lun,
                vbm_label = vbm_label,
                vbm_lun = vbm_lun,
            )
        );
    } else {
        live!(
            log,
            "[Unroot] {}",
            tr_args!(
                "log_unroot_lun_resolved_root_only",
                root_image_label = root_image_label,
                root_image_lun = root_image_lun,
            )
        );
    }

    live!(log, "[Unroot] {}", phases.marker(2));
    transition_to_edl(conn, &mut log)?;

    live!(log, "[Unroot] {}", phases.marker(3));
    let mut session = open_edl_session(&loader, &mut log)?;

    live!(log, "[Unroot] {}", phases.marker(4));
    // Nothing to compare on the testkey efisp/GBL route: its root run never ran
    // AVB post-processing, so the backup images hold no AVB metadata and the
    // pre-verification restore path is the correct one.
    let (root_image_path, vbmeta_path) = if root_skips_avb_postprocess(&device_model) {
        live!(log, "[Unroot] {}", tr("live_unroot_verify_skipped"));
        (root_image_path, vbmeta_path)
    } else {
        match verify_against_device(
            &mut session,
            backup,
            &slot,
            &root_image_path,
            vbmeta_path.as_deref(),
            &mut log,
        ) {
            Ok(verified) => verified,
            Err(error) => {
                if edl_start {
                    let _ = session.reset_to_edl(&mut log);
                } else {
                    let _ = session.reset(&mut log);
                }
                return Err(error);
            }
        }
    };

    live!(log, "[Unroot] {} ({restored_label})", phases.marker(5));
    session
        .flash_partition(
            &root_image_label,
            &root_image_path,
            0,
            root_image_lun,
            &mut log,
        )
        .map_err(|e| {
            tr_args!(
                "err_unroot_flash_failed",
                label = root_image_label,
                error = e
            )
        })?;
    if let Some(vbmeta_path) = &vbmeta_path {
        session
            .flash_partition(&vbm_label, vbmeta_path, 0, vbm_lun, &mut log)
            .map_err(|e| tr_args!("err_unroot_flash_failed", label = vbm_label, error = e))?;
    }

    live!(log, "[Unroot] {}", phases.marker(6));
    session
        .reset(&mut log)
        .map_err(|e| tr_args!("err_reset_failed", error = e))?;
    live!(log, "[Unroot] {}", ll.unroot_completed);
    Ok(log)
}

/// What the pre-flash comparison against the device concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnrootPrep {
    /// Device and backup agree — write the selected images untouched.
    FlashAsIs,
    /// Same firmware, but the device's `boot` sits at a different rollback
    /// index. Restore the backup image at the device's value instead.
    PatchRollback(u64),
    Abort(AbortReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AbortReason {
    /// A build fingerprint is missing on one side, so "same firmware" cannot
    /// be proven either way.
    FingerprintUnreadable,
    /// Both fingerprints read, and they name different builds.
    FingerprintMismatch,
}

/// Decide what to do with the selected backup image given what the device
/// actually holds. Pure, so the brick-critical policy is testable without an
/// EDL session.
///
/// A user who restores from *some* folder of the same firmware version rather
/// than their own backup gets images that never received this device's
/// rollback-bypass treatment; flashing those unchanged is what bricks the
/// device. Only `boot` and `vbmeta_system` carry a rollback index and unroot
/// never writes the latter, so an `init_boot` restore has nothing to reconcile
/// once the fingerprints agree.
fn unroot_prep(
    target: BackupRootTarget,
    device_fingerprint: Option<&str>,
    image_fingerprint: Option<&str>,
    device_index: u64,
    image_index: u64,
) -> UnrootPrep {
    let (Some(device_fingerprint), Some(image_fingerprint)) =
        (device_fingerprint, image_fingerprint)
    else {
        return UnrootPrep::Abort(AbortReason::FingerprintUnreadable);
    };
    if device_fingerprint != image_fingerprint {
        return UnrootPrep::Abort(AbortReason::FingerprintMismatch);
    }
    if target == BackupRootTarget::Boot && device_index != image_index {
        return UnrootPrep::PatchRollback(device_index);
    }
    UnrootPrep::FlashAsIs
}

/// Dump the partition this run would overwrite, compare it against the selected
/// backup, and return the images to actually flash — the originals when they
/// already agree with the device, patched copies when they do not.
///
/// Fails closed: a partition that will not dump or parse leaves the device's
/// rollback state unknown, which is exactly the case this check exists to keep
/// from being flashed blind.
fn verify_against_device(
    session: &mut ltbox_device::edl::EdlSession,
    backup: BackupContents,
    slot: &str,
    root_image_path: &Path,
    vbmeta_path: Option<&Path>,
    log: &mut Vec<String>,
) -> Result<(PathBuf, Option<PathBuf>), String> {
    let base = backup.root_target.partition_base();
    let device_label = format!("{base}{slot}");
    let lun = ltbox_core::partition_lun::lun_for_partition(base)
        .ok_or_else(|| tr_args!("err_no_hardcoded_lun", partition = base))?;

    let work_dir = ltbox_core::app_paths::work_dir_for("unroot_verify");
    let _ = std::fs::remove_dir_all(&work_dir);
    std::fs::create_dir_all(&work_dir)
        .map_err(|error| tr_args!("err_arb_work_dir_failed", error = error))?;

    live!(
        log,
        "[Unroot] {}",
        tr_args!("live_unroot_verify_dump", label = device_label)
    );
    let device_info = super::flash::dump_avb_info(session, &device_label, lun, &work_dir, log)
        .ok_or_else(|| tr_args!("err_unroot_device_dump_failed", label = device_label))?;
    let image_info =
        ltbox_patch::avb::extract_image_avb_info(root_image_path).map_err(|error| {
            tr_args!(
                "err_unroot_image_avb_failed",
                image = backup.root_target.filename(),
                error = error.to_string()
            )
        })?;

    let device_fingerprint = ltbox_patch::avb::build_fingerprint(&device_info);
    let image_fingerprint = ltbox_patch::avb::build_fingerprint(&image_info);
    let target_index = match unroot_prep(
        backup.root_target,
        device_fingerprint.as_deref(),
        image_fingerprint.as_deref(),
        device_info.rollback_index,
        image_info.rollback_index,
    ) {
        UnrootPrep::Abort(AbortReason::FingerprintUnreadable) => {
            return Err(tr("err_unroot_fingerprint_unreadable"));
        }
        UnrootPrep::Abort(AbortReason::FingerprintMismatch) => {
            return Err(tr_args!(
                "err_unroot_fingerprint_mismatch",
                device = device_fingerprint.unwrap_or_default(),
                image = image_fingerprint.unwrap_or_default()
            ));
        }
        UnrootPrep::FlashAsIs => {
            live!(
                log,
                "[Unroot] {}",
                tr_args!(
                    "live_unroot_verify_match",
                    label = device_label,
                    index = device_info.rollback_index.to_string()
                )
            );
            return Ok((
                root_image_path.to_path_buf(),
                vbmeta_path.map(Path::to_path_buf),
            ));
        }
        UnrootPrep::PatchRollback(index) => index,
    };

    live!(
        log,
        "[Unroot] {}",
        tr_args!(
            "live_unroot_rollback_patch",
            image = backup.root_target.filename(),
            from = image_info.rollback_index.to_string(),
            to = target_index.to_string()
        )
    );
    let patched = work_dir.join(backup.root_target.filename());
    let patch_failed = |error: String| {
        tr_args!(
            "err_unroot_rollback_patch_failed",
            image = backup.root_target.filename(),
            error = error
        )
    };
    std::fs::copy(root_image_path, &patched).map_err(|error| patch_failed(error.to_string()))?;
    // Same key policy the firmware flash uses for its ARB overlays: the stock
    // signer has to resolve in KEY_MAP, or the restored image is one nothing
    // verifies.
    let key =
        ltbox_patch::key_map::key_spec_for_signed_pubkey(image_info.public_key_sha1.as_deref())
            .map_err(|sha1| ltbox_patch::key_map::unresolved_signing_key_error(base, &sha1))?;
    // A `NONE` footer is hashed by vbmeta rather than self-signed; re-adding the
    // footer rewrites only the metadata appended after the payload.
    if image_info.algorithm == "NONE" {
        ltbox_patch::avb::add_hash_footer(&patched, &image_info, key, Some(target_index))
            .map_err(|error| patch_failed(error.to_string()))?;
    } else if let Some(spec) = key {
        ltbox_patch::avb::resign_image(&patched, spec, &image_info.algorithm, Some(target_index))
            .map_err(|error| patch_failed(error.to_string()))?;
    } else {
        return Err(patch_failed("unsigned image".to_string()));
    }

    // The root image just changed, so a vbmeta that hashes it (TB320FC family)
    // has to adopt the new descriptor — the same rebuild the root run does after
    // patching the ramdisk. A vbmeta that only chains the target pins the key
    // rather than the digest and is never in the backup to begin with.
    let Some(vbmeta_src) = vbmeta_path else {
        return Ok((patched, None));
    };
    let vbmeta_info = ltbox_patch::avb::extract_image_avb_info(vbmeta_src).map_err(|error| {
        tr_args!(
            "err_unroot_image_avb_failed",
            image = "vbmeta.img",
            error = error.to_string()
        )
    })?;
    let vbmeta_out = work_dir.join("vbmeta.img");
    let rebuild_failed =
        |error: String| tr_args!("err_unroot_vbmeta_rebuild_failed", error = error);
    match ltbox_patch::key_map::key_spec_for_signed_pubkey(vbmeta_info.public_key_sha1.as_deref())
        .map_err(|sha1| ltbox_patch::key_map::unresolved_signing_key_error("vbmeta.img", &sha1))?
    {
        Some(key) => {
            ltbox_patch::avb::rebuild_vbmeta_with_partition_descriptors(
                &vbmeta_out,
                vbmeta_src,
                &[patched.as_path()],
                key,
                None,
            )
            .map_err(|error| rebuild_failed(error.to_string()))?;
            let footer = ltbox_patch::avb::hash_descriptor(&patched, base)
                .map_err(|error| rebuild_failed(error.to_string()))?;
            let adopted = ltbox_patch::avb::hash_descriptor(&vbmeta_out, base)
                .map_err(|error| rebuild_failed(error.to_string()))?;
            if footer != adopted {
                return Err(rebuild_failed(format!(
                    "rebuilt vbmeta descriptor for {base} does not match the restored image"
                )));
            }
            live!(log, "[Unroot] {}", tr("live_unroot_vbmeta_rebuilt"));
        }
        // Unsigned vbmeta: NONE-algorithm bootloaders skip verification, so the
        // stock blob passes through exactly as the root pipeline sends it.
        None => {
            std::fs::copy(vbmeta_src, &vbmeta_out)
                .map_err(|error| rebuild_failed(error.to_string()))?;
        }
    }
    Ok((patched, Some(vbmeta_out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FP: &str = "qti/TB320FC/TB320FC:14/UP1A/S000123:user/release-keys";

    #[test]
    fn matching_fingerprint_and_index_flashes_the_backup_untouched() {
        for target in [BackupRootTarget::Boot, BackupRootTarget::InitBoot] {
            assert_eq!(
                unroot_prep(target, Some(FP), Some(FP), 4, 4),
                UnrootPrep::FlashAsIs
            );
        }
    }

    #[test]
    fn a_different_fingerprint_aborts() {
        assert_eq!(
            unroot_prep(
                BackupRootTarget::Boot,
                Some(FP),
                Some("qti/TB320FC/TB320FC:14/UP1A/S000999:user/release-keys"),
                4,
                4
            ),
            UnrootPrep::Abort(AbortReason::FingerprintMismatch)
        );
    }

    #[test]
    fn an_unreadable_fingerprint_aborts_on_either_side() {
        assert_eq!(
            unroot_prep(BackupRootTarget::Boot, None, Some(FP), 4, 4),
            UnrootPrep::Abort(AbortReason::FingerprintUnreadable)
        );
        assert_eq!(
            unroot_prep(BackupRootTarget::Boot, Some(FP), None, 4, 4),
            UnrootPrep::Abort(AbortReason::FingerprintUnreadable)
        );
    }

    #[test]
    fn a_boot_restore_adopts_the_device_rollback_index() {
        // The bricking case: a firmware-stock backup sitting below the device's
        // bypassed floor.
        assert_eq!(
            unroot_prep(BackupRootTarget::Boot, Some(FP), Some(FP), 7, 4),
            UnrootPrep::PatchRollback(7)
        );
        // And the reverse — the restored image must claim exactly what the
        // device holds, not merely "at least" it.
        assert_eq!(
            unroot_prep(BackupRootTarget::Boot, Some(FP), Some(FP), 4, 7),
            UnrootPrep::PatchRollback(4)
        );
    }

    #[test]
    fn an_init_boot_restore_never_patches() {
        // init_boot carries no rollback index; whatever the footers say, there
        // is nothing to reconcile.
        assert_eq!(
            unroot_prep(BackupRootTarget::InitBoot, Some(FP), Some(FP), 7, 4),
            UnrootPrep::FlashAsIs
        );
    }
}
