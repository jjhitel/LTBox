use crate::{App, ConnectionStatus, DevicePollResult, Message};

fn rollback_floors() -> ltbox_patch::rollback::FastbootRollbackFloors {
    ltbox_patch::rollback::FastbootRollbackFloors {
        vbmeta_system_location: 2,
        vbmeta_system_index: 0x100,
        boot_location: 3,
        boot_index: 0x200,
    }
}

fn full_poll(serial: &str, status: ConnectionStatus) -> DevicePollResult {
    DevicePollResult {
        status,
        model: format!("model-{serial}"),
        slot: "_a".to_string(),
        firmware: format!("firmware-{serial}"),
        firmware_full: format!("full-firmware-{serial}"),
        arb: format!("arb-{serial}"),
        rollback_floors: Some(rollback_floors()),
        ram: format!("ram-{serial}"),
        storage: format!("storage-{serial}"),
        market_name: format!("market-{serial}"),
        serial: serial.to_string(),
        platform_supported: Some(true),
        ..DevicePollResult::default()
    }
}

fn serial_only_poll(serial: &str, status: ConnectionStatus) -> DevicePollResult {
    DevicePollResult {
        status,
        serial: serial.to_string(),
        ..DevicePollResult::default()
    }
}

#[test]
fn serial_swap_drops_snapshot_and_invalidates_transient_ui() {
    let mut app = App::default();
    let _ = app.update(Message::DevicePolled(full_poll(
        "A",
        ConnectionStatus::Fastboot,
    )));

    app.device_info_popup = Some(("A".to_string(), crate::DeviceInfoState::Loading));
    app.ota_popup = Some((
        "A".to_string(),
        "full-firmware-A".to_string(),
        crate::OtaPopupState::Loading,
    ));
    app.qfil_popup = Some(("A".to_string(), crate::QfilPopupState::Loading));
    app.rollback_popup_open = true;
    app.flash_region_pending = Some(41);

    // A serial identifies B, but the rest of this poll is blank. B must not
    // inherit any identity or rollback data from A while the fields merge.
    let _ = app.update(Message::DevicePolled(serial_only_poll(
        "B",
        ConnectionStatus::Fastboot,
    )));

    assert_eq!(app.connection, ConnectionStatus::Fastboot);
    assert_eq!(app.device_serial, "B");
    assert!(app.device_model.is_empty());
    assert!(app.device_slot.is_empty());
    assert!(app.device_firmware.is_empty());
    assert!(app.device_firmware_full.is_empty());
    assert!(app.device_arb.is_empty());
    assert!(app.device_ram.is_empty());
    assert!(app.device_storage.is_empty());
    assert!(app.device_market_name.is_empty());
    assert!(app.device_rollback_floors.is_none());
    assert!(app.device_info_popup.is_none());
    assert!(app.ota_popup.is_none());
    assert!(app.qfil_popup.is_none());
    assert!(!app.rollback_popup_open);
    assert!(app.flash_region_pending.is_none());
    // Results already in flight for A may finish after the swap.
    let _ = app.update(Message::DeviceInfoFetched(
        "A".into(),
        Err("old result".into()),
    ));
    let _ = app.update(Message::OtaFetched(
        "A".into(),
        "full-firmware-A".into(),
        Err("old result".into()),
    ));
    let _ = app.update(Message::QfilFetched("A".into(), Err("old result".into())));
    assert!(app.device_info_popup.is_none());
    assert!(app.ota_popup.is_none());
    assert!(app.qfil_popup.is_none());
}

#[test]
fn same_serial_blank_poll_retains_identity_snapshot() {
    let mut app = App::default();
    let _ = app.update(Message::DevicePolled(full_poll(
        "A",
        ConnectionStatus::Fastboot,
    )));
    let expected_floors = app.device_rollback_floors;

    let _ = app.update(Message::DevicePolled(serial_only_poll(
        "A",
        ConnectionStatus::Fastboot,
    )));

    assert_eq!(app.device_serial, "A");
    assert_eq!(app.device_model, "model-A");
    assert_eq!(app.device_slot, "_a");
    assert_eq!(app.device_firmware, "firmware-A");
    assert_eq!(app.device_firmware_full, "full-firmware-A");
    assert_eq!(app.device_arb, "arb-A");
    assert_eq!(app.device_ram, "ram-A");
    assert_eq!(app.device_storage, "storage-A");
    assert_eq!(app.device_market_name, "market-A");
    assert_eq!(app.device_rollback_floors, expected_floors);
}

