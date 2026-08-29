//! Root worker: build patched root artifacts (Magisk / KernelSU
//! / APatch / GKI), flash them over EDL, and stage the manager APK.
//! Extracted from the update_root handler.

use crate::{
    ConnectionStatus, Family, LiveLabels, PhaseReporter, Provider, RootMode, VerChoice,
    fingerprint_token_match, fingerprint_uses_efisp_gbl_route, install_root_manager_apk,
    open_edl_session, prepare_tb323fu_efisp, provision_tb323fu_efisp,
    stage_manager_apk_for_manual_install, transition_to_edl, wait_and_install_root_manager_apk,
};
use ltbox_core::{i18n::tr, live, tr_args};

use super::root_backup::{ROOT_BACKUP_MANIFEST_NAME, write_root_backup_manifest};

fn fingerprint_matches_detected_model(fingerprint: &str, device_model: &str) -> bool {
    fingerprint_token_match(fingerprint, device_model)
}

/// The provider/version pair the pipeline runs with.
///
/// Three routes reach the confirm step without the user ever seeing a version
/// card, so none of them may demand one:
///
/// * **GKI** — the AnyKernel3 zip is the whole input.
/// * **SKRoot** — always the latest Lite release.
/// * **Magisk forks** — the wizard puts an APK picker where the version step
///   would be, and the pipeline ignores the channel for this provider anyway
///   (`provider_repo` has no slug for it and nightly resolution returns early).
///
/// The first two substitute `Magisk` to pick magiskboot as the unpack/repack
/// backend. Forks keep their own provider, and keep whatever channel happened
/// to be selected before the user switched to them.
fn resolve_provider_version(
    is_gki_route: bool,
    is_skroot_route: bool,
    provider: Option<Provider>,
    version: Option<VerChoice>,
) -> Result<(Provider, VerChoice), String> {
    if is_gki_route || is_skroot_route {
        return Ok((Provider::Magisk, VerChoice::Stable));
    }
    let provider = provider.ok_or_else(|| tr("err_root_provider_missing"))?;
    let version = if provider == Provider::MagiskForks {
        version.unwrap_or(VerChoice::Stable)
    } else {
        version.ok_or_else(|| tr("err_root_version_missing"))?
    };
    Ok((provider, version))
}

