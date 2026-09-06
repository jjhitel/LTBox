use super::*;
use crate::{ManualRollbackIndices, PhaseReporter};

#[allow(clippy::too_many_arguments)]
pub(crate) fn flash_worker(
    cfg: WorkflowConfig,
    conn: ConnectionStatus,
    mut device_model: String,
    fw_folder: String,
    loader_override: Option<String>,
    firmware_identity: Option<FirmwareIdentity>,
    user_abl_path: Option<String>,
    mut rb_mode: ltbox_patch::rollback::RollbackMode,
    manual_rollback_indices: Option<ManualRollbackIndices>,
    ll: LiveLabels,
    phases: PhaseReporter,
) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    let edl_start = matches!(conn, ConnectionStatus::Edl);
    let started_in_fastboot = matches!(conn, ConnectionStatus::Fastboot);
    let fw_dir = std::path::Path::new(&fw_folder);
    let (fw_key_class, firmware_fingerprint) = firmware_identity
        .map(|identity| (identity.key_class, identity.fingerprint))
        .unwrap_or((
            ltbox_patch::key_map::KeyClass::Unknown,
            Option::<String>::None,
        ));
    let user_abl = user_abl_path.map(std::path::PathBuf::from);

    // Phase 1/9 — Validate firmware inputs.
    live!(log, "[Flash] {}", phases.marker(1));
    if !fw_dir.exists() {
        return Err(tr_args!(
            "err_flash_firmware_folder_missing",
            path = fw_folder
        ));
    }
    live!(
        log,
        "[Flash] {}",
        tr_args!("live_flash_firmware_folder", path = fw_folder)
    );

    // Decompress any `*.zst` partition images (e.g. a ported ROM's
    // `super.img.zst`) up front — before any device probe / transition. The
    // output can be tens of GB and take a while, so do it while the device is
    // still untouched (rather than leaving it parked in the bootloader), and so
    // a compressed AVB-protected partition image is present for the scan + region/AVB/ARB
    // planning below. On failure the device has not been moved, so just return.
    // Phase 2/9 — Decompress packaged images.
    live!(log, "[Flash] {}", phases.marker(2));
    decompress_zst_images(fw_dir, &mut log)?;

    // Phase 3/9 — Inspect device and firmware compatibility.
    live!(log, "[Flash] {}", phases.marker(3));

    // Device detection
    //
    // Run the ADB device probe BEFORE the
    // Fastboot bridge below. The previous
    // ordering kicked off `adb reboot bootloader`
    // first and only then asked `AdbManager::
    // check_device`, by which point the device
    // had already detached from ADB — so the
    // detection block always logged "no ADB
    // device info" even when an ADB bridge
    // was sitting right there a second earlier.
    // Now device info gets collected on the live
    // ADB transport, and the bridge takes over
    // afterwards.
    let skip_adb = conn.skip_adb();
    if skip_adb {
        ltbox_core::live!(
            log,
            "[Flash] {}",
            ltbox_core::i18n::tr("live_flash_skip_adb")
        );
    } else {
        ltbox_core::live!(
            log,
            "[ADB] {}",
            ltbox_core::i18n::tr("live_adb_checking_device")
        );
        if ltbox_device::adb::AdbManager::new_if_connected().is_some() {
            ltbox_core::live!(
                log,
                "[ADB] {}",
                ltbox_core::i18n::tr("live_adb_device_connected")
            );
            // The active slot is resolved later via
            // `controller::poll_active_slot` — that
            // helper polls both ADB + Fastboot and
            // hard-errors on probe failure, so the
            // earlier `get_slot_suffix` round-trip
            // here was redundant (its result was
            // assigned to `_slot` and discarded).
        } else {
            ltbox_core::live!(
                log,
                "[ADB] {}",
                ltbox_core::i18n::tr("live_adb_no_device_info")
            );
        }
    }

    // Snapshot rollback index before EDL —
    // `stored_rollback_index` vanishes past
    // Fastboot. Probe Fastboot vars first, and
    // when the device is sitting in ADB bridge
    // it through `adb reboot bootloader` before
    // retrying — otherwise the user sees the
    // ARB=ON abort on every PRC↔ROW flash that
    // started from the ADB-connected state, even
    // though Fastboot is reachable in principle.
    struct FastbootProbe {
        device_index: Option<u64>,
        rollback_floors: Option<ltbox_patch::rollback::FastbootRollbackFloors>,
        reachable: bool,
        active_slot: Option<String>,
        raw_getvar_all: String,
    }
    let probe_fastboot = || -> FastbootProbe {
        match ltbox_device::fastboot::FastbootDevice::open() {
            Ok(mut dev) => match dev.get_all_vars() {
                Ok(v) => FastbootProbe {
                    device_index: ltbox_patch::rollback::compute_device_rollback_index(
                        &v.rollback_indices,
                    ),
                    rollback_floors: ltbox_patch::rollback::classify_fastboot_rollback_floors(
                        &v.rollback_indices,
                    ),
                    reachable: true,
                    active_slot: v.current_slot,
                    raw_getvar_all: v.raw_getvar_all,
                },
                Err(_) => FastbootProbe {
                    device_index: None,
                    rollback_floors: None,
                    reachable: false,
                    active_slot: None,
                    raw_getvar_all: String::new(),
                },
            },
            Err(_) => FastbootProbe {
                device_index: None,
                rollback_floors: None,
                reachable: false,
                active_slot: None,
                raw_getvar_all: String::new(),
            },
        }
    };
    let mut probe = probe_fastboot();
    let adb_connected = matches!(conn, ConnectionStatus::Adb | ConnectionStatus::AdbRecovery);
    if !probe.reachable && adb_connected {
        ltbox_core::live!(
            log,
            "[Flash] {}",
            ltbox_core::i18n::tr("live_flash_adb_to_bootloader")
        );
        if let Some(mut adb) = ltbox_device::adb::AdbManager::new_if_connected() {
            // `adb reboot bootloader` often returns a
            // disconnect/transport error even when the
            // reboot physically succeeds. Treat the
            // observed Fastboot state as authoritative:
            // always poll after the attempt, and only
            // surface the reboot error if Fastboot never
            // appears.
            let reboot_err = match adb.reboot("bootloader") {
                Ok(()) => None,
                Err(e) => Some(e.to_string()),
            };
            // Poll for Fastboot up to 60s — ADB→bootloader
            // typically lands inside 8 s but cold boots
            // can drag. Open/parse once per attempt and
            // reuse that probe result instead of
            // check_device()+immediate reopen.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            while std::time::Instant::now() < deadline {
                probe = probe_fastboot();
                if probe.reachable {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            // Final probe after the wait loop so the full
            // 60s deadline is covered (loop may last probe
            // slightly before the boundary due to sleep).
            if !probe.reachable {
                probe = probe_fastboot();
            }
            if !probe.reachable {
                if let Some(error) = reboot_err {
                    ltbox_core::live!(
                        log,
                        "[ADB] {}",
                        tr_args!("live_adb_reboot_failed", error = error)
                    );
                } else {
                    // reboot() returned Ok, but Fastboot never appeared.
                    // Keep going for Auto/Off (EDL path still works); only
                    // surface an explicit timeout so the log is not silent.
                    ltbox_core::live!(
                        log,
                        "[ADB] {}",
                        tr_args!("live_flash_fastboot_timeout", seconds = "60")
                    );
                }
            }
        }
    }
    let FastbootProbe {
        device_index: device_rollback_index,
        rollback_floors: fastboot_rollback_floors,
        reachable: fastboot_reachable,
        active_slot,
        raw_getvar_all: getvar_raw,
    } = probe;

    // 3. Scan firmware folder
    let vendor_boot = fw_dir.join("vendor_boot.img");
    let vbmeta = fw_dir.join("vbmeta.img");
    let boot = fw_dir.join("boot.img");
    let has_vendor_boot = vendor_boot.exists();
    let has_vbmeta = vbmeta.exists();
    let has_boot = boot.exists();
    let found = ltbox_core::i18n::tr("live_status_found");
    let not_found = ltbox_core::i18n::tr("live_status_not_found");
    ltbox_core::live!(
        log,
        "[Flash] {}",
        tr_args!(
            "live_flash_vendor_boot_status",
            status = if has_vendor_boot { &found } else { &not_found },
        )
    );
    ltbox_core::live!(
        log,
        "[Flash] {}",
        tr_args!(
            "live_flash_vbmeta_status",
            status = if has_vbmeta { &found } else { &not_found },
        )
    );
    ltbox_core::live!(
        log,
        "[Flash] {}",
        tr_args!(
            "live_flash_boot_status",
            status = if has_boot { &found } else { &not_found },
        )
    );
    // No rawprogram pack (.x encrypted or .xml) anywhere in the folder → almost
    // certainly the wrong folder, not a firmware image set. Say so clearly so the
    // later AVB key abort isn't mistaken for a firmware-key problem.
    let has_rawprogram_pack = std::fs::read_dir(fw_dir)
        .map(|rd| {
            rd.flatten().any(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|x| x.eq_ignore_ascii_case("x") || x.eq_ignore_ascii_case("xml"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    if !has_rawprogram_pack {
        ltbox_core::live!(
            log,
            "[Flash] {}",
            ltbox_core::i18n::tr("live_flash_no_rawprogram_pack")
        );
    }

    // Cross-check the cached vbmeta_system fingerprint against the probed model
    // before EDL. Folder-step inspection is the single firmware identity source.
    if !edl_start {
        match firmware_fingerprint.as_deref() {
            None => {
                ltbox_core::live!(
                    log,
                    "[Flash] {}",
                    ltbox_core::i18n::tr("live_rescue_no_fingerprint_skip")
                );
            }
            Some(fingerprint) => {
                let normalized_model = device_model.replace(' ', "");
                if normalized_model.is_empty()
                    || ltbox_core::model::fingerprint_model_match(fingerprint, &normalized_model)
                {
                    ltbox_core::live!(
                        log,
                        "[Flash] {}",
                        ltbox_core::i18n::tr("live_rescue_model_check_ok")
                    );
                } else {
                    ltbox_core::live!(
                        log,
                        "[Flash] {}",
                        tr_args!(
                            "live_rescue_model_mismatch_abort",
                            device = normalized_model,
                            fingerprint = fingerprint
                        )
                    );
                    let err = ltbox_core::i18n::tr("err_flash_model_mismatch_pre_edl");
                    reboot_fastboot_to_system_after_pre_edl_abort(&mut log, started_in_fastboot);
                    return Err(err);
                }
            }
        }
    }

    // Phase 4/9 — Prepare the flash and safety plan.
    live!(log, "[Flash] {}", phases.marker(4));

    // TB323FU keeps region vendor_boot/vbmeta AVB conversion off (it
    // provisions a GBL on efisp instead) but DOES take ARB
    // overlays. Region detect uses fp first, then model.
    let tb323fu_skip_region = firmware_fingerprint
        .as_deref()
        .map(|fp| fingerprint_token_match(fp, "TB323FU"))
        .unwrap_or(false)
        || fingerprint_token_match(&device_model, "TB323FU");

    // GBL/ARB work follows the TARGET firmware identity
    // (vendor_boot fp), never the connected device.
    let target_is_tb323fu = firmware_fingerprint
        .as_deref()
        .map(|fp| fingerprint_token_match(fp, "TB323FU"))
        .unwrap_or(false);
    let xiaoxin_skip_region = xiaoxin_pro13_token(&device_model).is_some()
        || firmware_fingerprint
            .as_deref()
            .and_then(xiaoxin_pro13_token)
            .is_some();
    let mut xiaoxin_pro13_flash = xiaoxin_skip_region;

    // EDL-start no longer forces rollback-bypass (or region) off. The device
    // model and committed rollback index are read by dumping vendor_boot +
    // boot + vbmeta_system from BOTH slots over EDL once the session is open
    // (see the `edl_start` block after `EdlSession::open` below), so the
    // user's selected rollback-bypass + region modes are preserved exactly as
    // on an ADB/bootloader start.

    // TB323FU must never run a blind ON: ON bumps even
    // matching indices and would force the testkey/_arb
    // chain when no downgrade is in play. Demote to AUTO so
    // the EDL-dumped device index decides per partition.
    if target_is_tb323fu && rb_mode != ltbox_patch::rollback::RollbackMode::Off {
        if rb_mode == ltbox_patch::rollback::RollbackMode::On {
            rb_mode = ltbox_patch::rollback::RollbackMode::Auto;
            ltbox_core::live!(
                log,
                "[ARB] {}",
                ltbox_core::i18n::tr("live_flash_tb323fu_force_auto")
            );
        } else if rb_mode == ltbox_patch::rollback::RollbackMode::Manual {
            rb_mode = ltbox_patch::rollback::RollbackMode::Off;
            ltbox_core::live!(
                log,
                "[ARB] {}",
                ltbox_core::i18n::tr("rollback_manual_tb323fu_disabled")
            );
        }
    }
    if xiaoxin_pro13_flash && rb_mode != ltbox_patch::rollback::RollbackMode::Auto {
        rb_mode = ltbox_patch::rollback::RollbackMode::Auto;
        ltbox_core::live!(
            log,
            "[ARB] {}",
            ltbox_core::i18n::tr("live_flash_xiaoxin_force_auto")
        );
    }
    if tb323fu_skip_region {
        ltbox_core::live!(
            log,
            "[Flash] {}",
            ltbox_core::i18n::tr("live_flash_tb323fu_region_efisp")
        );
    }
    if xiaoxin_skip_region {
        ltbox_core::live!(
            log,
            "[Flash] {}",
            ltbox_core::i18n::tr("live_flash_xiaoxin_region_proinfo")
        );
    }

    // Rollback=ON + no fastboot vars → can't target a safe
    // index. Bail before EDL — UNLESS the device started in EDL, where the
    // index is read by dumping partitions over the open session (the
    // `edl_start` block after `EdlSession::open`). A bootloader/ADB start with
    // unreachable fastboot still has no index source, so it still aborts.
    if matches!(rb_mode, ltbox_patch::rollback::RollbackMode::On)
        && !fastboot_reachable
        && !edl_start
    {
        live!(
            log,
            "[ARB] {}",
            ltbox_core::i18n::tr("live_arb_on_fastboot_unreachable")
        );
        // Best-effort reboot — any failure stays
        // in the log; wizard still gets the Err.
        if let Some(mut adb) = ltbox_device::adb::AdbManager::new_if_connected() {
            if let Err(e) = adb.shell("reboot") {
                ltbox_core::live!(
                    log,
                    "[ADB] {}",
                    tr_args!("live_adb_reboot_failed", error = e.to_string())
                );
            } else {
                ltbox_core::live!(
                    log,
                    "[ADB] {}",
                    ltbox_core::i18n::tr("live_adb_reboot_sent")
                );
            }
        } else {
            ltbox_core::live!(
                log,
                "[ADB] {}",
                ltbox_core::i18n::tr("live_adb_no_reboot_route")
            );
        }
        return Err(ltbox_core::i18n::tr("err_rollback_on_fastboot_unreachable"));
    }

    // efisp GBL download is deferred until after the EDL
    // ARB dump decides `_arb` (testkey-root) vs stock — see
    // the post-rawprogram-staging block below.
    let mut efisp_efi: Option<std::path::PathBuf> = None;
    let mut tb323fu_arb_need = false;

    // Count .x and .xml files
    // Count flashable `.x` (rawprogram) files. The
    // encrypted Sahara manifest
    // (`qsahara_device_programmer.x`) is a loader, not a
    // flash image, so it is excluded here and left for
    // `EdlSession::open` to decrypt at load time.
    let x_count = std::fs::read_dir(fw_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.extension()
                        .map(|ext| ext.eq_ignore_ascii_case("x"))
                        .unwrap_or(false)
                        && !ltbox_core::sahara_xml::is_encrypted_manifest_filename(p)
                })
                .count()
        })
        .unwrap_or(0);
    let xml_count = std::fs::read_dir(fw_dir)
        .map(|rd| {
            rd.filter(|e| {
                e.as_ref()
                    .ok()
                    .map(|e| {
                        let p = e.path();
                        p.extension().map(|ext| ext == "xml").unwrap_or(false)
                            && p.file_name()
                                .map(|n| n.to_string_lossy().starts_with("rawprogram"))
                                .unwrap_or(false)
                    })
                    .unwrap_or(false)
            })
            .count()
        })
        .unwrap_or(0);
    ltbox_core::live!(
        log,
        "[Flash] {}",
        tr_args!(
            "live_flash_files_count",
            x_count = x_count.to_string(),
            xml_count = xml_count.to_string()
        )
    );

    // AVB root-of-trust pre-check (before region conversion, which only re-signs
    // via testkeys in KEY_MAP). Use the folder step's cached vbmeta_system key
    // class (the unified key source): an `Unknown` key aborts; a Lenovo-key
    // firmware aborts on cross-region for now (same-region Lenovo-key is handled
    // after EDL opens; cross-region re-sign is a separate change). TB323FU has its
    // own region path (efisp GBL) and is exempt here.
    if fw_key_class == ltbox_patch::key_map::KeyClass::Unknown {
        ltbox_core::live!(
            log,
            "[AVB] {}",
            ltbox_core::i18n::tr("live_flash_vbmeta_key_unknown")
        );
        reboot_fastboot_to_system_after_pre_edl_abort(&mut log, started_in_fastboot);
        return Err(ltbox_core::i18n::tr("err_flash_vbmeta_key_unknown"));
    }
    // 4. Region conversion. Skipped for a Lenovo-key firmware: its
    //    vbmeta isn't in KEY_MAP, so the standard (testkey) converter cannot
    //    re-sign it. Cross-region Lenovo-key is handled after EDL opens, where the
    //    device key class decides between a testkey re-sign + conversion
    //    (testkey device) or an abort (Lenovo-key device).
    let mut region_pair: Option<ltbox_patch::region::RegionAvbOutput> = None;
    if cfg.modify_region
        && !tb323fu_skip_region
        && !xiaoxin_skip_region
        && fw_key_class != ltbox_patch::key_map::KeyClass::Lenovo
    {
        if has_vendor_boot && has_vbmeta {
            ltbox_core::live!(log, "[Region] {}", ltbox_core::i18n::tr("live_region_on"));
            ltbox_core::live!(
                log,
                "[Region] {}",
                ltbox_core::i18n::tr("live_region_ready")
            );
            let Some(device_region) = cfg.device_region else {
                let err = ltbox_core::i18n::tr("err_region_missing_device_region");
                reboot_fastboot_to_system_after_pre_edl_abort(&mut log, started_in_fastboot);
                return Err(err);
            };
            let target = device_region.to_region_target();
            let output_dir = ltbox_core::app_paths::auto_output_dir_for("region_convert");
            ltbox_core::live!(
                log,
                "[Region] {}",
                tr_args!(
                    "live_region_building_pair",
                    region = format!("{:?}", device_region)
                )
            );
            match ltbox_patch::region::build_region_converted_avb_images(
                fw_dir,
                &output_dir,
                target,
                &ltbox_patch::region::RegionPatternSet::default(),
                None,
            ) {
                Ok(ltbox_patch::region::RegionAvbBuild::Built(output)) => {
                    ltbox_core::live!(
                        log,
                        "[Region] {}",
                        tr_args!(
                            "live_region_source_target",
                            source = format!("{:?}", output.source_region),
                            target = format!("{:?}", output.target)
                        )
                    );
                    ltbox_core::live!(
                        log,
                        "[Region] {}",
                        tr_args!(
                            "live_region_patched",
                            count = output.replacement_count.to_string(),
                            path = output.vendor_boot.display().to_string()
                        )
                    );
                    ltbox_core::live!(
                        log,
                        "[Region] {}",
                        tr_args!(
                            "live_region_avb_images_rebuilt",
                            path = output.vbmeta.display().to_string()
                        )
                    );
                    region_pair = Some(output);
                }
                Ok(ltbox_patch::region::RegionAvbBuild::Skipped {
                    source_region,
                    target,
                }) => {
                    ltbox_core::live!(
                        log,
                        "[Region] {}",
                        tr_args!(
                            "live_region_source_target",
                            source = format!("{:?}", source_region),
                            target = format!("{:?}", target)
                        )
                    );
                    ltbox_core::live!(
                        log,
                        "[Region] {}",
                        ltbox_core::i18n::tr("live_region_source_matches_target")
                    );
                }
                Err(e) => {
                    let err = tr_args!("err_region_conversion_failed", error = e.to_string());
                    reboot_fastboot_to_system_after_pre_edl_abort(&mut log, started_in_fastboot);
                    return Err(err);
                }
            }
        } else {
            ltbox_core::live!(
                log,
                "[Region] {}",
                ltbox_core::i18n::tr("live_region_missing_skip")
            );
        }
    }

    // 5. ARB detection. The effective rollback mode is already surfaced by
    // the `[Flash] Bypass rollback protection: …` summary line, so it is not
    // repeated here; this block reports the measured indices + the final
    // bypass decision.
    let device_idx_str = device_rollback_index
        .map(|v| v.to_string())
        .unwrap_or_else(|| ltbox_core::i18n::tr("live_arb_device_index_none"));
    ltbox_core::live!(
        log,
        "[ARB] {}",
        tr_args!("live_arb_device_index", index = device_idx_str)
    );
    if has_boot && !edl_start && rb_mode != ltbox_patch::rollback::RollbackMode::Manual {
        // Pre-result "Analyzing …" line dropped — analysis is
        // synchronous and the result line ("boot.img rollback
        // index: …") fires immediately after. Skipped on EDL-start: the
        // device index is unknown until the post-open both-slot dump, so a
        // pre-EDL summary here would print a misleading "bypass: no".
        match ltbox_patch::rollback::analyze_rollback_with_mode(
            &boot,
            device_rollback_index,
            rb_mode,
        ) {
            Ok(info) => {
                ltbox_core::live!(
                    log,
                    "[ARB] {}",
                    tr_args!(
                        "live_arb_boot_index_result",
                        index = info.image_index.to_string()
                    )
                );
                ltbox_core::live!(
                    log,
                    "[ARB] {}",
                    tr_args!(
                        "live_arb_rollback_bypass",
                        value = ltbox_core::i18n::tr(if info.needs_patch {
                            "common_yes"
                        } else {
                            "common_no"
                        })
                    )
                );
            }
            Err(e) => ltbox_core::live!(
                log,
                "[ARB] {}",
                tr_args!("live_arb_boot_analysis_failed", error = e.to_string())
            ),
        }
    }
    // ARB analysis above is diagnostic only — flash plan unchanged.

    // 6. XML
    //
    // Decrypt every shipped `.x` rawprogram in place so the catalog scan
    // below picks up the `<stem>.xml` output (see decrypt_rawprogram_x_files).
    if x_count > 0 {
        decrypt_rawprogram_x_files(fw_dir, &mut log)?;
    }
    if !cfg.wipe && xml_count > 0 {
        ltbox_core::live!(
            log,
            "[XML] {}",
            ltbox_core::i18n::tr("live_xml_keep_excludes")
        );
    }

    // 7. Country code
    if cfg.wipe {
        ltbox_core::live!(
            log,
            "[Flash] {}",
            ltbox_core::i18n::tr("live_flash_data_mode_wipe")
        );
    }
    if let Some(cc) = cfg.country_action.target() {
        ltbox_core::live!(
            log,
            "[Flash] {}",
            tr_args!("live_flash_country_devinfo", code = cc)
        );
    } else if cfg.wipe && cfg.country_action.is_skipped() {
        ltbox_core::live!(
            log,
            "[Flash] {}",
            ltbox_core::i18n::tr("live_flash_country_skip")
        );
    }

    // 8. EDL flash. A user-picked loader (the firmware folder shipped none) wins
    // over the in-folder lookup.
    let loader = match loader_override {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            let loader = find_firmware_loader(fw_dir);
            if loader.is_none() {
                ltbox_core::live!(
                    log,
                    "[EDL] {}",
                    ltbox_core::i18n::tr("live_edl_loader_missing")
                );
            }
            require_firmware_loader(loader)?
        }
    };

    // Phase 5/9 — Enter EDL and open the Firehose transport.
    live!(log, "[Flash] {}", phases.marker(5));
    transition_to_edl(conn, &mut log)?;

    let mut session = open_edl_session(&loader, &mut log)?;

    // Phase 6/9 — Read live device state and stage safeguards.
    live!(log, "[Flash] {}", phases.marker(6));

    // EDL-start: fastboot/ADB never ran, so the device model + committed
    // rollback index are unknown. Read them off the device by dumping BOTH
    // slots over the open session — the active slot is unknown in EDL-start,
    // and the inactive slot's images may not carry parseable AVB info.
    // vendor_boot identifies the model (compared against the target firmware
    // fingerprint); boot + vbmeta_system give the rollback floor. If neither
    // slot yields a valid AVB image, reset back into EDL and abort rather than
    // flash blind. TB322FC has no rollback protection → bypass forced Off.
    // On EDL-start, the per-location rollback floors (component-wise max across
    // both slots) feed both the generic ARB overlay loop and TB323FU's overlay
    // builder below — each location keeps its own floor.
    let mut edl_floors: Option<(u64, u64)> = None;
    if edl_start {
        match read_edl_start_device(&mut session, firmware_fingerprint.as_deref(), &mut log) {
            Ok(probe) => {
                device_model = probe.model_token;
                if !xiaoxin_pro13_flash && ltbox_core::model::is_xiaoxin_pro13_model(&device_model)
                {
                    xiaoxin_pro13_flash = true;
                    if rb_mode != ltbox_patch::rollback::RollbackMode::Auto {
                        rb_mode = ltbox_patch::rollback::RollbackMode::Auto;
                        ltbox_core::live!(
                            log,
                            "[ARB] {}",
                            ltbox_core::i18n::tr("live_flash_xiaoxin_force_auto")
                        );
                    }
                }
                match probe.rollback_floors {
                    None => {
                        // TB322FC is PRC-only. The pre-EDL UI gates that block
                        // cross-region / non-CN country never fired (the model
                        // was unknown until now), so enforce the constraint
                        // here, before any region or country write.
                        let non_cn_country = cfg
                            .country_action
                            .target()
                            .map(|c| !c.eq_ignore_ascii_case("CN"))
                            .unwrap_or(false);
                        if rb_mode != ltbox_patch::rollback::RollbackMode::Manual {
                            if cfg.modify_region || non_cn_country {
                                let _ = session.reset_to_edl(&mut log);
                                return Err(ltbox_core::i18n::tr("err_flash_tb322fc_prc_only"));
                            }
                            rb_mode = ltbox_patch::rollback::RollbackMode::Off;
                            ltbox_core::live!(
                                log,
                                "[ARB] {}",
                                ltbox_core::i18n::tr("live_arb_device_index_none")
                            );
                        } else if cfg.modify_region || non_cn_country {
                            let _ = session.reset_to_edl(&mut log);
                            return Err(ltbox_core::i18n::tr("err_flash_tb322fc_prc_only"));
                        }
                    }
                    Some(floors) => {
                        // Component-wise floors flow per location into both the
                        // generic overlay loop and the TB323FU builder; never
                        // collapse to a single max (that would inflate the
                        // lower location and block future stock firmware).
                        edl_floors = Some(floors);
                    }
                }
            }
            Err(e) => {
                if e == ltbox_core::i18n::tr("err_flash_xiaoxin_arb_floor_unreadable") {
                    session.reset_tolerant(&mut log);
                } else {
                    let _ = session.reset_to_edl(&mut log);
                }
                return Err(e);
            }
        }
    }

    // TB376FC/TB390FU bootloaders do not expose rollback variables. Even on an
    // ADB/Fastboot start, determine both component floors from EDL dumps and
    // fail closed before rawprogram can write anything.
    if !edl_start && xiaoxin_pro13_flash {
        let floor_dir = ltbox_core::app_paths::work_dir_for("flash_xiaoxin_arb");
        let _ = std::fs::remove_dir_all(&floor_dir);
        if let Err(error) = std::fs::create_dir_all(&floor_dir) {
            ltbox_core::live!(
                log,
                "[ARB] {}",
                tr_args!("err_arb_work_dir_failed", error = error)
            );
            session.reset_tolerant(&mut log);
            return Err(ltbox_core::i18n::tr(
                "err_flash_xiaoxin_arb_floor_unreadable",
            ));
        }
        match read_device_vbmeta(&mut session, active_slot.as_deref(), &floor_dir, &mut log) {
            Ok(device) => edl_floors = Some((device.boot_floor, device.vbs_floor)),
            Err(error) => {
                ltbox_core::live!(log, "[ARB] {error}");
                session.reset_tolerant(&mut log);
                return Err(ltbox_core::i18n::tr(
                    "err_flash_xiaoxin_arb_floor_unreadable",
                ));
            }
        }
    }

    // TB376FC/TB390FU require a dedicated fail-closed comparison before the
    // Lenovo-key device gate so every unsafe or unreadable case reboots to system.
    if xiaoxin_pro13_flash {
        let firmware_indices = match (
            ltbox_patch::avb::extract_image_avb_info(&fw_dir.join("boot.img")),
            ltbox_patch::avb::extract_image_avb_info(&fw_dir.join("vbmeta_system.img")),
        ) {
            (Ok(boot), Ok(vbs)) => Some((boot.rollback_index, vbs.rollback_index)),
            _ => None,
        };
        match xiaoxin_rollback_decision(edl_floors, firmware_indices) {
            XiaoxinRollbackDecision::Proceed => {}
            XiaoxinRollbackDecision::Downgrade => {
                session.reset_tolerant(&mut log);
                return Err(ltbox_core::i18n::tr("err_flash_xiaoxin_arb_downgrade"));
            }
            XiaoxinRollbackDecision::Unreadable => {
                session.reset_tolerant(&mut log);
                return Err(ltbox_core::i18n::tr(
                    "err_flash_xiaoxin_arb_floor_unreadable",
                ));
            }
        }
    }

    // Full-firmware flash: rawprogram + patch XMLs
    // drive every program node (no slot guessing).
    let (raw_xmls, patch_xmls) = ltbox_device::edl::collect_firmware_xmls_for_flash(fw_dir, false)
        .map_err(|e| tr_args!("err_flash_xml_selection_failed", error = e.to_string()))?;
    if raw_xmls.is_empty() {
        return Err(tr_args!("err_flash_no_rawprogram_xml", path = fw_folder));
    }
    // Stage ARB copies; flash them after rawprogram.
    let mut arb_patched: Vec<(String, u8, std::path::PathBuf)> = Vec::new();
    // Bootloader to overlay onto abl_a after the flash: either the device backup
    // or the user-supplied testkey ABL selected by the wizard.
    let mut abl_restore: Option<(u8, std::path::PathBuf)> = None;

    // AVB root-of-trust gate (device side). The firmware vbmeta was already
    // classified (`fw_key_class`): `Unknown` aborted before region conversion;
    // `Testkey` firmware and a `Lenovo` firmware on TB323FU take their existing
    // paths. Here a `Lenovo` firmware on any other model classifies the
    // device's active-slot vbmeta and either proceeds as-is (Lenovo-key device, same
    // region, no downgrade), re-signs to the testkey + preserves the device
    // bootloader (testkey device — including a cross-region convert-then-resign),
    // or aborts (Lenovo-key device cross-region/downgrade, or unknown device key).
    if fw_key_class == ltbox_patch::key_map::KeyClass::Lenovo && !target_is_tb323fu {
        let kc_dir = ltbox_core::app_paths::work_dir_for("flash_keyclass");
        let _ = std::fs::remove_dir_all(&kc_dir);
        std::fs::create_dir_all(&kc_dir)
            .map_err(|e| tr_args!("err_keyclass_work_dir_failed", error = e))?;
        let dev = match read_device_vbmeta(&mut session, active_slot.as_deref(), &kc_dir, &mut log)
        {
            Ok(d) => d,
            Err(e) => {
                if edl_start {
                    let _ = session.reset_to_edl(&mut log);
                } else {
                    let _ = session.reset(&mut log);
                }
                return Err(e);
            }
        };
        let policy = lenovo_firmware_device_policy(dev.class, user_abl.is_some());
        match policy {
            LenovoFirmwareDevicePolicy::AbortUnknown => {
                if edl_start {
                    let _ = session.reset_to_edl(&mut log);
                } else {
                    let _ = session.reset(&mut log);
                }
                return Err(ltbox_core::i18n::tr("err_flash_device_key_unknown"));
            }
            LenovoFirmwareDevicePolicy::KeepLenovo => {
                // Lenovo-key device + Lenovo-key firmware: the bootloader enforces rollback.
                // Other models still reject region conversion; TB376FC/TB390FU
                // flash the selected package as-is and store identity in proinfo.
                let fw_boot_idx = ltbox_patch::avb::extract_image_avb_info(&boot)
                    .map(|i| i.rollback_index)
                    .unwrap_or(0);
                let fw_vbs_idx =
                    ltbox_patch::avb::extract_image_avb_info(&fw_dir.join("vbmeta_system.img"))
                        .map(|i| i.rollback_index)
                        .unwrap_or(0);
                let downgrade = fw_boot_idx < dev.boot_floor || fw_vbs_idx < dev.vbs_floor;
                if (!xiaoxin_pro13_flash && cfg.modify_region) || downgrade {
                    if edl_start {
                        let _ = session.reset_to_edl(&mut log);
                    } else {
                        let _ = session.reset(&mut log);
                    }
                    // Same cause as an unresolvable signing key: this device
                    // is on firmware that closed the test-key hole.
                    return Err(ltbox_core::i18n::tr("err_device_key_lenovo"));
                }
                // Manual indexes would have to be written into the images, and
                // rewriting one invalidates its LenovoRSA signature that this
                // device is the only thing that will accept. The mode cannot be
                // honoured, so stop instead of silently flashing stock indexes.
                if rb_mode == ltbox_patch::rollback::RollbackMode::Manual {
                    if edl_start {
                        let _ = session.reset_to_edl(&mut log);
                    } else {
                        let _ = session.reset(&mut log);
                    }
                    return Err(ltbox_core::i18n::tr(
                        "err_flash_rollback_manual_device_key_lenovo",
                    ));
                }
                ltbox_core::live!(
                    log,
                    "[AVB] {}",
                    ltbox_core::i18n::tr("live_flash_lenovo_key_proceed")
                );
                rb_mode = ltbox_patch::rollback::RollbackMode::Off;
            }
            LenovoFirmwareDevicePolicy::ResignTestkey => {
                // Testkey device (or a user-supplied testkey ABL) + Lenovo-key firmware:
                // re-sign the install to the RSA-4096 testkey root. The
                // vbmeta_system signer may itself be RSA-2048; it still indicates
                // a testkey-class device whose root vbmeta trusts this re-sign.
                // The firmware's abl would re-root the chain to the Lenovo key and
                // reject the re-signed images.
                ltbox_core::live!(
                    log,
                    "[AVB] {}",
                    ltbox_core::i18n::tr("live_flash_lenovo_key_resign")
                );
                let arb_work_dir = ltbox_core::app_paths::work_dir_for("flash_arb");
                let _ = std::fs::remove_dir_all(&arb_work_dir);
                std::fs::create_dir_all(&arb_work_dir)
                    .map_err(|e| tr_args!("err_arb_work_dir_failed", error = e))?;

                // Cross-region: convert vendor_boot + rebuild a testkey vbmeta
                // first (region converter passes the testkey override), then
                // re-sign the chain ON TOP of that vbmeta so the merged vbmeta
                // carries both the converted vendor_boot hash and the testkey
                // chain. The converted vendor_boot is flashed as an overlay just
                // before the merged vbmeta_a.
                let mut resign_base: Option<std::path::PathBuf> = None;
                let mut region_vendor_boot: Option<std::path::PathBuf> = None;
                if cfg.modify_region {
                    let Some(device_region) = cfg.device_region else {
                        if edl_start {
                            let _ = session.reset_to_edl(&mut log);
                        } else {
                            let _ = session.reset(&mut log);
                        }
                        return Err(ltbox_core::i18n::tr("err_region_missing_device_region"));
                    };
                    let region_dir = ltbox_core::app_paths::auto_output_dir_for("region_convert");
                    match ltbox_patch::region::build_region_converted_avb_images(
                        fw_dir,
                        &region_dir,
                        device_region.to_region_target(),
                        &ltbox_patch::region::RegionPatternSet::default(),
                        Some("testkey_rsa4096"),
                    ) {
                        Ok(ltbox_patch::region::RegionAvbBuild::Built(output)) => {
                            ltbox_core::live!(
                                log,
                                "[Region] {}",
                                tr_args!(
                                    "live_region_patched",
                                    count = output.replacement_count.to_string(),
                                    path = output.vendor_boot.display().to_string()
                                )
                            );
                            resign_base = Some(output.vbmeta.clone());
                            region_vendor_boot = Some(output.vendor_boot.clone());
                        }
                        Ok(ltbox_patch::region::RegionAvbBuild::Skipped { .. }) => {
                            // Source already matches target: nothing to convert;
                            // re-sign the firmware vbmeta as in the same-region case.
                        }
                        Err(e) => {
                            if edl_start {
                                let _ = session.reset_to_edl(&mut log);
                            } else {
                                let _ = session.reset(&mut log);
                            }
                            return Err(tr_args!(
                                "err_region_conversion_failed",
                                error = e.to_string()
                            ));
                        }
                    }
                }

                let (mut overlays, _need) = build_tb323fu_arb_overlays(
                    &mut session,
                    fw_dir,
                    &arb_work_dir,
                    Some(dev.slot),
                    Some((dev.boot_floor, dev.vbs_floor)),
                    true,
                    resign_base.as_deref(),
                    &mut log,
                )?;
                if let Some(vb) = region_vendor_boot {
                    let lun = ltbox_core::partition_lun::lun_for_partition("vendor_boot_a")
                        .ok_or_else(|| "no LUN for vendor_boot_a".to_string())?;
                    let at = overlays.len().saturating_sub(1);
                    overlays.insert(at, ("vendor_boot_a".to_string(), lun, vb));
                }
                arb_patched = overlays;
                if let Some(path) = &user_abl {
                    abl_restore = Some((4, path.clone()));
                } else {
                    match backup_device_abl(&mut session, dev.slot, &arb_work_dir, &mut log) {
                        Ok(backup) => abl_restore = Some(backup),
                        Err(e) => {
                            if edl_start {
                                let _ = session.reset_to_edl(&mut log);
                            } else {
                                let _ = session.reset(&mut log);
                            }
                            return Err(e);
                        }
                    }
                }
                rb_mode = ltbox_patch::rollback::RollbackMode::Off;
            }
        }
    }
    if rb_mode == ltbox_patch::rollback::RollbackMode::Manual {
        let Some(indices) = manual_rollback_indices else {
            let err = ltbox_core::i18n::tr("rollback_manual_error_missing");
            session.reset_tolerant(&mut log);
            return Err(err);
        };
        let arb_work_dir = ltbox_core::app_paths::work_dir_for("flash_arb");
        let _ = std::fs::remove_dir_all(&arb_work_dir);
        std::fs::create_dir_all(&arb_work_dir)
            .map_err(|e| tr_args!("err_arb_work_dir_failed", error = e))?;

        for (log_name, filename, target) in [
            ("boot", "boot.img", indices.boot),
            ("vbmeta_system", "vbmeta_system.img", indices.vbmeta_system),
        ] {
            let source = fw_dir.join(filename);
            if !source.exists() {
                ltbox_core::live!(
                    log,
                    "[ARB] {}",
                    tr_args!(
                        "live_arb_skip_image_missing",
                        name = log_name,
                        file = source.display().to_string()
                    )
                );
                continue;
            }
            let image_info = match ltbox_patch::avb::extract_image_avb_info(&source) {
                Ok(info) => info,
                Err(e) => {
                    let err = tr_args!(
                        "err_patch_arb_inspect_failed",
                        image = log_name,
                        error = e.to_string()
                    );
                    ltbox_core::live!(log, "[ARB] {err}");
                    session.reset_tolerant(&mut log);
                    return Err(err);
                }
            };
            let key_from_map = match ltbox_patch::key_map::key_spec_for_signed_pubkey(
                image_info.public_key_sha1.as_deref(),
            ) {
                Ok(spec) => spec,
                Err(sha) => {
                    let err = ltbox_patch::key_map::unresolved_signing_key_error(log_name, &sha);
                    ltbox_core::live!(log, "[ARB] {err}");
                    session.reset_tolerant(&mut log);
                    return Err(err);
                }
            };
            let patched = arb_work_dir.join(format!("{log_name}.manual.img"));
            let is_vbmeta = log_name.starts_with("vbmeta");
            let patch_result = if is_vbmeta {
                match key_from_map {
                    Some(spec) => {
                        std::fs::copy(&source, &patched)
                            .map_err(|e| format!("copy vbmeta: {e}"))?;
                        ltbox_patch::avb::resign_image(
                            &patched,
                            spec,
                            &image_info.algorithm,
                            Some(target),
                        )
                        .map_err(|e| format!("resign {log_name}: {e}"))
                    }
                    None => Err(tr_args!(
                        "err_patch_arb_resign_failed",
                        image = log_name,
                        error = "unsigned image; cannot stage required ARB overlay"
                    )),
                }
            } else if image_info.algorithm == "NONE" {
                std::fs::copy(&source, &patched).map_err(|e| format!("copy chained: {e}"))?;
                ltbox_patch::avb::add_hash_footer(&patched, &image_info, key_from_map, Some(target))
                    .map_err(|e| format!("patch {log_name}: {e}"))
            } else if let Some(spec) = key_from_map {
                std::fs::copy(&source, &patched).map_err(|e| format!("copy chained: {e}"))?;
                ltbox_patch::avb::resign_image(&patched, spec, &image_info.algorithm, Some(target))
                    .map_err(|e| format!("resign {log_name}: {e}"))
            } else {
                Err(tr_args!(
                    "err_patch_arb_resign_failed",
                    image = log_name,
                    error = "unsigned image; cannot stage required ARB overlay"
                ))
            };
            if let Err(e) = patch_result {
                let err = tr_args!(
                    "live_arb_patch_failed",
                    name = log_name,
                    error = e.to_string()
                );
                ltbox_core::live!(log, "[ARB] {err}");
                session.reset_tolerant(&mut log);
                return Err(err);
            }
            let lun = ltbox_core::partition_lun::lun_for_partition(log_name)
                .ok_or_else(|| format!("no hardcoded LUN for {log_name}"))?;
            arb_patched.push((
                format!("{log_name}{}", active_slot_suffix(None)),
                lun,
                patched.clone(),
            ));
            live!(
                log,
                "[ARB] {}",
                tr_args!(
                    "live_arb_manual_prepared",
                    name = log_name,
                    path = patched.display().to_string(),
                    target = target.to_string()
                )
            );
        }
    } else if rb_mode != ltbox_patch::rollback::RollbackMode::Off && target_is_tb323fu {
        // TB323FU stages the testkey chain whenever the
        // install is a downgrade, independent of region /
        // wipe: the matching `_arb` GBL is flashed to efisp
        // below in the exact same `need` cases, so the chain
        // and its root of trust stay paired. fastboot never
        // exposes the index, so dump it over EDL, testkey
        // re-sign the four AVB partitions and stage overlays
        // (or flash stock when not a downgrade).
        let arb_work_dir = ltbox_core::app_paths::work_dir_for("flash_arb");
        let _ = std::fs::remove_dir_all(&arb_work_dir);
        std::fs::create_dir_all(&arb_work_dir)
            .map_err(|e| tr_args!("err_arb_work_dir_failed", error = e))?;
        let (overlays, need) = build_tb323fu_arb_overlays(
            &mut session,
            fw_dir,
            &arb_work_dir,
            active_slot.as_deref(),
            edl_floors,
            false,
            None,
            &mut log,
        )?;
        tb323fu_arb_need = need;
        arb_patched = overlays;
    } else if rb_mode != ltbox_patch::rollback::RollbackMode::Off {
        let arb_work_dir = ltbox_core::app_paths::work_dir_for("flash_arb");
        let _ = std::fs::remove_dir_all(&arb_work_dir);
        std::fs::create_dir_all(&arb_work_dir)
            .map_err(|e| tr_args!("err_arb_work_dir_failed", error = e))?;

        // Per-location device rollback floors. Prefer the component-wise EDL
        // floors, then a two-entry Fastboot classification: after excluding
        // recovery location 1, the lower location is vbmeta_system and the
        // higher is boot. Unknown Fastboot layouts retain the legacy aggregate
        // fallback. TB322FC keeps `None` → the per-partition `else` below skips
        // patching, which is correct since it has no rollback floor.
        let (boot_floor, vbs_floor) = match (edl_floors, fastboot_rollback_floors) {
            (Some((b, v)), _) => (Some(b), Some(v)),
            (None, Some(floors)) => (Some(floors.boot_index), Some(floors.vbmeta_system_index)),
            (None, None) => {
                let idx = match device_rollback_index {
                    Some(i) => Some(i),
                    None if is_rollback_protected_model(&device_model) => {
                        ltbox_core::live!(
                            log,
                            "[ARB] {}",
                            ltbox_core::i18n::tr("live_arb_edl_dump")
                        );
                        Some(read_device_rollback_index_via_edl(
                            &mut session,
                            active_slot.as_deref(),
                            &arb_work_dir,
                            &mut log,
                        )?)
                    }
                    None => None,
                };
                (idx, idx)
            }
        };

        // (base, on-disk filename, slot label, device floor for this location)
        let label_pairs: [(&str, &str, &str, Option<u64>); 2] = [
            ("boot", "boot.img", "boot_a", boot_floor),
            (
                "vbmeta_system",
                "vbmeta_system.img",
                "vbmeta_system_a",
                vbs_floor,
            ),
        ];
        for (log_name, filename, slot_label, loc_floor) in label_pairs {
            let Some(lun) = ltbox_core::partition_lun::lun_for_partition(log_name) else {
                ltbox_core::live!(
                    log,
                    "[ARB] {}",
                    tr_args!("live_arb_skip_no_lun", name = log_name)
                );
                if xiaoxin_pro13_flash {
                    let err = tr_args!("err_no_hardcoded_lun", partition = log_name);
                    session.reset_tolerant(&mut log);
                    return Err(err);
                }
                continue;
            };
            let source = fw_dir.join(filename);
            if !source.exists() {
                ltbox_core::live!(
                    log,
                    "[ARB] {}",
                    tr_args!(
                        "live_arb_skip_image_missing",
                        name = log_name,
                        file = source.display().to_string()
                    )
                );
                continue;
            }

            // `Off` is already bypassed; On or Auto here.
            let analysis = match ltbox_patch::rollback::analyze_rollback_with_mode(
                &source, loc_floor, rb_mode,
            ) {
                Ok(a) => a,
                Err(e) => {
                    // On/Auto require a reliable rollback decision. Failing open
                    // here would rawprogram stock images that may sit below the
                    // device floor and brick on first boot.
                    let err = tr_args!(
                        "err_patch_arb_inspect_failed",
                        image = log_name,
                        error = e.to_string()
                    );
                    ltbox_core::live!(log, "[ARB] {err}");
                    session.reset_tolerant(&mut log);
                    return Err(err);
                }
            };
            ltbox_core::live!(
                log,
                "[ARB] {}",
                tr_args!(
                    "live_arb_image_status",
                    name = log_name,
                    image = analysis.image_index.to_string(),
                    needs = analysis.needs_patch.to_string()
                )
            );
            if !analysis.needs_patch {
                continue;
            }
            if xiaoxin_pro13_flash {
                session.reset_tolerant(&mut log);
                return Err(ltbox_core::i18n::tr("err_flash_xiaoxin_arb_downgrade"));
            }
            let Some(target) = loc_floor else {
                // needs_patch implies a committed device floor under On/Auto;
                // missing one is an internal inconsistency — abort rather than
                // flash an unpatched image that still needs a raised index.
                let err = ltbox_core::i18n::tr("err_patch_arb_target_missing");
                ltbox_core::live!(
                    log,
                    "[ARB] {}",
                    tr_args!("live_arb_skip_unknown_device", name = log_name)
                );
                session.reset_tolerant(&mut log);
                return Err(err);
            };

            // Non-TB323FU rollback bypass only supports stock keys in KEY_MAP.
            // TB323FU is handled above by the GBL/efisp path.
            let key_from_map = match ltbox_patch::key_map::key_spec_for_signed_pubkey(
                analysis.image_info.public_key_sha1.as_deref(),
            ) {
                Ok(spec) => spec,
                Err(sha) => {
                    let err = ltbox_patch::key_map::unresolved_signing_key_error(log_name, &sha);
                    ltbox_core::live!(log, "[ARB] {err}");
                    session.reset_tolerant(&mut log);
                    return Err(err);
                }
            };

            let patched = arb_work_dir.join(format!("{log_name}.arb.img"));
            let is_vbmeta = log_name.starts_with("vbmeta");
            let patch_result = if is_vbmeta {
                // vbmeta always resigns (no add_hash_footer).
                match key_from_map {
                    Some(spec) => {
                        std::fs::copy(&source, &patched)
                            .map_err(|e| format!("copy vbmeta: {e}"))?;
                        ltbox_patch::avb::resign_image(
                            &patched,
                            spec,
                            &analysis.image_info.algorithm,
                            Some(target),
                        )
                        .map_err(|e| format!("resign {log_name}: {e}"))
                    }
                    None => Err(tr_args!(
                        "err_patch_arb_resign_failed",
                        image = log_name,
                        error = "unsigned image; cannot stage required ARB overlay"
                    )),
                }
            } else if analysis.image_info.algorithm == "NONE" {
                std::fs::copy(&source, &patched).map_err(|e| format!("copy chained: {e}"))?;
                ltbox_patch::avb::add_hash_footer(
                    &patched,
                    &analysis.image_info,
                    key_from_map,
                    Some(target),
                )
                .map_err(|e| format!("patch {log_name}: {e}"))
            } else if let Some(spec) = key_from_map {
                std::fs::copy(&source, &patched).map_err(|e| format!("copy chained: {e}"))?;
                ltbox_patch::avb::resign_image(
                    &patched,
                    spec,
                    &analysis.image_info.algorithm,
                    Some(target),
                )
                .map_err(|e| format!("resign {log_name}: {e}"))
            } else {
                Err(tr_args!(
                    "err_patch_arb_resign_failed",
                    image = log_name,
                    error = "unsigned image; cannot stage required ARB overlay"
                ))
            };
            if let Err(e) = patch_result {
                // needs_patch is true: a required overlay must be staged before
                // rawprogram. Log-and-continue would flash the stock image and
                // leave the device exposed to an ARB brick on reboot.
                let err = tr_args!(
                    "live_arb_patch_failed",
                    name = log_name,
                    error = e.to_string()
                );
                ltbox_core::live!(log, "[ARB] {err}");
                session.reset_tolerant(&mut log);
                return Err(err);
            }

            live!(
                log,
                "[ARB] {}",
                tr_args!(
                    "live_arb_prepared_patch",
                    name = log_name,
                    path = patched.display().to_string(),
                    target = target.to_string()
                )
            );
            arb_patched.push((slot_label.to_string(), lun, patched));
        }
    }

    // Download the efisp GBL now that the ARB dump has
    // decided stock vs `_arb` (testkey-root). The `_arb`
    // GBL is fetched whenever a downgrade re-signed the
    // chain; the normal GBL is fetched for a cross-region
    // ("Other region") provisioning install. Neither is
    // gated on data wipe — flashing efisp no longer forces
    // a data reset, so it provisions in data-keep mode too.
    // Both flash below.
    if target_is_tb323fu && (tb323fu_arb_need || cfg.modify_region) {
        // TB323FU's AVB fingerprint carries no region token; read the region
        // from the firmware vendor_boot's `product_region` DTB marker instead.
        let staged =
            efisp_suffix_for_vendor_boot(&vendor_boot, tb323fu_arb_need).and_then(|suffix| {
                fetch_efisp_asset(
                    suffix,
                    &ltbox_core::app_paths::work_dir_for("flash_efisp"),
                    "[Flash]",
                    &mut log,
                )
            });
        let efi_path = match staged {
            Ok(path) => path,
            Err(error) => {
                // No partition writes have started. Release Firehose while
                // preserving EDL for devices that started there for recovery.
                live!(log, "[Flash] {error}");
                if edl_start {
                    let _ = session.reset_to_edl(&mut log);
                } else {
                    session.reset_tolerant(&mut log);
                }
                return Err(error);
            }
        };
        efisp_efi = Some(efi_path);
    }

    live!(
        log,
        "[Flash] {} ({})",
        phases.marker(7),
        tr_args!(
            "live_flash_phase3_xml_counts",
            raw = raw_xmls.len().to_string(),
            patch = patch_xmls.len().to_string()
        )
    );
    // The final ABL overlay is brick-critical once the firmware's own
    // (Lenovo-key) abl can land: if rawprogram or an ARB overlay fails after that point, the
    // selected testkey abl must still go back, or the device is left with a
    // Lenovo-key bootloader on a testkey-resigned chain. Restore best-effort on
    // those error paths (device stays in EDL for retry); the success-path
    // restore below stays fatal.
    if let Err(e) = session.flash_rawprogram_with_wipe(&raw_xmls, &patch_xmls, cfg.wipe, &mut log) {
        let err = tr_args!("err_flash_firmware_failed", error = e.to_string());
        restore_abl_best_effort(&mut session, &abl_restore, &mut log);
        return Err(err);
    }

    // Phase 8/9 — Apply overlays and activate the target slot.
    live!(log, "[Flash] {}", phases.marker(8));

    // Overlay ARB-patched boot/vbmeta_system by GPT name.
    for (label, lun, patched) in &arb_patched {
        live!(
            log,
            "[ARB] {}",
            tr_args!("live_arb_flash_patched", label = label)
        );
        if let Err(e) = session.flash_partition(label, patched, 0, *lun, &mut log) {
            let err = tr_args!(
                "err_flash_arb_partition_failed",
                label = label,
                error = e.to_string()
            );
            restore_abl_best_effort(&mut session, &abl_restore, &mut log);
            return Err(err);
        }
    }

    // Write the selected testkey bootloader on abl_a (the device backup in the
    // normal path, or the user's ABL override). The firmware's own abl would
    // re-root the chain to the Lenovo key and reject the re-signed images; a failed restore
    // leaves the device in EDL rather than resetting into that mismatch.
    if let Some((lun, abl_img)) = &abl_restore {
        live!(
            log,
            "[ARB] {}",
            ltbox_core::i18n::tr("live_flash_abl_restore")
        );
        if let Err(e) = session.flash_partition("abl_a", abl_img, 0, *lun, &mut log) {
            return Err(tr_args!(
                "err_flash_abl_restore_failed",
                error = e.to_string()
            ));
        }
    }

    // efisp GBL, flashed immediately after the ARB overlays
    // so the testkey chain and its `_arb` root of trust are
    // provisioned together, before the best-effort region /
    // country work that can abort in between. A fetched EFI
    // (Some) is flashed: the `_arb` variant whenever a
    // downgrade re-signed the chain — fatal on failure since
    // that chain can't boot without it — or the normal
    // variant on a region-provisioning wipe (best-effort).
    // With no EFI fetched, a same-region wipe strips efisp;
    // every other mode leaves it untouched.
    if target_is_tb323fu {
        let efisp_lun = ltbox_core::partition_lun::lun_for_partition("efisp").unwrap_or(4);
        match &efisp_efi {
            Some(efi) => {
                ltbox_core::live!(
                    log,
                    "[Flash] {}",
                    ltbox_core::i18n::tr("live_flash_efisp_flash")
                );
                if let Err(e) = session.flash_partition("efisp", efi, 0, efisp_lun, &mut log) {
                    ltbox_core::live!(
                        log,
                        "[Flash] {}",
                        tr_args!("live_flash_efisp_flash_failed", error = e.to_string())
                    );
                    // A staged testkey ARB chain only boots
                    // with this `_arb` GBL. Abort loudly
                    // (device stays in EDL for retry) rather
                    // than resetting into a rollback brick.
                    if tb323fu_arb_need {
                        return Err(tr_args!(
                            "err_flash_efisp_arb_failed",
                            error = e.to_string()
                        ));
                    }
                } else {
                    ltbox_core::live!(
                        log,
                        "[Flash] {}",
                        ltbox_core::i18n::tr("live_flash_efisp_flashed")
                    );
                }
            }
            None => {
                // A downgrade always fetches the `_arb` GBL,
                // so reaching here with `need` set is an
                // internal inconsistency — fail safe.
                if tb323fu_arb_need {
                    return Err(ltbox_core::i18n::tr("err_flash_efisp_arb_missing"));
                }
                // Same-region wipe with no downgrade strips
                // the GBL; other modes leave efisp as-is.
                if cfg.wipe && !cfg.modify_region {
                    ltbox_core::live!(
                        log,
                        "[Flash] {}",
                        ltbox_core::i18n::tr("live_flash_efisp_erase")
                    );
                    if let Err(e) = session.erase_partition_by_name("efisp", 0, efisp_lun, &mut log)
                    {
                        ltbox_core::live!(
                            log,
                            "[Flash] {}",
                            tr_args!("live_flash_efisp_erase_failed", error = e.to_string())
                        );
                    } else {
                        ltbox_core::live!(
                            log,
                            "[Flash] {}",
                            ltbox_core::i18n::tr("live_flash_efisp_erased")
                        );
                    }
                }
            }
        }
    }

    // Overwrite rawprogram's stock vendor_boot/vbmeta
    // with the final region-converted AVB-valid pair.
    // This must happen after rawprogram (and after any
    // ARB overlays) so stock XML entries cannot put the
    // unconverted ROW pair back on top.
    if let Some(output) = &region_pair {
        let overlays: [(&str, &std::path::Path); 2] = [
            ("vendor_boot_a", output.vendor_boot.as_path()),
            ("vbmeta_a", output.vbmeta.as_path()),
        ];
        for (label, image) in overlays {
            let Some(lun) = ltbox_core::partition_lun::lun_for_partition(label) else {
                return Err(tr_args!("err_region_flash_no_lun", label = label));
            };
            live!(
                log,
                "[Region] {}",
                tr_args!(
                    "live_region_flashing_final",
                    label = label,
                    path = image.display().to_string()
                )
            );
            if let Err(e) = session.flash_partition(label, image, 0, lun, &mut log) {
                return Err(tr_args!(
                    "err_region_flash_failed",
                    label = label,
                    error = e.to_string()
                ));
            }
        }
    }

    // Country-code/channel patch is best-effort after firmware flash. A
    // TB376FC↔TB390FU cross-flash must flip proinfo even when no country target
    // was requested; in that case only proinfo is dumped and flashed.
    let target_code = cfg.country_action.target();
    let flip_proinfo_channel =
        xiaoxin_pro13_cross_model(&device_model, firmware_fingerprint.as_deref());
    if target_code.is_some() || flip_proinfo_channel {
        if let Some(target_code) = target_code {
            live!(
                log,
                "[Flash] {}",
                tr_args!("live_flash_country_patch_target", target = target_code)
            );
        }
        let work_dir = ltbox_core::app_paths::work_dir_for("flash_country");
        let _ = std::fs::remove_dir_all(&work_dir);
        if let Err(e) = std::fs::create_dir_all(&work_dir) {
            return Err(tr_args!(
                "err_country_work_dir_failed",
                error = e.to_string()
            ));
        }
        // Keep original region partitions for manual restore.
        let critical_backup = crate::backup::create_backup_dir("flash_firmware", &device_model)
            .map_err(|e| tr_args!("err_country_backup_dir_failed", error = e.to_string()))?;
        // Stash the bootloader's `getvar all` (incl. serialno) next to the
        // backed-up partitions — revives + supersedes v2's `sn.txt`. Empty on
        // an EDL-start flash (no fastboot probe); best-effort, never fatal.
        if !getvar_raw.is_empty() {
            let _ = std::fs::write(critical_backup.join("getvar.txt"), &getvar_raw);
        }
        // Model-specific country partitions resolve through the hardcoded
        // LUN map; start/num come from the device GPT via
        // `dump_partition_by_name`. Avoids re-decrypting
        // `rawprogram*.x` mid-flow when the catalog scratch
        // dir has been cleaned.
        // Best-effort after a successful flash: warn on a partial country
        // failure but still reset (don't strand the device in EDL).
        if let Err(e) = run_country_change(
            &mut session,
            &work_dir,
            &critical_backup,
            "flash_firmware",
            firmware_fingerprint.as_deref(),
            &device_model,
            firmware_fingerprint.as_deref(),
            target_code,
            flip_proinfo_channel,
            target_code.is_none().then_some(&["proinfo"][..]),
            &ll,
            &mut log,
            None,
        ) {
            live!(
                log,
                "[Country] {}",
                tr_args!("live_country_warning", error = e)
            );
        }
    }

    // (efisp GBL is flashed earlier — right after the ARB
    // overlays — so the testkey chain and its `_arb` root of
    // trust are provisioned before the best-effort
    // region/country work that can abort in between.)

    // Mark `_a` active before reset. Lenovo
    // firmware rawprograms only target `_a`, so
    // a full flash always lands on `_a`. Without
    // this the SoC may continue booting from a
    // previously-active `_b` on the next reset
    // and the freshly-written `_a` firmware
    // would never run.
    if let Err(e) = session.set_active_slot_a(&mut log) {
        return Err(tr_args!(
            "err_flash_set_bootable_lun_failed",
            error = e.to_string()
        ));
    }

    // Phase 9/9 — Reboot to the system.
    live!(log, "[Flash] {}", phases.marker(9));
    session.reset_tolerant(&mut log);
    live!(log, "[Flash] {}", ll.flash_completed);
    // Flash succeeded — drop the `work_*` scratch (a mid-flow abort keeps it).
    ltbox_core::app_paths::clean_work_dirs();
    Ok(log)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XiaoxinRollbackDecision {
    Proceed,
    Downgrade,
    Unreadable,
}

fn xiaoxin_rollback_decision(
    device_floors: Option<(u64, u64)>,
    firmware_indices: Option<(u64, u64)>,
) -> XiaoxinRollbackDecision {
    let (Some((device_boot, device_vbs)), Some((firmware_boot, firmware_vbs))) =
        (device_floors, firmware_indices)
    else {
        return XiaoxinRollbackDecision::Unreadable;
    };
    if device_boot > firmware_boot || device_vbs > firmware_vbs {
        XiaoxinRollbackDecision::Downgrade
    } else {
        XiaoxinRollbackDecision::Proceed
    }
}

#[cfg(test)]
mod tests {
    use super::{XiaoxinRollbackDecision, xiaoxin_rollback_decision};
    use crate::arb_overlay::{
        EFISP_EXPECTED_ASSETS, EFISP_GBL_RELEASE_TAG, efisp_expected_asset, efisp_expected_sha256,
        verify_efisp_asset,
    };

    #[test]
    fn xiaoxin_rollback_decision_fails_closed_and_rejects_either_downgrade() {
        assert_eq!(
            xiaoxin_rollback_decision(Some((11, 7)), Some((10, 7))),
            XiaoxinRollbackDecision::Downgrade
        );
        assert_eq!(
            xiaoxin_rollback_decision(Some((10, 8)), Some((10, 7))),
            XiaoxinRollbackDecision::Downgrade
        );
        assert_eq!(
            xiaoxin_rollback_decision(Some((10, 7)), Some((10, 8))),
            XiaoxinRollbackDecision::Proceed
        );
        assert_eq!(
            xiaoxin_rollback_decision(None, Some((10, 8))),
            XiaoxinRollbackDecision::Unreadable
        );
        assert_eq!(
            xiaoxin_rollback_decision(Some((10, 7)), None),
            XiaoxinRollbackDecision::Unreadable
        );
    }

    #[test]
    fn maps_suffix_to_exact_asset_name() {
        assert_eq!(
            efisp_expected_asset("_prc.efi"),
            Some("generic_superfastboot_prc.efi")
        );
        assert_eq!(
            efisp_expected_asset("_prc_arb.efi"),
            Some("generic_superfastboot_prc_arb.efi")
        );
        assert_eq!(
            efisp_expected_asset("_row.efi"),
            Some("generic_superfastboot_row.efi")
        );
        assert_eq!(
            efisp_expected_asset("_row_arb.efi"),
            Some("generic_superfastboot_row_arb.efi")
        );
        assert_eq!(efisp_expected_asset("_prc.elf"), None);
        assert_eq!(efisp_expected_asset("generic_superfastboot_prc.efi"), None);
    }

    #[test]
    fn expected_hashes_cover_all_accepted_assets() {
        assert_eq!(EFISP_GBL_RELEASE_TAG, "5.3.120-mod5");
        assert_eq!(EFISP_EXPECTED_ASSETS.len(), 4);
        for (name, hash) in EFISP_EXPECTED_ASSETS {
            assert_eq!(hash.len(), 64, "{name} hash length");
            assert!(
                hash.chars().all(|c| c.is_ascii_hexdigit()),
                "{name} hash is not hex"
            );
            assert_eq!(efisp_expected_sha256(name), Some(*hash));
        }
        assert_eq!(efisp_expected_sha256("unknown.efi"), None);
    }

    #[test]
    fn sha256_hex_file_matches_known_digest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("abc.bin");
        std::fs::write(&path, b"abc").expect("write");
        // FIPS 180-2 SHA-256("abc")
        assert_eq!(
            crate::arb_overlay::sha256_hex_file(&path).expect("hash"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn verify_rejects_unknown_name_and_hash_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("generic_superfastboot_prc.efi");
        std::fs::write(&path, b"ltbox-efisp-hash-fixture").expect("write");
        // Body is not the pinned release asset, so the pinned hash must refuse.
        assert!(verify_efisp_asset(&path, "generic_superfastboot_prc.efi").is_err());
        assert!(efisp_expected_sha256("not-a-real-asset.efi").is_none());
        assert!(verify_efisp_asset(&path, "not-a-real-asset.efi").is_err());
    }
}
