//! Top-level `Message` dispatcher. Routes each variant to a focused
//! `update_*` handler or handles it inline. Extracted from `main.rs`;
//! lives in its own `impl App` block (a descendant module can still reach
//! `App`'s private fields + methods).

use crate::*;
use iced::Task;
use ltbox_core::tr_args;

mod advanced;
mod flash;
mod konabess;
mod reboot;
mod root;
mod settings;
mod sys;
mod unroot;
mod window;

impl App {
    fn open_available_release_page(&self) {
        let Some(release) = self.update_available.as_ref() else {
            return;
        };
        // `open` crate dispatches via `xdg-open` (Linux) / `start` (Windows) /
        // `open` (macOS). Failure is logged but not surfaced, matching other
        // external links in the app.
        if let Err(error) = open::that_detached(&release.html_url) {
            tracing::warn!("failed to open update URL: {error}");
        }
    }

    pub(crate) fn update(&mut self, msg: Message) -> Task<Message> {
        #[cfg(feature = "demo")]
        if demo::blocks_device_action(self, &msg) {
            return Task::none();
        }
        match msg {
            // Window chrome (titlebar buttons, cursor-drag move/resize,
            // persisted geometry) delegated to a focused handler so the
            // monster match in `update` doesn't have to spell every
            // variant out inline.
            Message::Window(m) => return self.update_window(m),
            Message::WindowResized(w, h) => return self.update_window_resized(w, h),
            Message::WindowMaximized(maximized) => {
                self.window_maximized = maximized;
            }
            Message::PersistWindowSize => return self.update_persist_window_size(),
            // Navigation
            Message::Noop => {}
            Message::ResumeBusyOperation => {
                if let Some(view) = busy_navigation_target(self.busy, self.busy_view) {
                    return self.update(Message::Navigate(view));
                }
            }
            Message::Navigate(v) => {
                if self.current_view == View::KonaBess
                    && v != View::KonaBess
                    && !self.busy
                    && !self.konabess_in_progress()
                {
                    self.konabess.reset();
                }
                self.current_view = v;
                // Keep wizard state during a running op or on the
                // exec/Done screen — sidebar bounce mid-flash must
                // not kick back to step 0.
                let busy = self.busy;
                // Skip the entry reset on the exec screen (mid-op) AND on
                // the confirm/start screen, so a sidebar bounce returns the
                // user to the confirm screen with their picks intact.
                if v == View::Root
                    && !busy
                    && !self.root.is_in_exec()
                    && !self.root.is_on_confirm_step()
                {
                    self.root.reset();
                }
                if v == View::Flash
                    && !busy
                    && !self.flash.is_in_exec()
                    && !self.flash.is_on_confirm_step()
                {
                    self.flash.reset();
                    // Fresh entry: auto-detect the hardware region (PRC/ROW)
                    // so the user lands on the target step; falls back to the
                    // manual region cards on failure. Returns early — the
                    // remaining view branches are for other views. Clearing the
                    // pending token invalidates any lookup still in flight from
                    // a prior entry.
                    self.flash_region_pending = None;
                    self.flash_serial_prompt = None;
                    return self.begin_flash_region_auto();
                }
                if v == View::SystemUpdate
                    && !busy
                    && !self.sysupdate.is_in_exec()
                    && !self.sysupdate.is_on_confirm_step()
                {
                    self.sysupdate.reset();
                }
                if v == View::Unroot
                    && !busy
                    && !self.unroot.is_in_exec()
                    && !self.unroot.is_on_confirm_step()
                {
                    self.unroot.reset();
                }
                if v == View::KonaBess
                    && !busy
                    && self.konabess.step < 2
                    && !self.konabess_in_progress()
                {
                    self.konabess.reset();
                    self.apply_default_loader_to_konabess();
                }
                // Loader pre-fill happens on the Next-into-loader-step
                // transition in `UnrootNext` (mirrors the Root wizard's
                // step-5 fill + advance pattern), not on view entry — an
                // entry-time pre-fill would make the loader step
                // unreachable when a default is set, hiding it from
                // anyone wanting to back-nav and pick a different
                // loader.
                // Advanced view: reset every sub-wizard + the generic
                // adv wizard's action / file selection on entry, so a
                // sidebar bounce mid-flow doesn't reopen the same
                // sub-wizard with the previous picked path still
                // populated. The `busy` gate covers in-flight ops.
                if v == View::Advanced && !busy && !self.advanced_in_progress() {
                    self.advanced_wizard_open = AdvancedWizardOpen::None;
                    self.adv_wizard = AdvWizard::default();
                    self.flash_parts = FlashPartsWizard::default();
                    self.dump_parts = DumpPartsWizard::default();
                    self.flash_phys = FlashPhysWizard::default();
                    self.dump_phys = DumpPhysWizard::default();
                    self.simple_flash = SimpleFlashWizard::default();
                }
                // Settings entry: rescan removable temp files so the cleanup
                // button reflects current on-disk state (enabled only when
                // there's something to clean).
                if v == View::Settings && !self.cleaning_temp {
                    return self.scan_temp_files_task();
                }
            }
            Message::SetTheme(choice) => {
                self.theme_choice = choice;
                self.dark_mode = match choice {
                    ThemeChoice::Light => false,
                    ThemeChoice::Dark => true,
                    ThemeChoice::System => theme_detect::system_prefers_dark(),
                };
                self.sync_runtime_theme();
                self.persist_settings();
            }
            Message::RefreshSystemTheme => {
                if self.theme_choice == ThemeChoice::System {
                    let dark = theme_detect::system_prefers_dark();
                    if self.dark_mode != dark {
                        self.dark_mode = dark;
                        self.sync_runtime_theme();
                        self.persist_settings();
                    }
                }
            }
            Message::ToggleLogPopup(open) => {
                self.log_popup_open = open;
            }
            // Settings dispatch delegates to a focused handler.
            Message::Settings(m) => return self.update_settings(m),
            // Flash wizard
            Message::Flash(m) => return self.update_flash(m),
            // Country code popup
            Message::SelectCountry(code) => {
                // TB322FC ships PRC-only — the Flash wizard only accepts `CN`.
                // The popup grays out other entries, but a stale dispatch could
                // still land here. Drop it — EXCEPT for the Advanced "Change
                // Country Code" op, which allows any country on any model.
                if self.is_tb322fc() && !self.adv_needs_country && !code.eq_ignore_ascii_case("CN")
                {
                    return Task::none();
                }
                self.country_popup_open = false;
                if self.adv_needs_country {
                    // Advanced wizard stores on `adv_wizard.country`.
                    self.adv_wizard.country = Some(code);
                    self.adv_needs_country = false;
                } else {
                    // Flash wizard: `wf_config` is source of truth.
                    self.wf_config.country_action = CountryAction::Set(code);
                }
            }
            Message::SkipCountryPatch => {
                // Flash wizard only — Advanced PatchDevinfo always needs a
                // target code, so the popup hides this option there.
                // Wipe mode records an explicit skip; keep-data mode leaves
                // the neutral default as Unset so the confirm row is not
                // highlighted for a no-op choice.
                self.country_popup_open = false;
                if !self.adv_needs_country {
                    self.wf_config.country_action = if self.wf_config.wipe {
                        CountryAction::Skip
                    } else {
                        CountryAction::Unset
                    };
                }
            }
            Message::DismissCountryPopup => {
                self.country_popup_open = false;
                if self.adv_needs_country {
                    self.adv_needs_country = false;
                } else if self.flash.step == 3
                    && matches!(self.wf_config.country_action, CountryAction::Unset)
                {
                    // Flash wizard — back to Data so user can switch wipe off.
                    self.flash.back();
                }
            }
            // Region-convert target picker popup
            Message::SelectRegionTarget(target) => {
                self.region_target_popup_open = false;
                self.adv_wizard.region_target = Some(target);
            }
            Message::DismissRegionTargetPopup => {
                self.region_target_popup_open = false;
            }
            // System Update wizard
            Message::Sys(m) => return self.update_sys(m),
            // Root wizard
            Message::Root(m) => return self.update_root(m),
            // Unroot wizard
            Message::Unroot(m) => return self.update_unroot(m),
            // Advanced
            Message::Adv(m) => return self.update_adv(m),
            Message::KonaBess(m) => return self.update_konabess(m),
            // Async results
            Message::FileSelected(path) => {
                if let Some(p) = path {
                    self.remember_recent(self.picker_target.kind(), &p);
                    match self.picker_target {
                        PickerTarget::RootFile => self.root.file_path = Some(p),
                        // Root loader `.melf` file — stored in
                        // `folder_path` for historical field-name reasons.
                        PickerTarget::RootLoader => self.root.folder_path = Some(p),
                        _ => {}
                    }
                }
                self.picker_target = PickerTarget::None;
            }
            Message::FolderSelected(path) => {
                if let Some(p) = path {
                    self.remember_recent(self.picker_target.kind(), &p);
                    match self.picker_target {
                        PickerTarget::UnrootFolder => self.unroot.folder_path = Some(p),
                        PickerTarget::FlashFolder => self.set_flash_firmware_folder(p),
                        _ => {}
                    }
                }
                self.picker_target = PickerTarget::None;
            }
            Message::RecentFilePicked(target, path) => {
                // Stale entries self-heal on the next real pick.
                if !std::path::Path::new(&path).is_file() {
                    return Task::none();
                }
                self.remember_recent(target.kind(), &path);
                match target {
                    PickerTarget::RootFile => self.root.file_path = Some(path),
                    PickerTarget::RootLoader => self.root.folder_path = Some(path),
                    PickerTarget::UnrootLoader => self.unroot.loader_path = Some(path),
                    _ => {}
                }
            }
            Message::RecentFolderPicked(target, path) => {
                if !std::path::Path::new(&path).is_dir() {
                    return Task::none();
                }
                self.remember_recent(target.kind(), &path);
                match target {
                    PickerTarget::UnrootFolder => self.unroot.folder_path = Some(path),
                    PickerTarget::FlashFolder => self.set_flash_firmware_folder(path),
                    _ => {}
                }
            }
            Message::NoticeRecentMissing(is_file) => {
                // Surface as the existing error banner — it already
                // overlays every view and has a dismiss button. Keep
                // out of the main log so the user's run history isn't
                // littered with picker UI noise.
                let key = if is_file {
                    "recent_missing_file"
                } else {
                    "recent_missing_folder"
                };
                self.error_msg = Some(self.t(key).to_string());
            }
            Message::OperationError(e) => {
                // Errors raised deep in the pipeline cannot name the operation,
                // so they leave `{work}` in the message. This is the one place
                // an operation error is surfaced, and the last point where the
                // running operation is still known — `fail_op` clears it.
                let e = e.replace("{work}", &self.busy_operation_label());
                self.fail_op();
                self.operation_error = Some(e.clone());
                self.error_msg = Some(e.clone());
                self.log_push(tr_args!("log_operation_error", error = e.to_string()));
            }
            Message::DismissError => self.error_msg = None,
            Message::KillAdbServer => {
                return Task::perform(
                    async {
                        tokio::task::spawn_blocking(ltbox_device::adb::kill_adb_server)
                            .await
                            .unwrap_or_else(|e| {
                                Err(ltbox_device::adb::AdbError::Client(format!(
                                    "spawn_blocking join: {e}"
                                )))
                            })
                    },
                    |res| match res {
                        Ok(()) => Message::PollDevice,
                        Err(e) => Message::OperationError(format!("Kill adb server: {e}")),
                    },
                );
            }
            Message::StartOver => {
                match self.current_view {
                    View::Root => self.root.reset(),
                    View::Flash => self.flash.reset(),
                    View::SystemUpdate => self.sysupdate.reset(),
                    View::Unroot => self.unroot.reset(),
                    View::KonaBess => {
                        self.konabess.reset();
                        self.apply_default_loader_to_konabess();
                    }
                    View::Advanced => {
                        // "Start over" on any Advanced sub-wizard should
                        // return to the Advanced grid, not step 0 of the
                        // currently open sub-flow.
                        self.advanced_wizard_open = AdvancedWizardOpen::None;
                        self.flash_parts.reset();
                        self.dump_parts.reset();
                        self.dump_phys.reset();
                        self.flash_phys.reset();
                        self.simple_flash.reset();
                        self.adv_wizard.reset();
                        self.adv_confirm_path = None;
                        self.set_image_info_log(String::new());
                    }
                    _ => {}
                }
                self.error_msg = None;
                self.operation_error = None;
            }
            Message::DrainStdoutTap => {
                // Pull from BOTH the Windows stdout pipe (`stdout_tap`,
                // which captures third-party `println!` from qdl /
                // magiskboot / pbr) AND our in-process live sink (every
                // `live!` line we emit). The pipe path can stall on GUI
                // subsystem builds — handle init order, full pipe
                // buffer back-pressure, etc. — so the in-process sink
                // is the safety net that guarantees our own log lines
                // show up regardless of OS plumbing state.
                //
                // Dedup the combined batch with a `HashSet` instead of
                // relying on `log_extend`'s adjacent-only dedup: each
                // of our `live!` lines lands in BOTH sources, so naive
                // chaining produces interleaved doubles
                // (`[A, B, C, A, B, C]`) that the adjacent walker
                // can't collapse. First-occurrence wins, so the tap
                // ordering (which interleaves third-party output with
                // ours in real chronological order) is preserved.
                self.drain_pending_log_streams();
                // Snapshot live firmware-write progress only while the
                // busy op is on the exact firmware-progress phase.
                self.refresh_flash_progress_snapshot();
                // Batched rebuild — at most one cosmic-text reshape per tick.
                if self.log_dirty {
                    self.rebuild_log_editor();
                }
            }
            Message::LogEditorAction(action) => {
                // Read-only: swallow `Edit(_)`, forward selection /
                // scroll / caret motion so drag-select + Ctrl+C work.
                // Ctrl+C goes through the widget's key binding directly.
                use iced::widget::text_editor::Action;
                if !matches!(action, Action::Edit(_)) {
                    self.log_editor.perform(action);
                }
            }
            Message::ImageInfoLogEditorAction(action) => {
                use iced::widget::text_editor::Action;
                if !matches!(action, Action::Edit(_)) {
                    self.image_info_log_editor.perform(action);
                }
            }
            Message::SaveLog => {
                let source = self.active_log_save_source();
                self.pending_log_save_source = source;
                let file_name = match source {
                    LogSaveSource::Main => "ltbox.log",
                    LogSaveSource::ImageInfo => "image_info.txt",
                };
                return Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_file_name(file_name)
                            .add_filter("Log", &["log", "txt"])
                            .save_file()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    Message::SaveLogPath,
                );
            }
            Message::SaveLogPath(path) => {
                if let Some(path) = path {
                    let source = self.pending_log_save_source;
                    let joined = self.log_text_for_save(source);
                    match std::fs::write(&path, joined) {
                        Ok(()) => self.note_log_save_result(
                            source,
                            tr_args!("log_save_succeeded", path = path.display()),
                        ),
                        Err(e) => {
                            let error = e.to_string();
                            self.error_msg =
                                Some(tr_args!("err_log_save_failed", error = error.clone()));
                            self.note_log_save_result(
                                source,
                                tr_args!("log_save_failed", error = error),
                            );
                        }
                    }
                }
            }
            // Device polling
            Message::PollDevice => {
                #[cfg(feature = "demo")]
                if let Some(result) = demo::poll_result(self) {
                    return Task::done(Message::DevicePolled(result));
                }
                return Task::perform(
                    async {
                        tokio::task::spawn_blocking(|| {
                            let mut r = DevicePollResult::default();
                            // ADB first: distinguish unauthorized /
                            // authorizing from a ready device.
                            let mut adb = ltbox_device::adb::AdbManager::new();
                            match adb.check_device_state() {
                                Ok(Some("adb_server_blocking")) => {
                                    r.status = ConnectionStatus::AdbServerBlocking;
                                    return r;
                                }
                                Ok(Some("unauthorized")) | Ok(Some("authorizing")) => {
                                    r.status = ConnectionStatus::AdbUnauthorized;
                                    return r;
                                }
                                Ok(Some("device")) | Ok(Some("recovery")) => {
                                    let raw_model =
                                        adb.get_model().ok().flatten().unwrap_or_default();
                                    // Empty model = USB-debug OFF or
                                    // auth pending (`adbd: error: closed`).
                                    // Bucket under AdbUnauthorized so
                                    // the dashboard doesn't falsely claim
                                    // the platform is unsupported.
                                    if raw_model.is_empty() {
                                        r.status = ConnectionStatus::AdbUnauthorized;
                                        return r;
                                    }
                                    // TWRP: `twrp_<model>` via `ro.product.device`.
                                    r.status = if is_twrp_product(&raw_model) {
                                        ConnectionStatus::AdbRecovery
                                    } else {
                                        ConnectionStatus::Adb
                                    };
                                    r.model = strip_twrp_prefix(&raw_model);
                                    r.slot =
                                        adb.get_slot_suffix().ok().flatten().unwrap_or_default();
                                    let fw_raw = adb
                                        .shell("getprop ro.build.display.id")
                                        .unwrap_or_default();
                                    r.firmware = trim_build_display(&fw_raw);
                                    r.firmware_full = fw_raw.trim().to_string();
                                    r.arb = arb_from_model(&r.model).to_string();
                                    let hwboard =
                                        adb.shell("getprop ro.boot.hwboardid").unwrap_or_default();
                                    if !hwboard.is_empty() {
                                        let (ram, storage) = parse_hwboardid_ram_storage(&hwboard);
                                        r.ram = ram;
                                        r.storage = storage;
                                    }
                                    r.market_name = select_device_name(|prop| {
                                        adb.shell(&format!("getprop {prop}")).unwrap_or_default()
                                    });
                                    let hw =
                                        adb.shell("getprop ro.boot.hardware").unwrap_or_default();
                                    r.platform_supported = Some(hw.to_lowercase() == "qcom");
                                    if let Some(sn) = adb.serial() {
                                        r.serial = sn.to_string();
                                    }
                                    return r;
                                }
                                _ => {
                                    // Offline / noperm / detached fall through to Fastboot/EDL.
                                }
                            }
                            // One open/claim only: a prior check_device()
                            // open+drop followed by an immediate re-open can
                            // fail transiently on Windows WinUSB and leave
                            // Fastboot status with blank fields until a later
                            // poll. Reuse the successful handle for get_all_vars.
                            if let Ok(mut dev) = ltbox_device::fastboot::FastbootDevice::open() {
                                r.status = ConnectionStatus::Fastboot;
                                let vars = dev.get_all_vars().unwrap_or_default();
                                r.model = vars.model.unwrap_or_default();
                                r.slot = vars.current_slot.unwrap_or_default();
                                let fw_raw = vars.build_display_id.unwrap_or_default();
                                r.firmware = trim_build_display(&fw_raw);
                                r.firmware_full = fw_raw.trim().to_string();
                                r.ram = vars.ram_gb.unwrap_or_default();
                                r.storage = vars.storage_gb.unwrap_or_default();
                                r.market_name = vars.product.unwrap_or_default();
                                r.serial = vars.serialno.unwrap_or_default();
                                // Bootloader mode reports the committed floors,
                                // but the Dashboard cell stays a yes/no by model
                                // like every other transport — a bare 10-digit
                                // number in a "Rollback Protection" field read as
                                // a different kind of answer than the question.
                                // The numbers move into the rollback popup, which
                                // can label and format them properly.
                                r.arb = arb_from_model(&r.model).to_string();
                                r.rollback_floors =
                                    ltbox_patch::rollback::classify_fastboot_rollback_floors(
                                        &vars.rollback_indices,
                                    );
                                return r;
                            }
                            if ltbox_device::edl::check_device() {
                                r.status = ConnectionStatus::Edl;
                            }
                            r
                        })
                        .await
                        .unwrap_or_default()
                    },
                    Message::DevicePolled,
                );
            }
            Message::DevicePolled(r) => {
                let prev_serial = self.device_serial.clone();
                let prev_status = self.connection;
                self.connection = r.status;
                if !r.model.is_empty() {
                    self.device_model = r.model;
                }
                if !r.slot.is_empty() {
                    self.device_slot = r.slot;
                }
                if !r.firmware.is_empty() {
                    self.device_firmware = r.firmware;
                }
                if !r.firmware_full.is_empty() {
                    self.device_firmware_full = r.firmware_full;
                }
                if !r.arb.is_empty() {
                    self.device_arb = r.arb;
                }
                // Same "don't clobber on a transient blank poll" rule as the
                // fields above. `get_all_vars` can come back empty on a
                // single cycle while the device is still very much in
                // bootloader mode; assigning unconditionally made the popup
                // close under the user mid-interaction. Floors are only
                // dropped once the transport actually changes.
                if r.rollback_floors.is_some() {
                    self.device_rollback_floors = r.rollback_floors;
                } else if self.connection != ConnectionStatus::Fastboot {
                    self.device_rollback_floors = None;
                    self.rollback_popup_open = false;
                }
                if !r.ram.is_empty() {
                    self.device_ram = r.ram;
                }
                if !r.storage.is_empty() {
                    self.device_storage = r.storage;
                }
                if !r.market_name.is_empty() {
                    self.device_market_name = r.market_name;
                }
                if !r.serial.is_empty() {
                    self.device_serial = r.serial;
                }
                self.platform_supported = r.platform_supported;
                if self.connection == ConnectionStatus::None {
                    self.device_model.clear();
                    self.device_slot.clear();
                    self.device_firmware.clear();
                    self.device_firmware_full.clear();
                    self.device_arb.clear();
                    self.device_rollback_floors = None;
                    self.rollback_popup_open = false;
                    self.device_ram.clear();
                    self.device_storage.clear();
                    self.device_market_name.clear();
                    self.device_serial.clear();
                    self.platform_supported = None;
                }
                // Device swap / disconnect mid-lookup: invalidate any in-flight
                // region auto-detect so a prior device's PRC/ROW answer can't
                // apply to the one now connected. Also covers a swap to a
                // serial-less state (e.g. EDL) where `device_serial` is kept
                // but the connection changed.
                if self.device_serial != prev_serial || self.connection != prev_status {
                    self.flash_region_pending = None;
                }
            }
            Message::DeviceInfoOpen => {
                let serial = self.device_serial.trim().to_string();
                if serial.is_empty() {
                    return Task::none();
                }
                if self.device_info_cache.contains_key(&serial) {
                    self.device_info_popup = Some((serial, DeviceInfoState::Ready));
                    return Task::none();
                }
                self.device_info_popup = Some((serial.clone(), DeviceInfoState::Loading));
                let serial_for_task = serial.clone();
                return task_heavy(
                    move || {
                        let result = ltbox_core::lenovo_info::fetch_machine_info(&serial_for_task)
                            .map_err(|e| e.to_string());
                        (serial_for_task, result)
                    },
                    |(s, r)| Message::DeviceInfoFetched(s, r),
                    |e| (String::new(), Err(e)),
                );
            }
            Message::DeviceInfoFetched(serial, result) => {
                if serial.is_empty() {
                    // Worker panic fallback (`task_heavy` fallback case);
                    // surface as error on whichever popup is open.
                    if let Some((s, _)) = self.device_info_popup.clone() {
                        let msg = match result {
                            Err(e) => e,
                            Ok(_) => "task panicked".to_string(),
                        };
                        self.device_info_popup = Some((s, DeviceInfoState::Error(msg)));
                    }
                    return Task::none();
                }
                match result {
                    Ok(info) => {
                        // Cache only. Flash region is no longer preselected
                        // silently here — the Flash wizard's Auto FAB is the
                        // explicit entry point for SaleArea-driven detection.
                        self.device_info_cache.insert(serial.clone(), info);
                        if matches!(&self.device_info_popup, Some((s, _)) if s == &serial) {
                            self.device_info_popup = Some((serial, DeviceInfoState::Ready));
                        }
                    }
                    Err(e) => {
                        if matches!(&self.device_info_popup, Some((s, _)) if s == &serial) {
                            self.device_info_popup = Some((serial, DeviceInfoState::Error(e)));
                        }
                    }
                }
            }
            Message::DeviceInfoRetry => {
                let Some((serial, _)) = self.device_info_popup.clone() else {
                    return Task::none();
                };
                self.device_info_popup = Some((serial.clone(), DeviceInfoState::Loading));
                let serial_for_task = serial;
                return task_heavy(
                    move || {
                        let result = ltbox_core::lenovo_info::fetch_machine_info(&serial_for_task)
                            .map_err(|e| e.to_string());
                        (serial_for_task, result)
                    },
                    |(s, r)| Message::DeviceInfoFetched(s, r),
                    |e| (String::new(), Err(e)),
                );
            }
            Message::DeviceInfoClose => {
                self.device_info_popup = None;
            }
            Message::OtaOpen => {
                self.firmware_menu_open = false;
                let serial = self.device_serial.trim().to_string();
                // Pass the untrimmed firmware id to the OTA endpoint —
                // Lenovo's `querynewfirmware` keys against the full
                // `ro.build.display.id` value (model prefix included),
                // so the dashboard's display-trimmed form would silently
                // miss every match. Fall back to the trimmed dashboard
                // value only when the full mirror is empty (older poll
                // result that never populated the field).
                let firmware_id = if !self.device_firmware_full.is_empty() {
                    self.device_firmware_full.trim().to_string()
                } else {
                    self.device_firmware.trim().to_string()
                };
                if serial.is_empty() || firmware_id.is_empty() {
                    return Task::none();
                }
                // Cache hit → restore the prior result without
                // re-issuing the upstream query. Mirrors
                // `device_info_cache` so the popup doesn't burn a
                // network round-trip every time the user reopens it
                // within the same session.
                let key = (serial.clone(), firmware_id.clone());
                if let Some(cached) = self.ota_cache.get(&key).cloned() {
                    let new_state = match cached {
                        Some(update) => OtaPopupState::Ready(update),
                        None => OtaPopupState::NoUpdate,
                    };
                    self.seed_ota_changelog_editor(&new_state);
                    self.ota_popup = Some((serial, firmware_id, new_state));
                    return Task::none();
                }
                self.ota_popup =
                    Some((serial.clone(), firmware_id.clone(), OtaPopupState::Loading));
                let s = serial.clone();
                let f = firmware_id.clone();
                return task_heavy(
                    move || {
                        let result =
                            ltbox_core::lenovo_ota::fetch_ota(&s, &f).map_err(|e| e.to_string());
                        (s, f, result)
                    },
                    |(s, f, r)| Message::OtaFetched(s, f, r),
                    |e| (String::new(), String::new(), Err(e)),
                );
            }
            Message::OtaFetched(serial, firmware_id, result) => {
                // Stale serial/firmware swap (device unplugged mid-fetch
                // and the popup was closed or reopened against another
                // device) → drop result silently.
                let still_relevant = matches!(
                    &self.ota_popup,
                    Some((s, f, _)) if s == &serial && f == &firmware_id
                );
                if !still_relevant {
                    return Task::none();
                }
                let new_state = match result {
                    Ok(Some(update)) => OtaPopupState::Ready(update),
                    Ok(None) => OtaPopupState::NoUpdate,
                    Err(e) => OtaPopupState::Error(e),
                };
                // Cache success / NoUpdate so reopening the popup
                // doesn't re-issue the same query. Errors are not
                // cached — a transient network failure should clear
                // on the next open instead of sticking until the
                // user manually retries.
                let key = (serial.clone(), firmware_id.clone());
                match &new_state {
                    OtaPopupState::Ready(u) => {
                        self.ota_cache.insert(key, Some(u.clone()));
                    }
                    OtaPopupState::NoUpdate => {
                        self.ota_cache.insert(key, None);
                    }
                    _ => {}
                }
                self.seed_ota_changelog_editor(&new_state);
                self.ota_popup = Some((serial, firmware_id, new_state));
            }
            Message::OtaClose => {
                self.ota_popup = None;
                self.ota_changelog_editor = iced::widget::text_editor::Content::with_text("");
            }
            Message::OtaChangelogAction(action) => {
                use iced::widget::text_editor::Action;
                if !matches!(action, Action::Edit(_)) {
                    self.ota_changelog_editor.perform(action);
                }
            }
            Message::OtaRetry => {
                let Some((serial, firmware_id, _)) = self.ota_popup.clone() else {
                    return Task::none();
                };
                self.ota_popup =
                    Some((serial.clone(), firmware_id.clone(), OtaPopupState::Loading));
                let s = serial.clone();
                let f = firmware_id.clone();
                return task_heavy(
                    move || {
                        let result =
                            ltbox_core::lenovo_ota::fetch_ota(&s, &f).map_err(|e| e.to_string());
                        (s, f, result)
                    },
                    |(s, f, r)| Message::OtaFetched(s, f, r),
                    |e| (String::new(), String::new(), Err(e)),
                );
            }
            Message::OpenUrl(url) => {
                // Same rationale as OtaOpenDownload: hand the URL to the host's
                // default browser rather than render an in-app webview.
                if let Err(e) = open::that_detached(url) {
                    tracing::warn!("failed to open URL {url}: {e}");
                }
            }
            Message::OtaOpenDownload(url) => {
                // `open::that_detached` hands the URL to the host's
                // default URL handler — Edge / Firefox / GNOME's
                // xdg-open chain — so the user gets a real browser
                // tab, not an in-app webview that we'd have to render
                // and security-audit.
                if let Err(e) = open::that_detached(&url) {
                    tracing::warn!("failed to open OTA download URL: {e}");
                }
            }
            Message::OpenExternalUrl(url) => {
                if let Err(e) = open::that_detached(&url) {
                    tracing::warn!("failed to open URL {url}: {e}");
                }
            }
            Message::FirmwareMenu(open) => {
                self.firmware_menu_open = open;
            }
            Message::QfilOpen => {
                self.firmware_menu_open = false;
                let serial = self.device_serial.trim().to_string();
                if serial.is_empty() {
                    return Task::none();
                }
                // Cache hit → restore the prior result without re-querying.
                if let Some(cached) = self.qfil_cache.get(&serial).cloned() {
                    self.qfil_popup = Some((serial, cached));
                    return Task::none();
                }
                self.qfil_popup = Some((serial.clone(), QfilPopupState::Loading));
                return self.spawn_qfil_fetch(serial);
            }
            Message::QfilFetched(serial, result) => {
                if serial.is_empty() {
                    // Worker panic fallback — surface on whichever popup is open.
                    if let Some((s, _)) = self.qfil_popup.clone() {
                        let msg = match result {
                            Err(e) => e,
                            Ok(_) => "task panicked".to_string(),
                        };
                        self.qfil_popup = Some((s, QfilPopupState::Error(msg)));
                    }
                    return Task::none();
                }
                let new_state = match result {
                    Ok(QfilOutcome::Global) => QfilPopupState::Global,
                    Ok(QfilOutcome::NoPackage) => QfilPopupState::NoPackage,
                    Ok(QfilOutcome::Package(p)) => QfilPopupState::Ready(p),
                    Err(e) => QfilPopupState::Error(e),
                };
                // Cache every non-error outcome so reopening never re-queries.
                if !matches!(new_state, QfilPopupState::Error(_)) {
                    self.qfil_cache.insert(serial.clone(), new_state.clone());
                }
                if matches!(&self.qfil_popup, Some((s, _)) if s == &serial) {
                    self.qfil_popup = Some((serial, new_state));
                }
            }
            Message::QfilClose => {
                self.qfil_popup = None;
            }
            Message::QfilRetry => {
                let Some((serial, _)) = self.qfil_popup.clone() else {
                    return Task::none();
                };
                self.qfil_popup = Some((serial.clone(), QfilPopupState::Loading));
                return self.spawn_qfil_fetch(serial);
            }
            Message::CopyToClipboard(payload) => {
                let toast = self.t("toast_copied").to_string();
                return iced::clipboard::write::<Message>(payload)
                    .chain(Task::done(Message::ToastShow(toast)));
            }
            Message::ToastShow(msg) => {
                self.toast_msg = Some(msg);
                return Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
                    },
                    |_| Message::ToastClear,
                );
            }
            Message::ToastClear => {
                self.toast_msg = None;
            }
            Message::SidebarHoverEnter => {
                self.sidebar_expanded = true;
            }
            Message::SidebarHoverExit => {
                self.sidebar_expanded = false;
            }
            Message::SidebarAnimTick => {
                // M3 Expressive Spatial spring: critically damped enough
                // that navigation doesn't oscillate, with a touch of
                // overshoot at hover-exit so the rail "snaps" closed.
                // stiffness=180, damping_ratio≈0.85 → damping ≈ 22.8.
                const STIFFNESS: f32 = 180.0;
                const DAMPING: f32 = 22.8;
                const DT: f32 = 0.016;
                let target = self.sidebar_anim_target();
                let displacement = target - self.sidebar_anim;
                let force = displacement * STIFFNESS;
                let damp = -self.sidebar_velocity * DAMPING;
                self.sidebar_velocity += (force + damp) * DT;
                let next = self.sidebar_anim + self.sidebar_velocity * DT;
                // Settle: both displacement AND velocity near zero.
                // Avoids clipping the tail of the spring response.
                if displacement.abs() < 0.001 && self.sidebar_velocity.abs() < 0.05 {
                    self.sidebar_anim = target;
                    self.sidebar_velocity = 0.0;
                } else {
                    self.sidebar_anim = next.clamp(-0.05, 1.05);
                }
            }
            Message::RollbackDetailOpen => {
                // Guarded so a stale click during a transport change can't
                // open a popup with nothing to show.
                if self.device_rollback_floors.is_some() {
                    self.rollback_popup_open = true;
                    self.rollback_value_format = RollbackValueFormat::default();
                }
            }
            Message::RollbackDetailClose => {
                self.rollback_popup_open = false;
            }
            Message::RollbackDetailCycleFormat => {
                self.rollback_value_format = self.rollback_value_format.next();
            }
            Message::DriverCheckDone(status) => {
                self.driver_status = Some(status);
            }
            Message::ConnectivityChecked(online) => {
                self.online = Some(online);
            }
            Message::StartupConnectivityProbed(report) => {
                // A dead link already implies GitHub is dead, so only the
                // more specific of the two lines is worth logging.
                if !report.internet {
                    self.log_push(ltbox_core::i18n::tr("live_startup_no_internet"));
                } else if !report.github {
                    self.log_push(ltbox_core::i18n::tr("live_startup_no_github"));
                }
            }
            Message::DriverUpdateCheckDone(update) => {
                // Respect a dismissal that may have landed between the
                // startup spawn and this result arriving.
                if !self.qcom_driver_update_dismissed {
                    self.driver_update = update;
                }
            }
            Message::DismissDriverUpdate => {
                self.qcom_driver_update_dismissed = true;
                self.driver_update = None;
                self.persist_settings();
            }
            Message::DismissDualUsbAdvisory(model) => {
                if !self
                    .dual_usb_advisory_dismissed
                    .iter()
                    .any(|m| m.eq_ignore_ascii_case(&model))
                {
                    self.dual_usb_advisory_dismissed.push(model);
                    self.persist_settings();
                }
            }
            Message::CloseDualUsbAdvisory(model) => {
                if !self
                    .dual_usb_advisory_closed
                    .iter()
                    .any(|m| m.eq_ignore_ascii_case(&model))
                {
                    self.dual_usb_advisory_closed.push(model);
                }
            }
            Message::UpdateCheckDone(result) => {
                // `None` means "no banner" — either we're already on the
                // latest stable, the repo has only prereleases, or the
                // probe failed (offline / 5xx / parse). All three should
                // render identically: nothing in the sidebar.
                self.update_available = result;
            }
            Message::OpenUpdate => {
                let source = ltbox_core::install_source::install_source();
                match source {
                    ltbox_core::install_source::InstallSource::Direct => {
                        self.direct_update_state = DirectUpdateState::Ready;
                        self.update_dialog_source = Some(source);
                    }
                    _ => {
                        self.update_dialog_source = Some(source);
                    }
                }
            }
            Message::UpdateDialogClose => {
                if !self.direct_update_state.is_active() {
                    self.update_dialog_source = None;
                }
            }
            Message::InstallSelfUpdate => {
                if self.update_dialog_source
                    != Some(ltbox_core::install_source::InstallSource::Direct)
                    || self.direct_update_state.is_active()
                {
                    return Task::none();
                }
                let Some(release) = self.update_available.as_ref() else {
                    return Task::none();
                };
                let tag = release.tag.clone();
                self.direct_update_state = DirectUpdateState::Updating;
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            self_update::install_release_and_restart(tag)
                        })
                        .await
                        .unwrap_or_else(|_| {
                            Err(SelfUpdateFailure {
                                kind: SelfUpdateFailureKind::Swap,
                                detail: ltbox_core::i18n::tr("err_task_panicked"),
                            })
                        })
                    },
                    Message::SelfUpdateFinished,
                );
            }
            Message::SelfUpdateFinished(result) => match result {
                Ok(()) => {
                    self.direct_update_state = DirectUpdateState::Restarting;
                    return Task::perform(
                        async {
                            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                        },
                        |_| Message::ExitAfterUpdate,
                    );
                }
                Err(error) => {
                    self.direct_update_state = DirectUpdateState::Failed(error);
                }
            },
            Message::ExitAfterUpdate => return iced::exit(),
            Message::OpenUpdateReleasePage => self.open_available_release_page(),
            Message::InstallDrivers => {
                if self.installing_drivers {
                    return Task::none();
                }
                self.installing_drivers = true;
                self.log_push(format!("[Driver] {}", self.t("live_driver_starting")));
                return Task::perform(
                    async {
                        tokio::task::spawn_blocking(|| {
                            let mut log = Vec::new();
                            match ltbox_device::driver::download_and_install(&mut log) {
                                Ok(()) => Ok(log),
                                Err(e) => {
                                    ltbox_core::live!(
                                        log,
                                        "[Driver] {}",
                                        tr_args!("live_driver_failed", error = e.to_string())
                                    );
                                    Err(format!("{e}"))
                                }
                            }
                        })
                        .await
                        .unwrap_or_else(|_| Err(ltbox_core::i18n::tr("err_task_panicked")))
                    },
                    Message::InstallDriversDone,
                );
            }
            Message::FlashParts(m) => return self.update_flash_parts(m),
            Message::DumpParts(m) => return self.update_dump_parts(m),
            // -- Physical Storage: Dump --------------------------------------
            Message::DumpPhys(m) => return self.update_dump_phys(m),
            // -- Physical Storage: Flash -------------------------------------
            Message::FlashPhys(m) => return self.update_flash_phys(m),
            // -- Simple Firmware Flash (stock-equivalent, no checks) ----------
            Message::SimpleFlash(m) => return self.update_simple_flash(m),
            Message::Reboot(m) => return self.update_reboot(m),
            Message::InstallDriversDone(result) => {
                self.installing_drivers = false;
                // Drain any lines still pending in the sink/tap so the
                // worker's terminal `live_driver_install_finished`
                // line lands before the banner re-check fires. Don't
                // append a separate `driver_install_done` line — the
                // worker already emitted a localized completion line
                // (`Installation finished (N/N succeeded)`), so the
                // extra log_push here was a near-duplicate of the same
                // message in a different wording.
                let _ = self.drain_pending_log_streams();
                match result {
                    Ok(_log) => {
                        // Install/update just brought the driver to the
                        // latest release, so drop any outstanding update
                        // banner. The presence re-check below clears the
                        // missing banner.
                        self.driver_update = None;
                        return Task::perform(
                            async {
                                tokio::task::spawn_blocking(
                                    ltbox_device::driver::check_required_drivers,
                                )
                                .await
                                .unwrap_or(ltbox_device::driver::DriverStatus::NotWindows)
                            },
                            Message::DriverCheckDone,
                        );
                    }
                    Err(e) => {
                        self.log_lines
                            .push(tr_args!("driver_install_failed", e = e));
                        self.error_msg = Some(tr_args!("driver_install_failed", e = e));
                    }
                }
            }
        }
        Task::none()
    }
}