// The params are the closure's captured locals, threaded through verbatim
// from the update_root handler; bundling them into a struct would only move the
// noise. Extraction is mechanical, so keep the 1:1 capture->param mapping.
#[allow(clippy::too_many_arguments)]
pub(crate) fn root_worker(
    family: Option<Family>,
    mode: Option<RootMode>,
    provider: Option<Provider>,
    version: Option<VerChoice>,
    file_path: Option<String>,
    gui_kernel_version: Option<String>,
    device_model: String,
    conn: ConnectionStatus,
    fw_folder: Option<String>,
    kpm_paths: Vec<std::path::PathBuf>,
    superkey: String,
    nightly_run_id: Option<u64>,
    preinit_device: String,
    ll: LiveLabels,
    phases: PhaseReporter,
) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    let skip_adb = conn.skip_adb();

    // GKI route: AnyKernel3 zip is the full input —
    // no provider / version / GitHub fetch.
    let is_gki_route = mode == Some(RootMode::Gki);
    let family = family.ok_or_else(|| tr("err_root_family_missing"))?;
    let is_skroot_route = family == Family::Skroot;
    let (provider, version) =
        resolve_provider_version(is_gki_route, is_skroot_route, provider, version)?;

    use ltbox_patch::root_pipeline::{
        RootFamily, RootPipelineConfig, RootProvider, RootVersion, build_patched_artifacts,
        ensure_nightly_run_id, resolve_root_image_target, root_run_rebuilds_vbmeta,
        stage_root_manager_apk, stage_root_payload,
    };

    let pipe_family = match family {
        Family::Magisk => RootFamily::Magisk,
        Family::KernelSU => RootFamily::KernelSU,
        Family::APatch => RootFamily::APatch,
        Family::Skroot => RootFamily::Skroot,
    };
    let pipe_provider = if is_skroot_route {
        RootProvider::Skroot
    } else {
        match provider {
            Provider::Magisk => RootProvider::Magisk,
            Provider::MagiskForks => RootProvider::MagiskFork,
            Provider::KernelSU => RootProvider::KernelSU,
            Provider::KernelSUNext => RootProvider::KernelSUNext,
            Provider::SukiSU => RootProvider::SukiSU,
            Provider::ReSukiSU => RootProvider::ReSukiSU,
            Provider::APatch => RootProvider::APatch,
            Provider::FolkPatch => RootProvider::FolkPatch,
        }
    };
    let pipe_version = match version {
        VerChoice::Stable => RootVersion::Stable,
        VerChoice::Nightly => RootVersion::Nightly,
    };
    let file_path_buf: Option<std::path::PathBuf> =
        file_path.as_ref().map(std::path::PathBuf::from);
    let root_image_target = resolve_root_image_target(pipe_family, is_gki_route, &device_model);
    // Whether vbmeta is dumped, patched, and flashed at all. Resolved here,
    // beside the target, because the model is only known before EDL.
    let rebuild_vbmeta = root_run_rebuilds_vbmeta(root_image_target, &device_model);

    let loader_path = fw_folder.ok_or_else(|| tr("err_root_loader_not_selected"))?;
    let loader = std::path::PathBuf::from(&loader_path);
    if !loader.is_file() {
        return Err(tr_args!("err_root_loader_missing", path = loader.display()));
    }
    // Accept single-blob loaders (`.melf` / `.mbn` /
    // `.elf`), the `.xml` multi-image manifest, or its
    // encrypted `.x` form (TB323FU; decrypted in
    // `EdlSession::open`). Filename is free-form.
    let loader_ok = loader
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            let l = e.to_ascii_lowercase();
            l == "melf" || l == "mbn" || l == "elf" || l == "xml"
        })
        || ltbox_core::sahara_xml::is_encrypted_manifest_filename(&loader);
    if !loader_ok {
        return Err(tr_args!(
            "err_root_loader_invalid_ext",
            path = loader.display()
        ));
    }
    // Signing key: pipeline resolves via KEY_MAP
    // + `public_key_sha1`; PEM is `include_str!`'d
    // in avbtool-rs. No on-disk key consulted here.
    ltbox_core::live!(
        log,
        "[Root] {}",
        tr_args!("log_root_loader", path = loader.display().to_string())
    );

    let base = ltbox_core::app_paths::work_dir_for("root");
    let work_dir = base.join("work");
    let output_dir = base.join("out");
    let _ = std::fs::remove_dir_all(&work_dir);
    std::fs::create_dir_all(&work_dir)
        .map_err(|e| tr_args!("err_root_work_dir_failed", error = e))?;
    std::fs::create_dir_all(&output_dir)
        .map_err(|e| tr_args!("err_root_output_dir_failed", error = e))?;

    // Phase 1/8 — Inspect the device, slot, and kernel.
    // Front-loaded so the user sees something happen
    // before the long manager-APK / payload download.
    live!(log, "[Root] {}", phases.marker(1));
    // Slot detection MUST succeed — root flashes
    // the resolved root target + vbmeta_<slot>,
    // and silently defaulting to `_a` previously
    // landed flashes on the wrong slot when the
    // device was actually running on `_b`. Poll
    // both ADB + Fastboot up to 30 s; on failure,
    // the helper returns a diagnostic that names
    // which transport last failed and what to do
    // (re-plug into normal/recovery, reboot to
    // bootloader, fix unauthorized ADB, …).
    let slot_suffix =
        ltbox_device::controller::poll_active_slot(std::time::Duration::from_secs(30), &mut log)
            .map_err(|e| e.to_string())?;
    // Kernel version probe (KSU LKM) needs ADB
    // shell; runs only when ADB is currently
    // usable so the slot-resolved-via-Fastboot
    // path doesn't waste 30 s waiting for a
    // shell that won't come.
    let mut kernel_version: Option<String> = gui_kernel_version.clone();
    let mut adb_ready_at_start = false;
    if !skip_adb && let Some(mut adb) = ltbox_device::adb::AdbManager::new_if_connected() {
        adb_ready_at_start = true;
        if mode == Some(RootMode::Lkm) {
            if let Ok(Some(kv)) = adb.get_kernel_version() {
                let normalized = ltbox_patch::root_pipeline::normalize_ksu_kernel_version(&kv);
                live!(
                    log,
                    "[ADB] {}",
                    tr_args!(
                        "live_adb_kernel_version",
                        version = normalized.as_deref().unwrap_or(&kv)
                    )
                );
                if let Some(kv) = normalized {
                    kernel_version = Some(kv);
                }
            } else {
                live!(log, "[ADB] {}", ll.adb_no_kver);
            }
        }
    }
    if mode == Some(RootMode::Lkm) && kernel_version.is_none() {
        return Err(tr("err_ksu_lkm_kernel_version_required"));
    }

    let mut manager_cfg = RootPipelineConfig {
        family: pipe_family,
        provider: pipe_provider,
        version: pipe_version,
        root_image_target,
        rebuild_vbmeta,
        work_dir: work_dir.clone(),
        output_dir: output_dir.clone(),
        loader: loader.clone(),
        slot_suffix: slot_suffix.clone(),
        preinit_device: preinit_device.clone(),
        kernel_version: kernel_version.clone(),
        gki_kernel_zip: if is_gki_route {
            file_path_buf.clone()
        } else {
            None
        },
        gki_mode: is_gki_route,
        kpm_paths: kpm_paths.clone(),
        superkey: superkey.clone(),
        magisk_forks_apk: if matches!(pipe_provider, RootProvider::MagiskFork) {
            file_path_buf.clone()
        } else {
            None
        },
        nightly_run_id,
    };
    // Phase 2/8 — Resolve and download root files before EDL.
    live!(log, "[Root] {}", phases.marker(2));
    // Pin the nightly workflow run ID once so
    // every fetch in this Phase 2 pulls from
    // the SAME upstream build. Without this,
    // a new workflow landing between the
    // ~minute-long manager APK download and
    // the .ko/ksuinit fetch would split the
    // installed manager APK across two
    // different builds.
    ensure_nightly_run_id(&mut manager_cfg, &mut log)
        .map_err(|e| tr_args!("err_root_nightly_run_failed", error = e))?;
    let mut manager_apk = stage_root_manager_apk(&manager_cfg, &mut log)
        .map_err(|e| tr_args!("err_root_manager_apk_failed", error = e))?;
    stage_root_payload(&manager_cfg, &mut log)
        .map_err(|e| tr_args!("err_root_payload_failed", error = e))?;
    // Manager install is non-fatal; keep the path to surface in the
    // post-run manual-install reminder (the on-device /sdcard copy when
    // the push fallback worked, otherwise the local staged file).
    // `keep_staging` forces the work dir to survive cleanup only in the
    // latter case, where the local file is the user's last resort.
    let mut manager_install_failed_path: Option<std::path::PathBuf> = None;
    let mut keep_staging = false;
    let manager_installed_pre_edl = if adb_ready_at_start {
        if let Some(path) = manager_apk.as_ref() {
            match install_root_manager_apk(path, &mut log) {
                Ok(()) => true,
                Err(e) => {
                    live!(
                        log,
                        "[Root] {}",
                        tr_args!("log_root_manager_apk_install_failed_manual", error = e)
                    );
                    let (reminder, keep) = stage_manager_apk_for_manual_install(path, &mut log);
                    manager_install_failed_path = Some(reminder);
                    keep_staging |= keep;
                    false
                }
            }
        } else {
            false
        }
    } else {
        false
    };

    // Track whether Phase 6 has started any partition write.
    // Pre-write failures still attempt an EDL -> system reset;
    // once a write has begun, leave the device in EDL instead of
    // rebooting into a half-written AVB image set.
    let mut writes_started = false;
    let device_phase_result: std::result::Result<(), String> =
        (|| -> std::result::Result<(), String> {
            // Phase 3/8 — Enter EDL mode.
            live!(log, "[Root] {}", phases.marker(3));
            transition_to_edl(conn, &mut log)?;

            // The target was resolved once before payload staging. Geometry
            // resolves from GPT by its partition label.
            let base_name = manager_cfg.root_image_target.partition_base();
            // `slot_suffix` was poll-resolved at Phase 1
            // and propagated through `RootPipelineConfig`;
            // it is guaranteed to be `_a` or `_b` here.
            let root_primary = format!("{base_name}{slot_suffix}");
            let vbmeta_primary = format!("vbmeta{slot_suffix}");
            // Lenovo devices on Qualcomm UFS place
            // boot / init_boot / vbmeta on LUN 4 (userdata
            // LUN), same index used by the reference
            // `qdl-rs --phys-part-idx 4` recipe.
            const ROOT_PARTITIONS_LUN: u8 = 4;
            // vbmeta only appears when it takes part; on a chained target it is
            // never read or written, so naming it here would be misleading.
            if rebuild_vbmeta {
                live!(
                    log,
                    "[Root] {} {} / {} (LUN {ROOT_PARTITIONS_LUN})",
                    ll.root_resolved_prefix,
                    root_primary,
                    vbmeta_primary,
                );
            } else {
                live!(
                    log,
                    "[Root] {} {} (LUN {ROOT_PARTITIONS_LUN})",
                    ll.root_resolved_prefix,
                    root_primary,
                );
            }

            // Phase 4/8 — Read stock AVB-protected root images.
            live!(log, "[Root] {}", phases.marker(4));
            // Hoisted so Phase 6 can echo the path.
            // Routed through `app_paths::backup_dir_for`
            // so AppImage / distro Linux installs don't
            // try to write next to the executable.
            let backup_dir = ltbox_core::app_paths::backup_dir_for(&format!("backup_{base_name}"));
            // Set inside the dump block from the dumped root image's
            // fingerprint; carried to Phase 5 to skip AVB + vbmeta.
            let uses_efisp_gbl_route;
            // Whether the stock vbmeta ended up in the backup folder, so the
            // manifest can tell Unroot which partitions to restore.
            let vbmeta_backed_up;
            // Efisp/GBL route: when the dumped efisp is empty (stock,
            // GBL-unprovisioned) we download the region GBL here and
            // flash it alongside the patched root target image at Phase 6.
            let mut root_efisp_efi: Option<std::path::PathBuf> = None;
            {
                let mut session = open_edl_session(&loader, &mut log)?;
                let root_image_name = manager_cfg.root_image_target.filename();
                let dumped_root_image = work_dir.join(root_image_name);
                let dumped_vbmeta = work_dir.join("vbmeta.img");
                // `dump_partition` scans the LUN's GPT for the
                // named partition — matches the shell-level
                // `qdl-rs --phys-part-idx 4 dump-part <name>`.
                //
                // The root target is read first because every route needs it,
                // and it carries the same build fingerprint vbmeta does
                // (`com.android.build.<part>.fingerprint`) — so the model
                // cross-check below no longer forces a vbmeta dump on devices
                // that chain the target.
                session
                    .dump_partition(
                        &root_primary,
                        &dumped_root_image,
                        0,
                        ROOT_PARTITIONS_LUN,
                        &mut log,
                    )
                    .map_err(|e| {
                        tr_args!(
                            "err_root_dump_partition_failed",
                            partition = root_primary,
                            error = e
                        )
                    })?;

                // The model was detected over ADB/Fastboot before EDL. Verify
                // the dumped AVB image agrees before patching or flashing it.
                // The shared matcher treats TB320FC and LAVIETab9QHD1 as
                // equivalent.
                let root_image_info = ltbox_patch::avb::extract_image_avb_info(&dumped_root_image)
                    .map_err(|error| {
                        tr_args!(
                            "err_root_image_inspect_failed",
                            image = root_image_name,
                            error = error.to_string()
                        )
                    })?;
                let root_image_fingerprint = ltbox_patch::avb::build_fingerprint(&root_image_info)
                    .ok_or_else(|| {
                        tr_args!(
                            "err_root_image_fingerprint_missing",
                            image = root_image_name
                        )
                    })?;
                if !fingerprint_matches_detected_model(&root_image_fingerprint, &device_model) {
                    return Err(tr_args!(
                        "live_rescue_model_mismatch_abort",
                        device = device_model.as_str(),
                        fingerprint = root_image_fingerprint
                    ));
                }
                uses_efisp_gbl_route = fingerprint_uses_efisp_gbl_route(&root_image_fingerprint);

                // vbmeta is read only when the run rebuilds it: efisp/GBL models take
                // the GBL route, and every other non-TB320FC model chains the
                // boot target, which leaves vbmeta byte-identical either way.
                vbmeta_backed_up = rebuild_vbmeta && !uses_efisp_gbl_route;
                if vbmeta_backed_up {
                    session
                        .dump_partition(
                            &vbmeta_primary,
                            &dumped_vbmeta,
                            0,
                            ROOT_PARTITIONS_LUN,
                            &mut log,
                        )
                        .map_err(|e| {
                            tr_args!(
                                "err_root_dump_partition_failed",
                                partition = vbmeta_primary,
                                error = e
                            )
                        })?;
                }

                // Efisp/GBL root needs provisioned efisp; once present, skip AVB
                // footer and vbmeta writes. Keep the verified fingerprint so
                // an empty efisp can fetch the matching region GBL.
                if uses_efisp_gbl_route {
                    let efi_dir = ltbox_core::app_paths::work_dir_for("root_efisp");
                    root_efisp_efi = prepare_tb323fu_efisp(
                        &mut session,
                        &slot_suffix,
                        None,
                        &work_dir,
                        &efi_dir,
                        &mut log,
                    )?;
                }
                // Stock-image safety net for Unroot, captured
                // before the irreversible patch + flash. A copy
                // failure must abort the run.
                std::fs::create_dir_all(&backup_dir).map_err(|e| {
                    tr_args!(
                        "err_root_backup_dir_failed",
                        path = backup_dir.display(),
                        error = e
                    )
                })?;
                std::fs::copy(&dumped_root_image, backup_dir.join(root_image_name)).map_err(
                    |e| {
                        tr_args!(
                            "err_root_backup_copy_failed",
                            image = root_image_name,
                            error = e
                        )
                    },
                )?;
                if vbmeta_backed_up {
                    std::fs::copy(&dumped_vbmeta, backup_dir.join("vbmeta.img")).map_err(|e| {
                        tr_args!(
                            "err_root_backup_copy_failed",
                            image = "vbmeta.img",
                            error = e
                        )
                    })?;
                }
                write_root_backup_manifest(&backup_dir, base_name, vbmeta_backed_up).map_err(
                    |e| {
                        tr_args!(
                            "err_root_backup_copy_failed",
                            image = ROOT_BACKUP_MANIFEST_NAME,
                            error = e
                        )
                    },
                )?;
                if vbmeta_backed_up {
                    live!(
                        log,
                        "[Root] {} {} + vbmeta.img → {}",
                        ll.root_backup_copy_prefix,
                        root_image_name,
                        backup_dir.display()
                    );
                } else {
                    live!(
                        log,
                        "[Root] {} {} → {}",
                        ll.root_backup_copy_prefix,
                        root_image_name,
                        backup_dir.display()
                    );
                }
                // Bounce to Sahara — otherwise the second
                // session's sahara_run times out because
                // the device is still in Firehose.
                session
                    .reset_to_edl(&mut log)
                    .map_err(|e| tr_args!("err_root_reset_to_edl_failed", error = e))?;
                live!(log, "[EDL] {}", ll.closing_dump);
                // Drop session — serial port closes so
                // the post-patch open gets a fresh handle.
            }

            // Phase 5/8 — Offline root target image patch + AVB metadata rebuild.
            // vbmeta rebuild. Network downloads moved
            // up to Phase 2; this step never touches
            // the network so progress now matches the
            // "patching" label.
            live!(log, "[Root] {}", phases.marker(5));

            // The patch phase reuses the same config the
            // download phase built — none of the input
            // locals mutate between Phase 2 and Phase 5
            // (only `nightly_run_id` was hoisted out of
            // `manager_cfg` for logging). Clone the cfg
            // instead of re-cloning every field, which
            // keeps the two phases in lockstep automatically
            // if a future field gets added to the struct.
            let cfg = manager_cfg.clone();
            let artifacts = build_patched_artifacts(&cfg, uses_efisp_gbl_route, &mut log)
                .map_err(|e| tr_args!("err_root_patch_failed", error = e))?;
            if manager_apk.is_none() {
                manager_apk = artifacts.manager_apk.clone();
            }
            // Phase 6/8 — Write patched images.
            live!(log, "[Root] {}", phases.marker(6));
            let mut session = open_edl_session(&loader, &mut log)?;
            // Mirror of the equivalent one-shot `qdl-rs
            // --phys-part-idx 4 write <name> <img>` — GPT
            // resolves the start sector, so no rawprogram
            // sector attrs to thread through.
            // Provision efisp with the region GBL fetched above (only set
            // when the dumped efisp was empty) BEFORE flashing the patched
            // root target image. Ordering still matters for brick-safety: if
            // the GBL flash fails, the root target image is still stock. After
            // any write
            // begins, the error path leaves the device in EDL rather than
            // rebooting a partial chain.
            if let Some(efi) = &root_efisp_efi {
                writes_started = true;
                provision_tb323fu_efisp(&mut session, Some(efi), &mut log)?;
            }
            writes_started = true;
            session
                .flash_partition(
                    &artifacts.root_partition,
                    &artifacts.patched_root_image,
                    0,
                    ROOT_PARTITIONS_LUN,
                    &mut log,
                )
                .map_err(|e| {
                    tr_args!(
                        "err_root_flash_partition_failed",
                        partition = artifacts.root_partition,
                        error = e
                    )
                })?;
            if let Some(vbpath) = &artifacts.patched_vbmeta {
                session
                    .flash_partition(&vbmeta_primary, vbpath, 0, ROOT_PARTITIONS_LUN, &mut log)
                    .map_err(|e| {
                        tr_args!(
                            "err_root_flash_partition_failed",
                            partition = vbmeta_primary,
                            error = e
                        )
                    })?;
            }
            // Surface the backup folder before the reset
            // so the user doesn't have to scroll.
            if backup_dir.exists() {
                live!(
                    log,
                    "[Root] {} {}",
                    ll.backup_saved_prefix,
                    backup_dir.display()
                );
            }
            // Phase 7/8 — Reboot to Android.
            live!(log, "[Root] {}", phases.marker(7));
            session.reset_tolerant(&mut log);
            // Phase 8/8 — Finish Android setup and manager installation.
            live!(log, "[Root] {}", phases.marker(8));
            // Skip post-reboot retry if the pre-EDL install
            // already failed for a deterministic reason
            // (e.g. `INSTALL_FAILED_VERSION_DOWNGRADE`) — the
            // 60 s wait + reinstall would just hit the same
            // error after the user's burned a minute waiting.
            // The end-of-run reminder still fires from the
            // pre-EDL `manager_install_failed_path` stamp.
            if !manager_installed_pre_edl
                && manager_install_failed_path.is_none()
                && let Some(path) = manager_apk.as_ref()
                && let Err(e) = wait_and_install_root_manager_apk(
                    path,
                    std::time::Duration::from_secs(60),
                    &mut log,
                )
            {
                // Same non-fatal handling as the pre-EDL path —
                // log the warning, record the donor path for the
                // post-run reminder, keep going so the user
                // doesn't lose the success summary just because
                // the manager package couldn't auto-install.
                live!(
                    log,
                    "[Root] {}",
                    tr_args!("log_root_manager_apk_install_failed_manual", error = e)
                );
                let (reminder, keep) = stage_manager_apk_for_manual_install(path, &mut log);
                manager_install_failed_path = Some(reminder);
                keep_staging |= keep;
            }
            if let Some(path) = manager_install_failed_path.as_ref() {
                live!(
                    log,
                    "[Root] {}",
                    tr_args!(
                        "log_root_manager_apk_manual_reminder",
                        path = path.display().to_string()
                    )
                );
            }
            live!(log, "[Root] {}", ll.root_completed);
            Ok(())
        })();
    match device_phase_result {
        Ok(()) => {
            // Keep staging files on error for debugging, and also when the
            // manager APK could not be auto-installed *and* the on-device
            // push fallback failed — the local staged APK is then the only
            // copy the user can reach, so the reminder points at it and the
            // work dir must survive.
            if !keep_staging {
                let _ = std::fs::remove_dir_all(&base);
            }
            Ok(log)
        }
        Err(e) => {
            if !should_reset_after_root_device_error(writes_started) {
                // A partition write already began. Rebooting now risks
                // booting a half-written boot/vbmeta/efisp chain. Leave
                // the device in EDL and surface recovery guidance.
                let msg = tr_args!("err_root_partial_write_recovery", error = e);
                println!("[Root] {msg}");
                Err(msg)
            } else {
                // Pre-write failure: best-effort open a fresh session on
                // the same loader and ask the device to boot.
                // `reset_tolerant` already swallows the post-handoff error
                // some devices return, so this never masks the real error
                // — failures here are only logged.
                let mut reset_log: Vec<String> = Vec::new();
                reset_log.push(format!(
                    "[EDL] {}",
                    tr_args!("log_edl_attempt_reset_after_error", error = e.to_string())
                ));
                if let Ok(mut s) = ltbox_device::edl::EdlSession::open(&loader, &mut reset_log) {
                    s.reset_tolerant(&mut reset_log);
                } else {
                    reset_log.push(format!(
                        "[EDL] {}",
                        ltbox_core::i18n::tr("log_edl_reset_reopen_skipped")
                    ));
                }
                for line in reset_log {
                    println!("{line}");
                }
                Err(e)
            }
        }
    }
}

