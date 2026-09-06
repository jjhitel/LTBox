//! Active-slot resolution across the ADB and Fastboot transports.

use crate::adb::AdbManager;
use crate::fastboot::FastbootDevice;
use ltbox_core::{i18n::tr, tr_args};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ControllerError {
    #[error("{0}")]
    SlotResolve(String),
}

/// Poll ADB then Fastboot for the active slot suffix until one
/// returns `_a` or `_b`, or the deadline expires.
///
/// Slot is required for every flash / dump / root path: writing to
/// the wrong slot's `boot_*` / `vbmeta_*` / `init_boot_*` partition
/// either fails AVB on the next boot (if the device flips slots
/// post-flash) or quietly leaves the device on the unmodified slot
/// (if it doesn't). Defaulting to `_a` when probing fails was a
/// silent footgun — flashes landed on `_a` while the device was
/// running on `_b`, so the user saw "flash succeeded" but nothing
/// changed. Force a hard error instead so the caller has to fix the
/// transport state before any destructive op runs.
///
/// Polls both transports because the device's state mid-flow
/// determines which one answers: ADB works in normal / recovery,
/// Fastboot works in bootloader. EDL has no slot getvar — caller
/// must probe BEFORE entering EDL.
///
/// `log` receives one human-readable line per poll attempt
/// (suppressed via the standard `live!` macro contract — drop the
/// `Vec` in headless callers).
pub fn poll_active_slot(
    timeout: std::time::Duration,
    log: &mut Vec<String>,
) -> std::result::Result<String, ControllerError> {
    let deadline = std::time::Instant::now() + timeout;
    let mut adb_attempted = false;
    let mut fastboot_attempted = false;
    let mut last_adb_err = String::new();
    let mut last_fastboot_err = String::new();

    while std::time::Instant::now() < deadline {
        // ADB attempt — only if device is currently in a state that
        // accepts shell (Device or Recovery).
        let mut adb = AdbManager::new();
        match adb.check_device_state() {
            Ok(Some(state @ ("device" | "recovery"))) => {
                adb_attempted = true;
                match adb.get_slot_suffix() {
                    Ok(Some(s)) if s == "_a" || s == "_b" => {
                        ltbox_core::live!(
                            log,
                            "[Slot] {}",
                            ltbox_core::tr_args!("log_slot_resolved_adb", state = state, slot = s,)
                        );
                        return Ok(s);
                    }
                    Ok(Some(other)) => {
                        last_adb_err = tr_args!("slot_err_adb_unexpected", slot = other);
                    }
                    Ok(None) => {
                        last_adb_err = tr("slot_err_adb_empty");
                    }
                    Err(e) => {
                        last_adb_err = tr_args!("slot_err_adb_shell_failed", error = e);
                    }
                }
            }
            Ok(Some(state)) => {
                last_adb_err = tr_args!("slot_err_adb_state_no_shell", state = state);
            }
            Ok(None) => {
                last_adb_err = tr("slot_err_adb_no_device");
            }
            Err(e) => {
                last_adb_err = tr_args!("slot_err_adb_probe_failed", error = e);
            }
        }

        // Fastboot attempt — open() fails fast if the device isn't
        // in bootloader, so no separate state probe.
        match FastbootDevice::open() {
            Ok(mut fb) => {
                fastboot_attempted = true;
                match fb.get_slot_suffix() {
                    Ok(Some(s)) if s == "_a" || s == "_b" => {
                        ltbox_core::live!(
                            log,
                            "[Slot] {}",
                            ltbox_core::tr_args!("log_slot_resolved_fastboot", slot = s)
                        );
                        return Ok(s);
                    }
                    Ok(Some(other)) => {
                        last_fastboot_err = tr_args!("slot_err_fastboot_unexpected", slot = other);
                    }
                    Ok(None) => {
                        last_fastboot_err = tr("slot_err_fastboot_empty");
                    }
                    Err(e) => {
                        last_fastboot_err = tr_args!("slot_err_fastboot_getvar_failed", error = e);
                    }
                }
            }
            Err(e @ crate::fastboot::FastbootError::MultipleDevices) => {
                return Err(ControllerError::SlotResolve(e.to_string()));
            }
            Err(e) => {
                last_fastboot_err = tr_args!("slot_err_fastboot_open_failed", error = e);
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    // Build a diagnostic that surfaces what was tried + the last
    // failure mode per transport so the user knows whether to plug
    // ADB cable, reboot to bootloader, or fix permissions.
    let mut detail = String::new();
    if adb_attempted {
        detail.push_str(&tr_args!("slot_err_adb_detail", error = last_adb_err));
    } else {
        detail.push_str(&tr("slot_err_adb_never_shell"));
    }
    detail.push(' ');
    if fastboot_attempted {
        detail.push_str(&tr_args!(
            "slot_err_fastboot_detail",
            error = last_fastboot_err
        ));
    } else {
        detail.push_str(&tr("slot_err_fastboot_never"));
    }
    Err(ControllerError::SlotResolve(tr_args!(
        "err_active_slot_detect_failed",
        timeout = format!("{timeout:?}"),
        detail = detail
    )))
}
