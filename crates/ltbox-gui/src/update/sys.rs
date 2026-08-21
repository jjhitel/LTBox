//! System-update wizard handler (OTA disable/enable, Rescue). Extracted from `main.rs`.

use crate::*;
use iced::Task;
use ltbox_core::tr_args;

impl App {
    pub(crate) fn update_sys(&mut self, msg: SysMsg) -> Task<Message> {
        match msg {
            SysMsg::SysAction(a) => {
                // TB323FU can't run Boot Recovery (vendor_boot/vbmeta LUN
                // mismatch). The card is disabled, but drop a stale dispatch
                // here too so the action can never latch.
                if a == SysUpdateAction::Rescue && self.is_tb323fu() {
                    return Task::none();
                }
                // Switching action resets Rescue-specific state so a stale
                // folder/region can't leak into a fresh flow.
                self.sysupdate.action = Some(a);
                self.sysupdate.rescue_folder = None;
                self.sysupdate.rescue_region = None;
                self.sysupdate.rescue_region_popup_open = false;
                self.sysupdate.rescue_region_confirmed = false;
                Task::none()
            }
            SysMsg::SysNext => {
                // Rescue flow: Action(0) → Folder(1) → Confirm(2) → Exec(3).
                // Gate: popping the region popup between Folder and Confirm.
                if self.sysupdate.is_rescue() {
                    if self.sysupdate.step == 1 && !self.sysupdate.rescue_region_confirmed {
                        // Pre-select rescue region from the polled
                        // device's PTSTPD `SaleArea` when we have it,
                        // mirroring how Flash seeds `device_region`
                        // from `inferred_flash_region`. The popup
                        // still opens — users get to see/confirm —
                        // but the matching radio is checked on entry
                        // so a CN device doesn't force a blind pick.
                        if self.sysupdate.rescue_region.is_none()
                            && let Some(inferred) = self.inferred_flash_region()
                        {
                            self.sysupdate.rescue_region = Some(match inferred {
                                DeviceRegion::Prc => RescueRegion::Prc,
                                DeviceRegion::Row => RescueRegion::Row,
                            });
                        }
                        self.sysupdate.rescue_region_popup_open = true;
                        return Task::none();
                    }
                    if self.sysupdate.step == 2 {
                        self.sysupdate.next();
                        return self.update(Message::Sys(SysMsg::SysExecStart));
                    }
                    self.sysupdate.next();
                } else {
                    // Disable/Enable: Action(0) → Confirm(1) → Exec(2).
                    if self.sysupdate.step == 1 {
                        self.sysupdate.next();
                        return self.update(Message::Sys(SysMsg::SysExecStart));
                    }
                    self.sysupdate.next();
                }
                Task::none()
            }
            SysMsg::SysBack => {
                self.sysupdate.back();
                Task::none()
            }
            SysMsg::SysRescueSelectFolder => {
                // Rescue dump+flash resolves vendor_boot / vbmeta against
                // the device's on-storage GPT (LUN 0), so the wizard only
                // needs the EDL loader binary — `rawprogram*.xml` was
                // never read in this path. File picker with the standard
                // loader extension filter, recents shared with the rest
                // of the loader pickers via the File bucket.
                self.pick_loader_with_default(|__v| {
                    Message::Sys(SysMsg::SysRescueFolderChosen(__v))
                })
            }
            SysMsg::SysRescueFolderChosen(path) => {
                if let Some(p) = path {
                    self.remember_recent(pickers::PickerKind::File, &p);
                    self.sysupdate.rescue_folder = Some(p);
                    // Force re-pick of region when loader changes — a stale
                    // region from a prior firmware could target the wrong
                    // hardware.
                    self.sysupdate.rescue_region = None;
                    self.sysupdate.rescue_region_confirmed = false;
                }
                Task::none()
            }
            SysMsg::SysRescueRegion(r) => {
                self.sysupdate.rescue_region = Some(r);
                self.sysupdate.rescue_region_confirmed = true;
                self.sysupdate.rescue_region_popup_open = false;
                // Auto-advance out of Folder step into Confirm — picking
                // the region is the implicit "Next" of the popup.
                if self.sysupdate.step == 1 {
                    self.sysupdate.next();
                }
                Task::none()
            }
            SysMsg::SysRescueRegionPopupDismiss => {
                self.sysupdate.rescue_region_popup_open = false;
                Task::none()
            }
            SysMsg::SysExecStart => {
                let Some(action) = self.sysupdate.action else {
                    return Task::none();
                };
                // Final guard: never start Boot Recovery on TB323FU even if a
                // stale Rescue selection slipped past the disabled card.
                if action == SysUpdateAction::Rescue && self.is_tb323fu() {
                    self.error_msg = Some(tr_args!("model_unsupported", model = "TB323FU"));
                    return Task::none();
                }
                // Rescue captures folder + region into the blocking task.
                // Cloning here keeps `self` untouched while the async move
                // takes ownership.
                let rescue_folder = self.sysupdate.rescue_folder.clone();
                let rescue_region = self.sysupdate.rescue_region;
                if action == SysUpdateAction::Rescue
                    && self.validate_loader_path(&rescue_folder).is_err()
                {
                    return Task::none();
                }
                // Capture model for AVB fingerprint validation — prevents
                // flashing firmware built for other models.
                let device_model = self.device_model.clone();
                let conn = self.connection;
                let phase_kind = match action {
                    SysUpdateAction::Disable => OperationPhaseKind::SysUpdateDisable,
                    SysUpdateAction::Enable => OperationPhaseKind::SysUpdateEnable,
                    SysUpdateAction::Rescue => OperationPhaseKind::BootRecovery,
                };
                let phases = self.begin_phased_op(View::SystemUpdate, phase_kind);
                self.error_msg = None;
                self.log_push(format!(
                    "[SysUpdate] {}",
                    tr_args!(
                        "log_sysupdate_starting",
                        action = self.t(action.label_key())
                    )
                ));
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            sysupdate_worker(
                                action,
                                rescue_folder,
                                rescue_region,
                                device_model,
                                conn,
                                phases,
                            )
                        })
                        .await
                        .unwrap_or_else(|_| Err(ltbox_core::i18n::tr("err_task_failed")))
                    },
                    |result| match result {
                        Ok(lines) => Message::Sys(SysMsg::SysExecDone(lines)),
                        Err(e) => Message::OperationError(e),
                    },
                )
            }
            SysMsg::SysExecDone(lines) => {
                self.flush_exec_done_log(lines);
                self.end_op();
                Task::none()
            }
        }
    }
}