/// Whether the root device-phase error path should attempt an EDL reset.
/// Pre-write failures may reset; post-write failures must not.
fn should_reset_after_root_device_error(writes_started: bool) -> bool {
    !writes_started
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_write_failures_still_reset() {
        assert!(should_reset_after_root_device_error(false));
    }

    #[test]
    fn post_write_failures_leave_device_in_edl() {
        assert!(!should_reset_after_root_device_error(true));
    }

    #[test]
    fn dumped_image_fingerprint_must_match_detected_model() {
        let tb320fc = "qti/TB320FC/TB320FC:15/build:user/release-keys";
        let tb323fu = "qti/TB323FU/TB323FU:15/build:user/release-keys";
        let tb324zc = "qti/TB324ZC/TB324ZC:16/build:user/release-keys";

        assert!(fingerprint_matches_detected_model(tb320fc, "TB320FC"));
        assert!(fingerprint_matches_detected_model(tb320fc, "LAVIETab9QHD1"));
        assert!(!fingerprint_matches_detected_model(tb323fu, "TB320FC"));
        assert!(fingerprint_matches_detected_model(tb324zc, "TB324ZC"));
        assert!(fingerprint_uses_efisp_gbl_route(tb324zc));
        assert!(!fingerprint_uses_efisp_gbl_route(tb320fc));
    }

    #[test]
    fn routes_without_a_version_card_do_not_demand_a_version() {
        // Magisk forks: the wizard shows an APK picker where the version step
        // would be, so `version` is never set and the run must still start.
        assert_eq!(
            resolve_provider_version(false, false, Some(Provider::MagiskForks), None),
            Ok((Provider::MagiskForks, VerChoice::Stable))
        );
        // A channel picked before switching to forks is kept, not discarded.
        assert_eq!(
            resolve_provider_version(
                false,
                false,
                Some(Provider::MagiskForks),
                Some(VerChoice::Nightly)
            ),
            Ok((Provider::MagiskForks, VerChoice::Nightly))
        );
        for (gki, skroot) in [(true, false), (false, true)] {
            assert_eq!(
                resolve_provider_version(gki, skroot, None, None),
                Ok((Provider::Magisk, VerChoice::Stable))
            );
        }
    }

    #[test]
    fn routes_with_a_version_card_still_require_one() {
        assert_eq!(
            resolve_provider_version(false, false, Some(Provider::Magisk), None),
            Err(tr("err_root_version_missing"))
        );
        assert_eq!(
            resolve_provider_version(false, false, None, Some(VerChoice::Stable)),
            Err(tr("err_root_provider_missing"))
        );
    }
}
