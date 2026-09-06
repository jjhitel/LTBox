//! ARB (anti-rollback) detection worker and its UTC timestamp helpers.
//! Extracted from main.rs.

use crate::*;

/// Format a unix timestamp (seconds) as `YYYY-MM-DD HH:MM:SS UTC`.
/// Pure stdlib, independent of the local-time backup naming. Uses Howard Hinnant's civil-from-days
/// algorithm so the proleptic Gregorian conversion stays correct
/// across leap years and century boundaries without a calendar table.
pub(crate) fn format_unix_timestamp_utc(ts: u64) -> String {
    let days = (ts / 86_400) as i64;
    let rem = (ts % 86_400) as u32;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
}

/// Date-only rendering of a rollback index (`YYYY-MM-DD`, UTC). The
/// rollback-index popup cycles through this as its most human form —
/// the time-of-day component carries no meaning for a rollback floor.
pub(crate) fn format_unix_date_utc(ts: u64) -> String {
    let (y, mo, d) = civil_from_days((ts / 86_400) as i64);
    format!("{y:04}-{mo:02}-{d:02}")
}

/// Howard Hinnant `civil_from_days`: (days since 1970-01-01) →
/// `(year, month, day)` in the proleptic Gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Worker for the Advanced → Detect Anti-Rollback flow. Mirrors the
/// flash wizard's ARB probe but in a manual, report-only shape:
///
/// 1. Reach fastboot (reboot from ADB if needed).
/// 2. Read `stored_rollback_index:N` vars. Any entry whose value is
///    not 0 / 1 makes the device anti-rollback; the report lists each
///    surviving entry as `stored_rollback_index:N = TS (UTC)`.
/// 3. If no `stored_rollback_index` was reported AND the model
///    enforces rollback protection (every supported model except
///    `TB322FC`), fall back to dumping the active-slot `boot` +
///    `vbmeta_system` over EDL using the user-picked Firehose loader
///    and report their AVB rollback indices the same way.
/// 4. Otherwise (no stored_rollback_index, or `TB322FC`) the device
///    is not anti-rollback.
/// 5. Always reboot to system at the end so the user can keep using
///    the device.
#[allow(clippy::too_many_arguments)]
pub(crate) fn detect_arb_run(
    conn: ConnectionStatus,
    device_model: String,
    loader_path: Option<String>,
    i_anti: &str,
    i_not: &str,
    i_reboot_fastboot: &str,
    i_reboot_system: &str,
    i_edl_dump: &str,
    phases: PhaseReporter,
    log: &mut Vec<String>,
) -> std::result::Result<(), String> {
    use ltbox_device::adb::AdbManager;
    use ltbox_device::fastboot::FastbootDevice;

    // Step 1: ensure we are in fastboot.
    ltbox_core::live!(log, "[ARB] {}", phases.marker(1));
    if !matches!(conn, ConnectionStatus::Fastboot) {
        match conn {
            ConnectionStatus::Adb | ConnectionStatus::AdbRecovery => {
                ltbox_core::live!(log, "[ARB] {i_reboot_fastboot}");
                let mut adb = AdbManager::new();
                if !adb.check_device().unwrap_or(false) {
                    return Err("ADB device not reachable".into());
                }
                let _ = adb.shell("reboot bootloader");
                std::thread::sleep(std::time::Duration::from_secs(5));
                if FastbootDevice::wait_for_device().is_err() {
                    return Err("Failed to enter fastboot".into());
                }
            }
            _ => {
                return Err(
                    "Device must be in ADB or fastboot to run anti-rollback detection".into(),
                );
            }
        }
    }

    // Step 2: read fastboot vars (rollback_indices map is the source
    // of truth — its emptiness drives the model-specific fallback).
    ltbox_core::live!(log, "[ARB] {}", phases.marker(2));
    let vars = FastbootDevice::open()
        .and_then(|mut d| d.get_all_vars())
        .map_err(|e| format!("fastboot vars: {e}"))?;

    let stored_present = !vars.rollback_indices.is_empty();
    if stored_present {
        ltbox_core::live!(log, "[ARB] {}", phases.marker(4));
        let mut filtered: Vec<(u32, u64)> = vars
            .rollback_indices
            .iter()
            .filter(|&(_, &v)| v != 0 && v != 1)
            .map(|(k, v)| (*k, *v))
            .collect();
        filtered.sort_by_key(|(k, _)| *k);
        ltbox_core::live!(log, "");
        ltbox_core::live!(log, "{i_anti}");
        ltbox_core::live!(log, "");
        for (idx, ts) in &filtered {
            let utc = format_unix_timestamp_utc(*ts);
            ltbox_core::live!(log, "stored_rollback_index:{idx} = {ts} ({utc})");
        }
        ltbox_core::live!(log, "");
        ltbox_core::live!(log, "[ARB] {}", phases.marker(5));
        ltbox_core::live!(log, "[ARB] {i_reboot_system}");
        if let Ok(mut dev) = FastbootDevice::open() {
            let _ = dev.reboot();
        }
        return Ok(());
    }

    // Step 3: every model except the no-ARB TB322FC enforces rollback
    // protection but may not expose `stored_rollback_index` over fastboot —
    // read the ACTIVE-slot boot + vbmeta_system indices over EDL. (TB322FC
    // falls through to step 4 / "no anti-rollback".)
    if is_rollback_protected_model(&device_model) {
        ltbox_core::live!(log, "[ARB] {}", phases.marker(3));
        let Some(loader) = loader_path else {
            return Err("An EDL loader is required for the deeper rollback inspection".into());
        };
        ltbox_core::live!(log, "[ARB] {i_edl_dump}");
        if ensure_edl(ConnectionStatus::Fastboot, "ARB", log).is_err() {
            return Err("Failed to enter EDL".into());
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
        let loader_pb = std::path::PathBuf::from(&loader);
        let mut session = open_edl_session(&loader_pb, log)?;
        // Read the active slot (a first-time user may be on `_b`).
        let slot = active_slot_suffix(vars.current_slot.as_deref());
        let boot_part = format!("boot{slot}");
        let vbm_part = format!("vbmeta_system{slot}");
        let tmp = std::env::temp_dir();
        let boot_out = tmp.join(format!("ltbox_arb_{boot_part}.img"));
        let vbm_out = tmp.join(format!("ltbox_arb_{vbm_part}.img"));
        // boot → LUN 4, vbmeta_system → LUN 0 per the hardcoded LUN map.
        session
            .dump_partition(&boot_part, &boot_out, 0, 4, log)
            .map_err(|e| format!("dump {boot_part}: {e}"))?;
        session
            .dump_partition(&vbm_part, &vbm_out, 0, 0, log)
            .map_err(|e| format!("dump {vbm_part}: {e}"))?;
        let boot_idx = ltbox_patch::avb::extract_image_avb_info(&boot_out)
            .map_err(|e| format!("boot AVB: {e}"))?
            .rollback_index;
        let vbm_idx = ltbox_patch::avb::extract_image_avb_info(&vbm_out)
            .map_err(|e| format!("vbmeta_system AVB: {e}"))?
            .rollback_index;
        let _ = std::fs::remove_file(&boot_out);
        let _ = std::fs::remove_file(&vbm_out);
        ltbox_core::live!(log, "[ARB] {}", phases.marker(4));
        ltbox_core::live!(log, "");
        ltbox_core::live!(log, "{i_anti}");
        ltbox_core::live!(log, "");
        ltbox_core::live!(
            log,
            "{boot_part} = {boot_idx} ({})",
            format_unix_timestamp_utc(boot_idx)
        );
        ltbox_core::live!(
            log,
            "{vbm_part} = {vbm_idx} ({})",
            format_unix_timestamp_utc(vbm_idx)
        );
        ltbox_core::live!(log, "");
        ltbox_core::live!(log, "[ARB] {}", phases.marker(5));
        ltbox_core::live!(log, "[ARB] {i_reboot_system}");
        session.reset_tolerant(log);
        return Ok(());
    }

    // Step 4: no stored_rollback_index, no TB320FC override.
    ltbox_core::live!(log, "[ARB] {}", phases.marker(4));
    ltbox_core::live!(log, "");
    ltbox_core::live!(log, "{i_not}");
    ltbox_core::live!(log, "");
    ltbox_core::live!(log, "[ARB] {}", phases.marker(5));
    ltbox_core::live!(log, "[ARB] {i_reboot_system}");
    if let Ok(mut dev) = FastbootDevice::open() {
        let _ = dev.reboot();
    }
    Ok(())
}
