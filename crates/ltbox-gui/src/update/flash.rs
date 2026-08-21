//! Firmware-flash wizard handler. Extracted from `main.rs`.

use crate::*;
use iced::Task;

impl App {
    pub(crate) fn update_flash(&mut self, msg: FlashMsg) -> Task<Message> {
        match msg {
            FlashMsg::FlashRegion(r) => {
                // TB322FC is a PRC-only SKU. The region card UI grays
                // out ROW, but a stale message from a pre-poll click
                // could still land here. Drop it so the wizard never
                // accepts a region the hardware doesn't ship with.
                if self.is_tb322fc() && r == DeviceRegion::Row {
                    return Task::none();
                }
                self.flash.device_region = Some(r);
                Task::none()
            }
            FlashMsg::FlashSerialPromptInput(s) => {
                if let Some(buf) = &mut self.flash_serial_prompt {
                    *buf = s;
                }
                Task::none()
            }
            FlashMsg::FlashSerialPromptSkip => {
                // Dismiss → the region step shows the manual PRC/ROW cards
                // (not probing, no prompt).
                self.flash_serial_prompt = None;
                Task::none()
            }
            FlashMsg::FlashSerialPromptSubmit => {
                let Some(buf) = self.flash_serial_prompt.take() else {
                    return Task::none();
                };
                let serial = buf.trim().to_string();
                if serial.is_empty() {
                    // Nothing entered — keep the prompt open.
                    self.flash_serial_prompt = Some(buf);
                    return Task::none();
                }
                self.start_region_probe(serial)
            }
            FlashMsg::FlashAutoRegionFetched(id, serial, result) => {
                // Ignore a superseded lookup — a newer probe (re-entry, device
                // swap, or a fresh manual serial) has taken over. Leaves the
                // active probe's progress indicator untouched.
                if self.flash_region_pending != Some(id) {
                    return Task::none();
                }
                self.flash_region_pending = None;
                match result {
                    Ok(info) => {
                        let region = region_from_salearea(&info);
                        if !serial.is_empty() {
                            self.device_info_cache.insert(serial, info);
                        }
                        // Only touch the selection while still on the region
                        // step — never retroactively change a region the user
                        // has already advanced past.
                        if self.flash.step != 0 {
                            return Task::none();
                        }
                        match region {
                            // Resolved → preselect and advance to the target step.
                            Some(r) => {
                                self.flash.device_region = Some(r);
                                self.flash.step = 1;
                            }
                            // SaleArea neither CN nor null → can't decide;
                            // stay on the manual region cards + note it.
                            None => {
                                let msg = self.t("flash_region_auto_unknown").to_string();
                                return Task::done(Message::ToastShow(msg));
                            }
                        }
                    }
                    // Network/upstream failure → manual region cards + toast.
                    Err(e) => {
                        if self.flash.step != 0 {
                            return Task::none();
                        }
                        return Task::done(Message::ToastShow(e));
                    }
                }
                Task::none()
            }
            FlashMsg::FlashTarget(t) => {
                // TB322FC: cross-region (OtherRegion) flashes are blocked
                // because the only valid region is PRC. Drop the message
                // even if a stale dispatch slips past the disabled card.
                if self.is_tb322fc() && t == FlashTarget::OtherRegion {
                    return Task::none();
                }
                self.flash.target = Some(t);
                Task::none()
            }
            FlashMsg::FlashDataMode(m) => {
                self.flash.data_mode = Some(m);
                Task::none()
            }
            FlashMsg::FlashNext => {
                // Data step → build WorkflowConfig; wipe opens country popup.
                if self.flash.step == 2 {
                    self.wf_config = WorkflowConfig {
                        modify_region: self.flash.target == Some(FlashTarget::OtherRegion),
                        device_region: self.flash.device_region,
                        modify_rollback: if self.flash.target == Some(FlashTarget::OtherRegion) {
                            RollbackSetting::On
                        } else {
                            RollbackSetting::Auto
                        },
                        wipe: self.flash.data_mode == Some(DataMode::Wipe),
                        country_action: CountryAction::Unset,
                    };
                    // Rebuilding the config invalidates any prior confirm-step
                    // override baseline; a fresh one is captured on entry below.
                    self.confirm_baseline = None;
                    if self.wf_config.wipe {
                        self.flash.next();
                        self.country_popup_open = true;
                        return Task::none();
                    }
                }
                if self.flash.step == 4 {
                    self.flash.next();
                    return self.update(Message::Flash(FlashMsg::FlashExecStart));
                }
                self.flash.next();
                // Snapshot the baseline only on the FIRST entry to confirm
                // after a rebuild (it is `None` then). Re-capturing on every
                // entry would fold a prior override into the baseline, so a
                // Back→Next round trip would hide a change that Start still
                // applies. The step-2 rebuild and exec/reset clear it again.
                if self.flash.step == 4 && self.confirm_baseline.is_none() {
                    self.confirm_baseline = Some(self.wf_config.clone());
                }
                Task::none()
            }
            FlashMsg::FlashBack => {
                if self.flash.step == 4 {
                    // Leaving confirm only closes any open editor. The baseline
                    // and picked overrides persist, so a Back→Next bounce to the
                    // folder step keeps power-user changes visible and applied.
                    // Going deeper to the data step rebuilds `wf_config` (and
                    // re-opens the country popup on wipe), which resets both.
                    self.confirm_edit_field = None;
                }
                self.flash.back();
                Task::none()
            }
            FlashMsg::FlashSelectFolder => {
                self.picker_target = PickerTarget::FlashFolder;
                pickers::pick_folder_for(
                    pickers::PickerKind::QfilFirmwareFolder,
                    &self.recent_paths,
                    Message::FolderSelected,
                )
            }
            FlashMsg::FlashSelectLoader => {
                // Always open the picker (don't auto-reuse the Settings default
                // via `pick_loader_with_default`) so the Change button can pick
                // a different loader — the default was already applied when the
                // loader-less folder was selected.
                pickers::pick_file_for(loader_file_spec(), &self.recent_paths, |v| {
                    Message::Flash(FlashMsg::FlashLoaderChosen(v))
                })
            }
            FlashMsg::FlashLoaderChosen(path) => {
                if let Some(p) = path {
                    // Model-aware resolve: upgrades a `.melf` to a sibling Sahara
                    // manifest on TB323FU (and rejects a standalone `.melf`
                    // there), validates the extension, and records the recent.
                    match self.resolve_loader_input(&p) {
                        Ok(loader) => {
                            self.flash.loader_override = Some(loader);
                            self.flash.loader_error = None;
                        }
                        Err(msg) => self.flash.loader_error = Some(msg),
                    }
                }
                Task::none()
            }
            FlashMsg::FlashExecStart => {
                let phases = self.begin_phased_op(View::Flash, OperationPhaseKind::Flash);
                self.error_msg = None;
                let cfg = self.wf_config.clone();
                let conn = self.connection;
                let device_model = self.device_model.clone();
                let fw_folder = self.flash.firmware_folder.clone().unwrap_or_default();
                let loader_override = self.flash.loader_override.clone();
                let rollback_label = self.t(cfg.modify_rollback.label_key()).to_string();
                // Split the old single "Starting: modify_region=… rollback=…
                // wipe=…" line into three labelled, translated lines — the
                // raw variable dump read like debug output.
                let region_yn = self
                    .t(if cfg.modify_region {
                        "common_yes"
                    } else {
                        "common_no"
                    })
                    .to_string();
                let wipe_yn = self
                    .t(if cfg.wipe { "common_yes" } else { "common_no" })
                    .to_string();
                self.log_push(format!(
                    "[Flash] {}",
                    tr_args!("live_flash_region_convert", value = region_yn)
                ));
                self.log_push(format!(
                    "[Flash] {}",
                    tr_args!("live_flash_rollback_bypass", value = rollback_label)
                ));
                self.log_push(format!(
                    "[Flash] {}",
                    tr_args!("live_flash_data_wipe", value = wipe_yn)
                ));
                let rb_mode = cfg.modify_rollback.to_mode();
                // NOTE: the EDL-start ARB downgrade (On/Auto → Off when the
                // device can't be Fastboot/ADB-probed) is applied inside the
                // worker, AFTER the firmware's vendor_boot fingerprint is
                // known — so a TB323FU target (which reads its rollback index
                // by dumping partitions over EDL) is exempt and stays on Auto.
                let ll = self.live_labels();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            ltbox_core::runtime::run_heavy(move || {
                                flash_worker(
                                    cfg,
                                    conn,
                                    device_model,
                                    fw_folder,
                                    loader_override,
                                    rb_mode,
                                    ll,
                                    phases,
                                )
                            })
                            .and_then(|r| r)
                        })
                        .await
                        .unwrap_or_else(|_| Err(ltbox_core::i18n::tr("err_task_failed")))
                    },
                    |result| match result {
                        Ok(lines) => Message::Flash(FlashMsg::FlashExecDone(lines)),
                        Err(e) => Message::OperationError(e),
                    },
                )
            }
            FlashMsg::FlashExecDone(lines) => {
                // Extend *before* end_op so the END separator sits
                // below the backend's detail lines, not above them.
                self.flush_exec_done_log(lines);
                self.end_op();
                self.wf_config = WorkflowConfig::default();
                self.confirm_baseline = None;
                self.confirm_edit_field = None;
                Task::none()
            }
            // Confirm-step "hidden dropdown" editors. Country reuses the
            // existing country popup; everything else opens the shared editor.
            // Each setter writes straight to `wf_config` (the worker's only
            // input) — no cascade — so the change is an explicit power-user
            // override, surfaced by the accent highlight against the baseline.
            FlashMsg::FlashConfirmOpen(field) => {
                match field {
                    ConfirmField::Country => self.country_popup_open = true,
                    other => self.confirm_edit_field = Some(other),
                }
                Task::none()
            }
            FlashMsg::FlashConfirmClose => {
                self.confirm_edit_field = None;
                Task::none()
            }
            FlashMsg::FlashConfirmSetRegion(r) => {
                // TB322FC is PRC-only; the editor grays out ROW, but drop a
                // stale dispatch defensively like the region card handler does.
                if !(self.is_tb322fc() && r == DeviceRegion::Row) {
                    self.wf_config.device_region = Some(r);
                }
                self.confirm_edit_field = None;
                Task::none()
            }
            FlashMsg::FlashConfirmSetTarget(t) => {
                // Target ↔ region edit both map onto `modify_region`. TB322FC
                // can't cross regions, so block OtherRegion defensively.
                if !(self.is_tb322fc() && t == FlashTarget::OtherRegion) {
                    self.wf_config.modify_region = t == FlashTarget::OtherRegion;
                }
                self.confirm_edit_field = None;
                Task::none()
            }
            FlashMsg::FlashConfirmSetData(m) => {
                let wipe = m == DataMode::Wipe;
                self.wf_config.wipe = wipe;
                self.confirm_edit_field = None;
                // Wipe still demands an explicit country/skip decision before
                // leaving the data step. Keep-data normally means "do not
                // change", but a confirm-step country override is valid and
                // must survive toggling back to Keep.
                if wipe {
                    if matches!(self.wf_config.country_action, CountryAction::Unset) {
                        self.wf_config.country_action = CountryAction::Skip;
                    }
                } else if self.wf_config.country_action.is_skipped() {
                    self.wf_config.country_action = CountryAction::Unset;
                }
                Task::none()
            }
            FlashMsg::FlashConfirmSetRegionEdit(on) => {
                // Region edit drives the same `modify_region` cross-region flag
                // as the Target row, so apply the TB322FC PRC-only guard here
                // too — otherwise this path bypasses the disabled Target option.
                if !(self.is_tb322fc() && on) {
                    self.wf_config.modify_region = on;
                }
                self.confirm_edit_field = None;
                Task::none()
            }
            FlashMsg::FlashConfirmSetRollback(s) => {
                self.wf_config.modify_rollback = s;
                self.confirm_edit_field = None;
                Task::none()
            }
        }
    }
}
