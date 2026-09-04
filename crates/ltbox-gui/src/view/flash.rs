//! Flash wizard view + steps (region, target, data, folder, confirm, exec). Extracted from `main.rs`.

use crate::*;
use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Theme};
use ltbox_core::tr_args;

impl App {
    pub(crate) fn view_flash_wizard(&self) -> Element<'_, Message> {
        if self.log_popup_open && self.flash.is_in_exec() {
            return self.log_popup_view();
        }
        let step_labels: Vec<&str> = self
            .flash
            .visible_steps()
            .iter()
            .map(|step| self.t(step.label_key()))
            .collect();
        let step_bar = wizard_step_bar(&step_labels, self.flash.step);
        let current_step = self.flash.current_step();
        let body = match current_step {
            FlashStep::Region => self.flash_region_step(),
            FlashStep::Target => self.flash_target_step(),
            FlashStep::Data => self.flash_data_step(),
            FlashStep::Folder => self.flash_folder_step(),
            FlashStep::Bootloader => self.flash_bootloader_step(),
            FlashStep::Confirm => self.flash_confirm_step(),
            FlashStep::Flash => self.flash_exec_step(),
        };
        let nav = if current_step != FlashStep::Flash {
            let is_start = current_step == FlashStep::Confirm;
            let label_owned = if is_start {
                self.t("btn_start").to_string()
            } else {
                self.t("btn_next").to_string()
            };
            let can = self.flash.can_next()
                && !(self.busy && is_start)
                && (!is_start || self.device_reachable());
            wizard_nav_generic(
                self.flash.step > 0,
                &label_owned,
                can,
                self.t("btn_back"),
                Message::Flash(FlashMsg::FlashBack),
                Message::Flash(FlashMsg::FlashNext),
            )
        } else {
            empty_wizard_nav()
        };
        let mut layout = column![].width(Length::Fill).height(Length::Fill);
        if let Some(header) = self.flash_action_bar() {
            layout = layout.push(header);
        }
        layout
            .push(step_bar)
            .push(body)
            .push(nav)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn flash_action_bar(&self) -> Option<Element<'_, Message>> {
        let (title_key, subtitle_key) = match self.flash.current_step() {
            FlashStep::Region => ("flash_region_title", Some("flash_region_subtitle")),
            FlashStep::Target => ("flash_target_title", Some("flash_target_subtitle")),
            FlashStep::Data => ("flash_data_title", Some("flash_data_subtitle")),
            FlashStep::Folder => ("flash_folder_title", Some("flash_folder_subtitle")),
            FlashStep::Bootloader => ("flash_bootloader_title", Some("flash_bootloader_subtitle")),
            FlashStep::Confirm => ("flash_confirm_title", Some("flash_confirm_subtitle")),
            FlashStep::Flash => return Some(self.exec_action_bar()),
        };
        Some(wizard_action_bar(
            self.t(title_key).to_string(),
            subtitle_key.map(|key| self.t(key).to_string()),
        ))
    }

    pub(crate) fn flash_region_step(&self) -> Element<'_, Message> {
        let d = self.density();
        let columns = 2;
        let side = self.wizard_square_side();
        let prc_icon = lucide_primary(icon::region_prc(), self.wizard_square_icon());
        // TB322FC is a PRC-only SKU. Render ROW as a disabled card with
        // a grayed icon so the constraint is visible — silent skip
        // would confuse users who expect both options.
        let tb322fc = self.is_tb322fc();
        let unsupported_tb322fc = tr_args!("model_unsupported", model = "TB322FC");
        let row_card: Element<'_, Message> = if tb322fc {
            icon_option_card_sub_square_disabled_sized(
                lucide_disabled(icon::region_row(), self.wizard_square_icon()),
                self.t("region_row"),
                &unsupported_tb322fc,
                side,
            )
        } else {
            icon_option_card_sub_square_sized(
                lucide_primary(icon::region_row(), self.wizard_square_icon()),
                self.t("region_row"),
                self.t("region_row_name"),
                self.flash.device_region == Some(DeviceRegion::Row),
                Message::Flash(FlashMsg::FlashRegion(DeviceRegion::Row)),
                side,
            )
        };
        // While the on-entry auto-detect runs, show a progress ring + label instead
        // of the cards — the user briefly sees this, then lands on the target
        // step. The manual PRC/ROW cards are the fallback (probe failed /
        // inconclusive / skipped from the serial prompt).
        if self.flash_region_pending.is_some() {
            let probing = column![
                material_circular_progress(MaterialProgressSize::Standard),
                text(self.t("flash_region_detecting").to_string())
                    .size(d.text(13.0))
                    .style(muted_style),
            ]
            .spacing(d.space(16.0))
            .align_x(iced::Alignment::Center);
            // Shrink-height content centered in the filled body slot — both
            // axes, so the indicator sits dead center rather than at the top.
            return container(probing)
                .padding(d.space(28.0))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into();
        }
        let col = column![
            row![
                icon_option_card_sub_square_sized(
                    prc_icon,
                    self.t("region_prc"),
                    self.t("region_prc_name"),
                    self.flash.device_region == Some(DeviceRegion::Prc),
                    Message::Flash(FlashMsg::FlashRegion(DeviceRegion::Prc)),
                    side,
                ),
                row_card,
            ]
            .spacing(d.space(12.0)),
        ]
        .spacing(d.space(14.0))
        .padding(d.space(28.0))
        .width(Length::Fill)
        .align_x(iced::Alignment::Center);
        centered_step(col, self.square_step_max_width(columns))
    }

    pub(crate) fn flash_target_step(&self) -> Element<'_, Message> {
        let d = self.density();
        let columns = 2;
        let side = self.wizard_square_side();
        let device = lucide_primary(icon::tile_device(), self.wizard_square_icon());
        // TB322FC ships only in PRC, so cross-region (OtherRegion) is
        // never a valid target. Disable the card with a grayed icon to
        // keep the constraint visible on the picker.
        let tb322fc = self.is_tb322fc();
        let unsupported_tb322fc = tr_args!("model_unsupported", model = "TB322FC");
        // Region-aware target descriptions spell out the hardware market and
        // the ROM being installed so users don't conflate the two (the most
        // common point of confusion in this wizard). device_region is chosen
        // in step 0, so it is Some here; the None arm is a defensive fallback.
        let (same_desc, other_desc) = match self.flash.device_region {
            Some(DeviceRegion::Prc) => ("flashtarget_same_desc_prc", "flashtarget_other_desc_prc"),
            Some(DeviceRegion::Row) => ("flashtarget_same_desc_row", "flashtarget_other_desc_row"),
            None => ("flashtarget_same_desc", "flashtarget_other_desc"),
        };
        let other_card: Element<'_, Message> = if tb322fc {
            icon_option_card_sub_square_disabled_sized(
                lucide_disabled(icon::tile_globe(), self.wizard_square_icon()),
                self.t(FlashTarget::OtherRegion.label_key()),
                &unsupported_tb322fc,
                side,
            )
        } else {
            icon_option_card_sub_square_sized(
                lucide_primary(icon::tile_globe(), self.wizard_square_icon()),
                self.t(FlashTarget::OtherRegion.label_key()),
                self.t(other_desc),
                self.flash.target == Some(FlashTarget::OtherRegion),
                Message::Flash(FlashMsg::FlashTarget(FlashTarget::OtherRegion)),
                side,
            )
        };
        let col = column![
            row![
                other_card,
                icon_option_card_sub_square_sized(
                    device,
                    self.t(FlashTarget::SameRegion.label_key()),
                    self.t(same_desc),
                    self.flash.target == Some(FlashTarget::SameRegion),
                    Message::Flash(FlashMsg::FlashTarget(FlashTarget::SameRegion)),
                    side,
                ),
            ]
            .spacing(d.space(12.0)),
        ]
        .spacing(d.space(14.0))
        .padding(d.space(28.0))
        .width(Length::Fill)
        .align_x(iced::Alignment::Center);
        centered_step(col, self.square_step_max_width(columns))
    }

    pub(crate) fn flash_data_step(&self) -> Element<'_, Message> {
        let d = self.density();
        let columns = 2;
        let side = self.wizard_square_side();
        let shield = lucide_primary(icon::tile_shield(), self.wizard_square_icon());
        // Erasing `metadata` + `userdata` is the one irreversible choice
        // on this step, so it carries the error role rather than looking
        // like the sibling it is not.
        let wipe = lucide_error(icon::tile_wipe(), self.wizard_square_icon());
        let col = column![
            row![
                icon_option_card_sub_square_sized(
                    shield,
                    self.t(DataMode::Keep.label_key()),
                    self.t("datamode_keep_desc"),
                    self.flash.data_mode == Some(DataMode::Keep),
                    Message::Flash(FlashMsg::FlashDataMode(DataMode::Keep)),
                    side,
                ),
                icon_option_card_sub_square_destructive_sized(
                    wipe,
                    self.t(DataMode::Wipe.label_key()),
                    self.t("datamode_wipe_desc"),
                    self.flash.data_mode == Some(DataMode::Wipe),
                    Message::Flash(FlashMsg::FlashDataMode(DataMode::Wipe)),
                    side,
                ),
            ]
            .spacing(d.space(12.0)),
        ]
        .spacing(d.space(14.0))
        .padding(d.space(28.0))
        .width(Length::Fill)
        .align_x(iced::Alignment::Center);
        centered_step(col, self.square_step_max_width(columns))
    }

    pub(crate) fn flash_folder_step(&self) -> Element<'_, Message> {
        let d = self.density();
        let selected = self.flash.firmware_folder.is_some();
        let status = if let Some(p) = &self.flash.firmware_folder {
            p.clone()
        } else {
            self.t("flash_folder_placeholder").to_string()
        };
        let btn = button(
            container(
                column![
                    text(self.t("btn_browse_folder").to_string())
                        .size(d.text(14.0))
                        .center(),
                    text(self.t("flash_folder_desc").to_string())
                        .size(d.text(11.0))
                        .style(muted_style)
                        .center(),
                ]
                .spacing(d.space(6.0))
                .width(Length::Fill)
                .align_x(iced::Alignment::Center),
            )
            .padding(d.padding(20.0, 24.0))
            .width(Length::Fixed(d.width(280.0)))
            .style(move |t: &Theme| sel_card_style(t, selected)),
        )
        .on_press(Message::Flash(FlashMsg::FlashSelectFolder))
        .padding(0)
        .style(move |t: &Theme, status| sel_card_btn_style(t, status, selected));
        let chips = self.recent_chips(
            self.recent_paths
                .recent(PickerTarget::FlashFolder.kind().storage_key()),
            |p| Message::RecentFolderPicked(PickerTarget::FlashFolder, p),
            "picker_recents",
            false,
        );
        let mut col = column![
            btn,
            text(status)
                .size(d.text(12.0))
                .width(Length::Fill)
                .style(move |t: &Theme| {
                    let p = pal_of(t);
                    iced::widget::text::Style {
                        color: Some(if selected { p.success } else { p.outline }),
                    }
                })
                .center()
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        ]
        .spacing(d.space(14.0))
        .padding(d.space(28.0))
        .width(Length::Fill)
        .align_x(iced::Alignment::Center);

        // The picked firmware folder ships no EDL loader — require a
        // separately-picked loader (or the configured default) before Next.
        if selected && self.flash.loader_required {
            let has = self.flash.loader_override.is_some();
            let notice = text(
                self.t(if has {
                    "flash_loader_provided"
                } else {
                    "flash_loader_missing"
                })
                .to_string(),
            )
            .size(d.text(12.0))
            .style(move |t: &Theme| iced::widget::text::Style {
                color: Some(if has {
                    pal_of(t).success
                } else {
                    pal_of(t).warning
                }),
            })
            .center()
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph);
            let browse = m3_text_button(
                self.t(if has {
                    "flash_loader_change"
                } else {
                    "flash_loader_browse"
                })
                .to_string(),
            )
            .on_press(Message::Flash(FlashMsg::FlashSelectLoader));
            let mut loader_col = column![notice]
                .spacing(d.space(6.0))
                .align_x(iced::Alignment::Center);
            if let Some(p) = &self.flash.loader_override {
                loader_col = loader_col.push(
                    text(p.clone())
                        .size(d.text(11.0))
                        .style(muted_style)
                        .center()
                        .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
                );
            }
            if let Some(err) = &self.flash.loader_error {
                loader_col = loader_col.push(
                    text(err.clone())
                        .size(d.text(11.0))
                        .style(|t: &Theme| iced::widget::text::Style {
                            color: Some(pal_of(t).error),
                        })
                        .center()
                        .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
                );
            }
            col = col.push(loader_col.push(browse));
        }

        col = col.push(chips);
        container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    pub(crate) fn flash_bootloader_step(&self) -> Element<'_, Message> {
        let d = self.density();
        let selected = self.flash.user_abl_path.is_some();
        let analyzing = self.flash.user_abl_analyzing;
        let mut browse = button(
            container(
                column![
                    text(self.t("flash_bootloader_select").to_string())
                        .size(d.text(14.0))
                        .center(),
                    text("abl.elf")
                        .size(d.text(11.0))
                        .style(muted_style)
                        .center(),
                ]
                .spacing(d.space(6.0))
                .width(Length::Fill)
                .align_x(iced::Alignment::Center),
            )
            .padding(d.padding(20.0, 24.0))
            .width(Length::Fixed(d.width(280.0)))
            .style(move |theme: &Theme| sel_card_style(theme, selected)),
        )
        .padding(0)
        .style(move |theme: &Theme, status| {
            if analyzing {
                let palette = pal_of(theme);
                button::Style {
                    background: Some(palette.surface_container.into()),
                    text_color: with_alpha(palette.on_surface, 0.38),
                    border: iced::Border {
                        radius: theme::shape::LG.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            } else {
                sel_card_btn_style(theme, status, selected)
            }
        });
        if !analyzing {
            browse = browse.on_press(Message::Flash(FlashMsg::FlashSelectBootloader));
        }

        let clear_slot: Element<'_, Message> = if selected {
            // Same control as the Root wizard's KPM remove button: the shared
            // lucide `minus` glyph in a neutral circular icon button.
            m3_icon_button(icon::kpm_remove(), d.image(18.0), |theme, status| {
                let palette = pal_of(theme);
                button::Style {
                    background: Some(
                        with_alpha(palette.on_surface, 0.10 + theme::state_alpha(status)).into(),
                    ),
                    text_color: palette.on_surface,
                    border: iced::Border {
                        radius: theme::shape::FULL.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .on_press(Message::Flash(FlashMsg::FlashClearBootloader))
            .into()
        } else {
            iced::widget::Space::new()
                .width(Length::Fixed(M3_ICON_BUTTON_SIZE))
                .height(Length::Fixed(M3_ICON_BUTTON_SIZE))
                .into()
        };
        let picker_row = row![
            iced::widget::Space::new()
                .width(Length::Fixed(M3_ICON_BUTTON_SIZE))
                .height(Length::Fixed(M3_ICON_BUTTON_SIZE)),
            browse,
            clear_slot,
        ]
        .spacing(d.space(8.0))
        .align_y(iced::Alignment::Center);

        let verdict_key = if !selected {
            "flash_bootloader_empty"
        } else if analyzing {
            "flash_bootloader_analyzing"
        } else {
            match self.flash.user_abl_key_class {
                Some(ltbox_patch::key_map::KeyClass::Testkey) => "flash_key_testkey",
                Some(ltbox_patch::key_map::KeyClass::Lenovo) => "flash_key_lenovo",
                Some(ltbox_patch::key_map::KeyClass::Unknown) | None => "flash_key_unknown",
            }
        };
        let valid = self.flash.user_abl_key_class == Some(ltbox_patch::key_map::KeyClass::Testkey)
            && !analyzing;
        let verdict = text(self.t(verdict_key).to_string())
            .size(d.text(13.0))
            .style(move |theme: &Theme| iced::widget::text::Style {
                color: Some(if valid {
                    pal_of(theme).success
                } else if selected && !analyzing {
                    pal_of(theme).error
                } else {
                    pal_of(theme).outline
                }),
            });

        let mut content = column![picker_row, verdict]
            .spacing(d.space(10.0))
            .align_x(iced::Alignment::Center);
        if let Some(path) = &self.flash.user_abl_path {
            content = content.push(
                text(path.clone())
                    .size(d.text(11.0))
                    .style(muted_style)
                    .center()
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
            );
        }

        container(content)
            .padding(d.space(28.0))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    pub(crate) fn flash_confirm_step(&self) -> Element<'_, Message> {
        let d = self.density();
        let dash = "—".to_string();
        // `wf_config` is the worker's only input, so the summary derives every
        // editable row from it (not the wizard cards). The values match the
        // card selections in the normal flow, so the rendered rows are
        // unchanged until a confirm-step override diverges from the baseline.
        let cfg = &self.wf_config;
        let base = self.confirm_baseline.as_ref();
        let caution = self.t("flash_confirm_override_warning").to_string();
        let open = |f: ConfirmField| Message::Flash(FlashMsg::FlashConfirmOpen(f));

        let region = cfg
            .device_region
            .map(|r| self.t(r.label_key()).to_string())
            .unwrap_or_else(|| dash.clone());
        let region_changed = base.is_some_and(|b| b.device_region != cfg.device_region);

        // Target ↔ Region-edit both reflect `modify_region`, so they always
        // agree and highlight together.
        let target_kind = if cfg.modify_region {
            FlashTarget::OtherRegion
        } else {
            FlashTarget::SameRegion
        };
        let target = self.t(target_kind.label_key()).to_string();
        let modify_changed = base.is_some_and(|b| b.modify_region != cfg.modify_region);

        let data = self
            .t(if cfg.wipe {
                "flash_confirm_data_wipe"
            } else {
                "flash_confirm_data_keep"
            })
            .to_string();
        let data_changed = base.is_some_and(|b| b.wipe != cfg.wipe);

        // Confirm rows use short value labels (Modify / Auto / Ignore)
        // instead of the verbose "… rollback index" strings shown in
        // logs — the review summary is tighter to read that way.
        let modify_region = self
            .t(if cfg.modify_region {
                "flash_confirm_rb_on"
            } else {
                "flash_confirm_rb_off"
            })
            .to_string();
        let rollback = self
            .t(match cfg.modify_rollback {
                RollbackSetting::On => "flash_confirm_rb_on",
                RollbackSetting::Auto => "flash_confirm_rb_auto",
                RollbackSetting::Manual => "flash_confirm_rb_manual",
                RollbackSetting::Off => "flash_confirm_rb_off",
            })
            .to_string();
        let rollback_changed = base.is_some_and(|b| b.modify_rollback != cfg.modify_rollback);

        let rollback_caution = if cfg.modify_rollback == RollbackSetting::Manual
            && self.manual_rollback_downgrade_warning().is_some()
        {
            self.t("flash_confirm_rb_manual_downgrade").to_string()
        } else {
            caution.clone()
        };

        // Destructive-op callout, hoisted above the summary so the hazard
        // reads before the device details. Amber `warning` colour — not an
        // error/failure. Wipe vs keep-data show different cautions.
        let warning_key = if cfg.wipe {
            "flash_confirm_warning_wipe"
        } else {
            "flash_confirm_warning"
        };
        let warning = container(
            text(self.t(warning_key).to_string())
                .size(d.text(12.0))
                .style(warning_style)
                .center()
                .width(Length::Fill)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        )
        .padding(d.padding(4.0, 8.0))
        .width(Length::Fill);

        let region_row = info_kv_center_editable(
            self.t("flash_confirm_region"),
            &region,
            region_changed,
            &caution,
            open(ConfirmField::Region),
        );
        let target_row = info_kv_center_editable(
            self.t("flash_confirm_target"),
            &target,
            modify_changed,
            &caution,
            open(ConfirmField::Target),
        );
        let data_row = info_kv_center_editable(
            self.t("flash_confirm_data"),
            &data,
            data_changed,
            &caution,
            open(ConfirmField::Data),
        );
        let region_edit_row = info_kv_center_editable(
            self.t("flash_confirm_region_edit"),
            &modify_region,
            modify_changed,
            &caution,
            open(ConfirmField::RegionEdit),
        );
        let rollback_row = info_kv_center_editable(
            self.t("flash_confirm_rollback"),
            &rollback,
            rollback_changed,
            &rollback_caution,
            // Always the setting list, Manual included: the row is how the user
            // gets back to On/Auto/Off, and picking Manual there opens the
            // editor anyway.
            open(ConfirmField::Rollback),
        );

        let country_changed = base.is_some_and(|b| b.country_action != cfg.country_action);
        let country_label = if let Some(cc) = cfg.country_action.target() {
            COUNTRY_CODES
                .iter()
                .find(|e| e.code == cc)
                .map(|e| format!("{} — {}", e.code, e.name))
                .unwrap_or_else(|| cc.to_string())
        } else {
            self.t("flash_confirm_country_skip").to_string()
        };
        let country_row = info_kv_center_editable(
            self.t("flash_confirm_country"),
            &country_label,
            country_changed,
            &caution,
            open(ConfirmField::Country),
        );
        let folder_owned = self
            .flash
            .firmware_folder
            .clone()
            .unwrap_or_else(|| dash.clone());
        let folder_row = info_kv_center_action(
            self.t("flash_confirm_folder"),
            &folder_owned,
            Message::Flash(FlashMsg::FlashSelectFolder),
        );

        self.confirm_step_frame(
            vec![warning.into()],
            vec![
                region_row,
                target_row,
                data_row,
                region_edit_row,
                rollback_row,
                country_row,
            ],
            vec![folder_row],
        )
    }

    pub(crate) fn flash_exec_step(&self) -> Element<'_, Message> {
        self.exec_step_view()
    }
}
