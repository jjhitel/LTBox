use super::*;

/// Simple firmware flash: flash a firmware folder exactly like a stock
/// Lenovo flash script, skipping every LTBox-side check and modification.
///
/// Unlike [`flash_worker`] this performs **no** fingerprint/model check, **no**
/// signing-key check, and **no** region / rollback / country / wipe handling.
/// It only: decrypts the firmware's own `rawprogram*.x` pack to `.xml`,
/// transitions to EDL per the current connection mode, and flashes the
/// selected rawprogram + patch XMLs verbatim. The XML selection is the *same*
/// [`collect_firmware_xmls_for_flash`](ltbox_device::edl::collect_firmware_xmls_for_flash)
/// the full flash uses, so the persist-less LUN0 rawprogram stays prioritized
/// and only it is included. Whether user data is wiped is therefore decided
/// solely by the firmware package, not by LTBox.
pub(crate) fn simple_flash_worker(
    conn: ConnectionStatus,
    fw_folder: String,
    phases: PhaseReporter,
) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    live!(log, "[SimpleFlash] {}", phases.marker(1));
    let fw_dir = std::path::Path::new(&fw_folder);

    // 1. Validate firmware folder.
    if !fw_dir.exists() {
        return Err(tr_args!(
            "err_flash_firmware_folder_missing",
            path = fw_folder
        ));
    }
    live!(
        log,
        "[SimpleFlash] {}",
        tr_args!("live_flash_firmware_folder", path = fw_folder)
    );

    // 2. Decrypt the firmware's own `rawprogram*.x` pack to `.xml` so the
    //    catalog scan below can read it. The encrypted Sahara manifest
    //    (`qsahara_device_programmer.x`) is a loader, not a flash image, so it
    //    is left for `EdlSession::open` to decrypt at load time. This unpacks
    //    the firmware as shipped — it is not a content modification.
    decrypt_rawprogram_x_files(fw_dir, &mut log)?;

    // 2b. Decompress any `*.zst` partition images (e.g. `super.img.zst`) so the
    //     rawprogram references resolve — same as the full flash, and before
    //     EDL so a multi-GB decompress doesn't hold the session open.
    decompress_zst_images(fw_dir, &mut log)?;

    // 3. Locate the EDL loader inside the firmware folder (or its parent).
    //    A missing loader is a hard error — nothing can be flashed, so the run
    //    must fail rather than report success.
    let loader = require_firmware_loader(find_firmware_loader(fw_dir))?;

    // 4. XML selection — identical to the full firmware flash so the
    //    persist-less rawprogram0 stays first and only it is included.
    let (raw_xmls, patch_xmls) = ltbox_device::edl::collect_firmware_xmls_for_flash(fw_dir, false)
        .map_err(|e| tr_args!("err_flash_xml_selection_failed", error = e.to_string()))?;
    if raw_xmls.is_empty() {
        return Err(tr_args!("err_flash_no_rawprogram_xml", path = fw_folder));
    }

    // 5. Transition to EDL using the shared live-probe path (re-probes the
    //    current transport rather than trusting the captured snapshot), then
    //    open the session — same entry path normal firmware flashing uses.
    live!(log, "[SimpleFlash] {}", phases.marker(2));
    transition_to_edl(conn, &mut log)?;
    let mut session = open_edl_session(&loader, &mut log)?;

    // 6. Flash verbatim — no FP check, no signing-key check, no region / ARB /
    //    country edits, no keep-data skip.
    live!(log, "[SimpleFlash] {}", phases.marker(3));
    live!(
        log,
        "[SimpleFlash] {}",
        tr_args!(
            "live_flash_phase3_xml_counts",
            raw = raw_xmls.len().to_string(),
            patch = patch_xmls.len().to_string()
        )
    );
    session
        .flash_rawprogram_verbatim(&raw_xmls, &patch_xmls, &mut log)
        .map_err(|e| tr_args!("err_flash_firmware_failed", error = e.to_string()))?;

    // 7. Mark `_a` active before reset (Lenovo rawprograms only target `_a`),
    //    same as the stock script / full flash so the device boots the
    //    freshly-written slot on the next reset.
    live!(log, "[SimpleFlash] {}", phases.marker(4));
    if let Err(e) = session.set_active_slot_a(&mut log) {
        return Err(tr_args!(
            "err_flash_set_bootable_lun_failed",
            error = e.to_string()
        ));
    }
    live!(log, "[SimpleFlash] {}", phases.marker(5));
    session.reset_tolerant(&mut log);
    live!(
        log,
        "[SimpleFlash] {}",
        ltbox_core::i18n::tr("live_flash_completed")
    );
    ltbox_core::app_paths::clean_work_dirs();
    Ok(log)
}
