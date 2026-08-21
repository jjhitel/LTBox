//! Unroot worker: restore a stock root image — plus vbmeta when the root run
//! rebuilt it — from a backup folder over EDL. Extracted from the update_unroot
//! handler.

use crate::{
    ConnectionStatus, LiveLabels, PhaseReporter, UnrootType, find_edl_loader, open_edl_session,
    transition_to_edl,
};
use ltbox_core::{i18n::tr, live, tr_args};

use super::root_backup::{ROOT_BACKUP_MANIFEST_NAME, resolve_backup_contents};

pub(crate) fn unroot_worker(
    folder: String,
    unroot_type: UnrootType,
    loader_override: Option<String>,
    conn: ConnectionStatus,
    ll: LiveLabels,
    phases: PhaseReporter,
) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    let dir = std::path::Path::new(&folder);

    live!(log, "[Unroot] {}", phases.marker(1));

    let backup = resolve_backup_contents(dir, unroot_type).map_err(|error| {
        tr_args!(
            "err_unroot_backup_manifest_invalid",
            manifest = ROOT_BACKUP_MANIFEST_NAME,
            error = error
        )
    })?;
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
    // Names the images this run will write, so the pre-flight line and the
    // Phase 4 header agree with what actually gets flashed.
    let restored_label = if vbmeta_path.is_some() {
        tr_args!("live_unroot_backup_pair", root_image = root_image_name)
    } else {
        root_image_name.to_string()
    };
    live!(log, "[Unroot] {}", restored_label);

    // Slot resolution must succeed —
    // unroot writes the manifest-resolved root target +
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

    live!(log, "[Unroot] {} ({restored_label})", phases.marker(4));
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

    live!(log, "[Unroot] {}", phases.marker(5));
    session
        .reset(&mut log)
        .map_err(|e| tr_args!("err_reset_failed", error = e))?;
    live!(log, "[Unroot] {}", ll.unroot_completed);
    Ok(log)
}