#[test]
fn serialless_edl_preserves_identity_but_different_serial_resets_it() {
    let mut app = App::default();
    let _ = app.update(Message::DevicePolled(full_poll(
        "A",
        ConnectionStatus::Fastboot,
    )));

    let _ = app.update(Message::DevicePolled(DevicePollResult {
        status: ConnectionStatus::Edl,
        ..DevicePollResult::default()
    }));

    assert_eq!(app.device_serial, "A");
    assert_eq!(app.device_model, "model-A");
    assert_eq!(app.device_firmware, "firmware-A");
    assert_eq!(app.device_ram, "ram-A");
    assert_eq!(app.device_storage, "storage-A");
    assert_eq!(app.device_market_name, "market-A");
    assert!(app.device_rollback_floors.is_none());

    let _ = app.update(Message::DevicePolled(serial_only_poll(
        "B",
        ConnectionStatus::Adb,
    )));

    assert_eq!(app.device_serial, "B");
    assert!(app.device_model.is_empty());
    assert!(app.device_firmware.is_empty());
    assert!(app.device_ram.is_empty());
    assert!(app.device_storage.is_empty());
    assert!(app.device_market_name.is_empty());
    assert!(app.device_rollback_floors.is_none());
}

#[test]
fn disconnect_clears_snapshot_and_closes_transient_ui() {
    let mut app = App::default();
    let _ = app.update(Message::DevicePolled(full_poll("A", ConnectionStatus::Adb)));
    app.device_info_popup = Some(("A".to_string(), crate::DeviceInfoState::Loading));
    app.ota_popup = Some((
        "A".to_string(),
        "full-firmware-A".to_string(),
        crate::OtaPopupState::Loading,
    ));
    app.qfil_popup = Some(("A".to_string(), crate::QfilPopupState::Loading));
    app.rollback_popup_open = true;
    app.flash_region_pending = Some(42);

    let _ = app.update(Message::DevicePolled(DevicePollResult::default()));

    assert_eq!(app.connection, ConnectionStatus::None);
    assert!(app.device_serial.is_empty());
    assert!(app.device_model.is_empty());
    assert!(app.device_slot.is_empty());
    assert!(app.device_firmware.is_empty());
    assert!(app.device_firmware_full.is_empty());
    assert!(app.device_arb.is_empty());
    assert!(app.device_ram.is_empty());
    assert!(app.device_storage.is_empty());
    assert!(app.device_market_name.is_empty());
    assert!(app.device_rollback_floors.is_none());
    assert!(app.device_info_popup.is_none());
    assert!(app.ota_popup.is_none());
    assert!(app.qfil_popup.is_none());
    assert!(!app.rollback_popup_open);
    assert!(app.flash_region_pending.is_none());
}

#[test]
fn unknown_snapshot_is_cleared_when_first_known_serial_arrives() {
    let mut app = App::default();
    let _ = app.update(Message::DevicePolled(full_poll(
        "",
        ConnectionStatus::Fastboot,
    )));

    let _ = app.update(Message::DevicePolled(serial_only_poll(
        "A",
        ConnectionStatus::Adb,
    )));

    assert_eq!(app.device_serial, "A");
    assert!(app.device_model.is_empty());
    assert!(app.device_slot.is_empty());
    assert!(app.device_firmware.is_empty());
    assert!(app.device_firmware_full.is_empty());
    assert!(app.device_arb.is_empty());
    assert!(app.device_ram.is_empty());
    assert!(app.device_storage.is_empty());
    assert!(app.device_market_name.is_empty());
    assert!(app.device_rollback_floors.is_none());
}

#[test]
fn new_serial_applies_its_available_fields_after_reset() {
    let mut app = App::default();
    let _ = app.update(Message::DevicePolled(full_poll(
        "A",
        ConnectionStatus::Fastboot,
    )));
    let _ = app.update(Message::DevicePolled(DevicePollResult {
        status: ConnectionStatus::Fastboot,
        serial: "B".into(),
        model: "model-B".into(),
        slot: "_b".into(),
        ..DevicePollResult::default()
    }));
    assert_eq!(app.device_serial, "B");
    assert_eq!(app.device_model, "model-B");
    assert_eq!(app.device_slot, "_b");
    assert!(app.device_firmware.is_empty());
    assert!(app.device_rollback_floors.is_none());
}

#[test]
fn blank_serial_fastboot_poll_does_not_prove_a_device_swap() {
    let mut app = App::default();
    let _ = app.update(Message::DevicePolled(full_poll(
        "A",
        ConnectionStatus::Fastboot,
    )));
    app.rollback_popup_open = true;
    let _ = app.update(Message::DevicePolled(serial_only_poll(
        "",
        ConnectionStatus::Fastboot,
    )));
    assert_eq!(app.device_serial, "A");
    assert_eq!(app.device_model, "model-A");
    assert_eq!(app.device_rollback_floors, Some(rollback_floors()));
    assert!(app.rollback_popup_open);
}
