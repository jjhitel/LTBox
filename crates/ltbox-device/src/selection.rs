//! Reject ambiguous Android USB targets before mode transitions or opening a transport.

use nusb::MaybeFuture;

fn is_android_interface(class: u8, subclass: u8, protocol: u8) -> bool {
    (class == 0xff && subclass == 0x42 && matches!(protocol, 1 | 3))
        || (class == 0xdc && subclass == 2 && protocol == 1)
}

/// Why automatic USB target selection cannot proceed.
#[derive(Debug, thiserror::Error)]
pub enum UsbSelectionError {
    #[error("Multiple Android/EDL USB devices found. Disconnect other devices and try again.")]
    MultipleDevices,
    #[error("USB device enumeration failed: {0}")]
    Enumeration(String),
}

fn check_candidate_count(count: usize) -> Result<(), UsbSelectionError> {
    if count > 1 {
        Err(UsbSelectionError::MultipleDevices)
    } else {
        Ok(())
    }
}

/// Counts physical device entries once even if they expose several Android
/// interfaces. Zero devices is allowed while a mode transition is in progress.
/// Transport-specific discovery still checks its own complete candidate set.
pub fn ensure_single_usb_target() -> Result<(), UsbSelectionError> {
    let devices = nusb::list_devices()
        .wait()
        .map_err(|error| UsbSelectionError::Enumeration(error.to_string()))?;
    let count = devices.filter(is_android_device).count();
    check_candidate_count(count)
}

fn is_android_device(device: &nusb::DeviceInfo) -> bool {
    if (device.vendor_id() == 0x05c6 && matches!(device.product_id(), 0x9008 | 0x900e))
        || is_android_interface(device.class(), device.subclass(), device.protocol())
        || device.interfaces().any(|interface| {
            is_android_interface(
                interface.class(),
                interface.subclass(),
                interface.protocol(),
            )
        })
    {
        return true;
    }
    // Windows only exposes interface summaries for composite usbccgp bindings.
    // Whole-device WinUSB bindings require reading configuration descriptors.
    let Ok(handle) = device.open().wait() else {
        return false;
    };
    handle.configurations().any(|config| {
        config.interfaces().any(|interface| {
            interface
                .alt_settings()
                .any(|alt| is_android_interface(alt.class(), alt.subclass(), alt.protocol()))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_modes_are_candidates_but_normal_usb_peripherals_are_not() {
        assert!(is_android_interface(0xff, 0x42, 1));
        assert!(is_android_interface(0xff, 0x42, 3));
        assert!(is_android_interface(0xdc, 2, 1));
        assert!(!is_android_interface(3, 1, 1));
        assert!(!is_android_interface(8, 6, 0x50));
        assert!(!is_android_interface(0xff, 0x42, 2));
    }

    #[test]
    fn mixed_modes_and_same_mode_pairs_are_both_ambiguous() {
        assert!(check_candidate_count(0).is_ok());
        assert!(check_candidate_count(1).is_ok());
        assert!(check_candidate_count(2).is_err());
        assert!(check_candidate_count(3).is_err());
    }
}
