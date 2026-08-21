//! Modal popup views (device info, OTA, ARB index, country, region, rescue region, log). Extracted from `main.rs`.

use crate::*;
use iced::widget::{self, Space, button, column, container, row, scrollable, text};
use iced::{Element, Length, Theme};
use theme::with_alpha;

impl App {
    /// Device-info popup: render the Lenovo PTSTPD `data` block as a
    /// 2-column key/value table. Branches on `DeviceInfoState` so the
    /// modal stays open through Loading / Error / Ready transitions
    /// without flashing in/out of existence.
    pub(crate) fn device_info_popup_view(&self) -> Element<'_, Message> {
        let Some((serial, state)) = self.device_info_popup.clone() else {
            return container(text("")).into();
        };
        let title = text(self.t("device_info_popup_title").to_string())
            .size(theme::text_size::WIZARD_STEP_TITLE);
        // Copy-icon button — only enabled once the upstream payload is
        // cached; clicking copies the unmodified `data` JSON to the
        // clipboard and surfaces a toast.
        let copy_payload: Option<String> = self
            .device_info_cache
            .get(&serial)
            .map(|i| i.data_pretty.clone());
        let copy_glyph = text("⧉").size(16);
        let copy_btn = if let Some(payload) = copy_payload {
            button(container(copy_glyph).padding([2, 6]))
                .on_press(Message::CopyToClipboard(payload))
                .padding(0)
                .style(|t: &Theme, status| {
                    let p = pal_of(t);
                    // `surface_container` base + M3 state layer on hover / press.
                    let bg = theme::mix_color(
                        p.surface_container,
                        p.on_surface,
                        theme::state_alpha(status),
                    );
                    button::Style {
                        background: Some(bg.into()),
                        text_color: p.on_surface,
                        border: iced::Border {
                            radius: 6.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                })
        } else {
            // Same shape, no on_press — keeps the header layout stable
            // during the loading / error states without leaving an
            // active click target.
            button(container(copy_glyph).padding([2, 6]))
                .padding(0)
                .style(|t: &Theme, _s| {
                    let p = pal_of(t);
                    button::Style {
                        background: Some(p.surface_container.into()),
                        text_color: p.on_surface_variant,
                        border: iced::Border {
                            radius: 6.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                })
        };
        let header = iced::widget::row![title, Space::new().width(Length::Fill), copy_btn]
            .align_y(iced::Alignment::Center);
        let serial_line = text(format!("{}: {serial}", self.t("device_info_popup_serial")))
            .size(12)
            .style(muted_style);

        let body: Element<'_, Message> = match &state {
            DeviceInfoState::Loading => self.popup_loading_view(),
            DeviceInfoState::Error(e) => {
                self.popup_error_view("device_info_popup_error", e, Message::DeviceInfoRetry)
            }
            DeviceInfoState::Ready => {
                let info = match self.device_info_cache.get(&serial) {
                    Some(i) => i,
                    None => {
                        return container(text("")).into();
                    }
                };
                let mut table = column![].spacing(0);
                for (i, (k, v)) in info.fields.iter().enumerate() {
                    let display_v = v.clone().unwrap_or_default();
                    let key_cell = text(k.clone()).size(12).style(muted_style).width(180);
                    let val_cell = text(display_v).size(12).width(Length::Fill);
                    let row_inner = iced::widget::row![key_cell, val_cell]
                        .spacing(12)
                        .padding([4, 10])
                        .align_y(iced::Alignment::Center);
                    let zebra = i % 2 == 1;
                    let tinted = container(row_inner).width(Length::Fill).style(
                        move |t: &Theme| -> container::Style {
                            let p = pal_of(t);
                            container::Style {
                                background: if zebra {
                                    Some(iced::Background::Color(p.surface_container_low))
                                } else {
                                    None
                                },
                                ..Default::default()
                            }
                        },
                    );
                    table = table.push(tinted);
                }
                scrollable(table)
                    .style(m3_scrollable_style)
                    .height(Length::Fixed(420.0))
                    .width(Length::Fill)
                    .into()
            }
        };

        let close_btn =
            m3_filled_button(self.t("btn_close").to_string()).on_press(Message::DeviceInfoClose);

        let content = column![
            header,
            serial_line,
            widget::rule::horizontal(1),
            body,
            iced::widget::row![Space::new().width(Length::Fill), close_btn]
                .align_y(iced::Alignment::Center),
        ]
        .spacing(12)
        .padding(20)
        .width(640);

        m3_dialog(content.into())
    }

    /// Lenovo OTA "querynewfirmware" popup. Opens when the user clicks
    /// the dashboard firmware version. Mirrors `device_info_popup_view`
    /// for header / progress / error / close-button shape, but renders
    /// the OTA payload as a stacked card (From / To / Size / MD5 /
    /// Changelog / Download) instead of a flat key-value table.
    pub(crate) fn ota_popup_view(&self) -> Element<'_, Message> {
        let Some((_serial, _firmware_id, state)) = self.ota_popup.clone() else {
            return container(text("")).into();
        };
        let title =
            text(self.t("ota_popup_title").to_string()).size(theme::text_size::WIZARD_STEP_TITLE);
        let header = iced::widget::row![title, Space::new().width(Length::Fill)]
            .align_y(iced::Alignment::Center);

        let body: Element<'_, Message> = match &state {
            OtaPopupState::Loading => self.popup_loading_view(),
            OtaPopupState::Error(e) => {
                self.popup_error_view("ota_popup_error", e, Message::OtaRetry)
            }
            OtaPopupState::NoUpdate => container(
                text(self.t("ota_popup_unavailable").to_string())
                    .size(14)
                    .style(muted_style)
                    .width(Length::Fill)
                    .center(),
            )
            .width(Length::Fill)
            .height(48)
            .center_x(Length::Fill)
            .center_y(48)
            .into(),
            OtaPopupState::Ready(update) => {
                // Changelog text lives in `self.ota_changelog_editor`,
                // seeded by the `OtaFetched` handler from `desc_cn`
                // (Chinese GUI locale, when populated) or `desc_en`.
                // Rendered here through `text_editor` so drag-select +
                // Ctrl+C work — a plain `text` widget is a static label
                // and won't surface a selection.
                let size_str = ltbox_core::lenovo_ota::format_size(update.size_bytes);

                let from_to_row = column![
                    text(format!("{}: {}", self.t("ota_popup_from"), update.from))
                        .size(12)
                        .style(muted_style),
                    text(format!("{}: {}", self.t("ota_popup_to"), update.to)).size(13),
                ]
                .spacing(4);

                let meta_row = iced::widget::row![
                    info_kv(self.t("ota_popup_size"), &size_str),
                    info_kv(self.t("ota_popup_md5"), &update.md5),
                ]
                .spacing(40);

                let changelog_editor: Element<'_, Message> =
                    iced::widget::text_editor(&self.ota_changelog_editor)
                        .on_action(Message::OtaChangelogAction)
                        .size(12)
                        .into();
                let changelog_block = column![
                    text(self.t("ota_popup_changelog").to_string())
                        .size(11)
                        .style(muted_style),
                    container(changelog_editor)
                        .padding([8, 10])
                        .width(Length::Fill)
                        .style(|t: &Theme| {
                            let p = pal_of(t);
                            container::Style {
                                background: Some(p.surface_container_low.into()),
                                border: iced::Border {
                                    color: p.outline_variant,
                                    width: 1.0,
                                    radius: theme::shape::SM.into(),
                                },
                                ..Default::default()
                            }
                        }),
                ]
                .spacing(4);

                scrollable(
                    column![
                        from_to_row,
                        widget::rule::horizontal(1),
                        meta_row,
                        widget::rule::horizontal(1),
                        changelog_block,
                    ]
                    .spacing(12)
                    .width(Length::Fill),
                )
                .style(m3_scrollable_style)
                .height(Length::Fixed(420.0))
                .width(Length::Fill)
                .into()
            }
        };

        // Bottom action row: Download (when Ready + url present) sits
        // left of Close so the scrollable body's right-edge gutter
        // can't overlap the action — both buttons live on the dialog
        // chrome below the scrollable, not inside it.
        let download_url: Option<String> = match &state {
            OtaPopupState::Ready(u) if !u.download_url.is_empty() => Some(u.download_url.clone()),
            _ => None,
        };
        let close_btn =
            m3_filled_button(self.t("btn_close").to_string()).on_press(Message::OtaClose);
        let mut action_row = iced::widget::row![Space::new().width(Length::Fill)]
            .spacing(8)
            .align_y(iced::Alignment::Center);
        if let Some(url) = download_url {
            let download_btn = m3_filled_button(self.t("ota_popup_download").to_string())
                .on_press(Message::OtaOpenDownload(url));
            action_row = action_row.push(download_btn);
        }
        action_row = action_row.push(close_btn);

        let content = column![header, widget::rule::horizontal(1), body, action_row,]
            .spacing(12)
            .padding(20)
            .width(640);

        m3_dialog(content.into())
    }

    /// QFIL-firmware popup: the official flash-tool package for a CN device
    /// (resolved via MTM → `getPadFlashingMachine`), or a Software Fix pointer
    /// for a global device. Branches on `QfilPopupState` like the OTA popup.
    pub(crate) fn qfil_popup_view(&self) -> Element<'_, Message> {
        let Some((_serial, state)) = self.qfil_popup.clone() else {
            return container(text("")).into();
        };
        let title =
            text(self.t("qfil_popup_title").to_string()).size(theme::text_size::WIZARD_STEP_TITLE);
        let header = row![title, Space::new().width(Length::Fill)].align_y(iced::Alignment::Center);

        let placeholder = |key: &str| -> Element<'_, Message> {
            container(
                text(self.t(key).to_string())
                    .size(14)
                    .style(muted_style)
                    .width(Length::Fill)
                    .center(),
            )
            .width(Length::Fill)
            .height(48)
            .center_x(Length::Fill)
            .center_y(48)
            .into()
        };

        let body: Element<'_, Message> = match &state {
            QfilPopupState::Loading => self.popup_loading_view(),
            QfilPopupState::Error(e) => {
                self.popup_error_view("qfil_popup_error", e, Message::QfilRetry)
            }
            QfilPopupState::NoPackage => placeholder("qfil_popup_no_package"),
            QfilPopupState::Global => self.qfil_global_message(),
            QfilPopupState::Ready(pkg) => {
                let updated = pkg
                    .upd_time
                    .map(|t| crate::format_unix_timestamp_utc(t as u64))
                    .unwrap_or_default();
                let mut rows = column![].spacing(10).width(Length::Fill);
                if !pkg.version.is_empty() {
                    rows = rows.push(info_kv(self.t("qfil_popup_version"), &pkg.version));
                }
                if !pkg.file_name.is_empty() {
                    rows = rows.push(info_kv(self.t("qfil_popup_file"), &pkg.file_name));
                }
                if !pkg.platform.is_empty() {
                    rows = rows.push(info_kv(self.t("qfil_popup_platform"), &pkg.platform));
                }
                if !updated.is_empty() {
                    rows = rows.push(info_kv(self.t("qfil_popup_updated"), &updated));
                }
                // Archive password (fixed constant) with a copy affordance —
                // it isn't discoverable and the user needs it to extract.
                let pw = ltbox_core::lenovo_qfil::package_password();
                let pw_row = row![
                    info_kv(self.t("qfil_popup_password"), &pw),
                    Space::new().width(Length::Fill),
                    m3_filled_button(self.t("qfil_popup_copy").to_string())
                        .on_press(Message::CopyToClipboard(pw.clone())),
                ]
                .align_y(iced::Alignment::Center);
                rows = rows.push(widget::rule::horizontal(1));
                rows = rows.push(pw_row);
                rows.into()
            }
        };

        let download_url: Option<String> = match &state {
            QfilPopupState::Ready(p) if !p.download_url.is_empty() => Some(p.download_url.clone()),
            _ => None,
        };
        let close_btn =
            m3_filled_button(self.t("btn_close").to_string()).on_press(Message::QfilClose);
        let mut action_row = row![Space::new().width(Length::Fill)]
            .spacing(8)
            .align_y(iced::Alignment::Center);
        if let Some(url) = download_url {
            action_row = action_row.push(
                m3_filled_button(self.t("qfil_popup_download").to_string())
                    .on_press(Message::OpenExternalUrl(url)),
            );
        }
        action_row = action_row.push(close_btn);

        let content = column![header, widget::rule::horizontal(1), body, action_row,]
            .spacing(12)
            .padding(20)
            .width(560);

        m3_dialog(content.into())
    }

    /// The global-device message with an inline "Software Fix" hyperlink. The
    /// term is untranslated (product name) in every locale, so we split the
    /// localized sentence on it and link that span.
    fn qfil_global_message(&self) -> Element<'_, Message> {
        const SOFTWARE_FIX_URL: &str = "https://pcsupport.lenovo.com/rescue-and-smart-assistant";
        const LINK_TERM: &str = "Software Fix";
        let msg = self.t("qfil_popup_global").to_string();
        let primary = self.pal().primary;
        let content: Element<'_, Message> = if let Some(idx) = msg.find(LINK_TERM) {
            let before = msg[..idx].to_string();
            let after = msg[idx + LINK_TERM.len()..].to_string();
            iced::widget::rich_text([
                iced::widget::span(before).size(13),
                iced::widget::span(LINK_TERM)
                    .size(13)
                    .color(primary)
                    .underline(true)
                    .link(SOFTWARE_FIX_URL.to_string()),
                iced::widget::span(after).size(13),
            ])
            .on_link_click(Message::OpenExternalUrl)
            .into()
        } else {
            text(msg).size(13).into()
        };
        container(content)
            .padding([12, 4])
            .width(Length::Fill)
            .into()
    }

    /// One `<partition> = <value>` row of the rollback-index popup.
    ///
    /// The value is a button rather than static text: pressing it steps
    /// the shared format cycle, so the same click target answers "what
    /// number is this really" and "what date is that". The copy button
    /// beside it copies exactly the string currently on screen.
    fn rollback_floor_row<'a>(&'a self, partition: &str, index: u64) -> Element<'a, Message> {
        let rendered = self.rollback_value_format.render(index);
        let value_btn = button(
            text(rendered.clone())
                .size(theme::text_size::BODY_LARGE)
                .font(theme::emphasis::medium())
                .wrapping(iced::widget::text::Wrapping::None),
        )
        .on_press(Message::RollbackDetailCycleFormat)
        .padding([6, 10])
        .style(|t: &Theme, status| {
            let p = pal_of(t);
            button::Style {
                background: theme::state_layer_bg(status, p.on_surface).map(Into::into),
                text_color: p.on_surface,
                border: iced::Border {
                    radius: theme::shape::SM.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        });

        let copy_btn = m3_icon_button(icon::action_copy(), 16.0, |t: &Theme, status| {
            let p = pal_of(t);
            button::Style {
                background: theme::state_layer_bg(status, p.on_surface).map(Into::into),
                text_color: p.on_surface_variant,
                border: iced::Border {
                    radius: theme::shape::FULL.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .on_press(Message::CopyToClipboard(rendered));

        let copy_btn = widget::tooltip(
            copy_btn,
            container(text(self.t("rollback_copy_tip").to_string()).size(11))
                .padding([6, 10])
                .style(|t: &Theme| theme::tooltip_style(t, theme::shape::SM)),
            widget::tooltip::Position::Top,
        )
        .gap(6);

        row![
            text(partition.to_string())
                .size(theme::text_size::BODY_MEDIUM)
                .style(muted_style)
                .width(Length::Fixed(150.0)),
            value_btn,
            Space::new().width(Length::Fill),
            copy_btn,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
    }

    /// Rollback-index breakdown for a device in bootloader mode.
    ///
    /// The Dashboard cell only answers "is rollback protection on"; this
    /// is where the two committed floors live, since a raw index is
    /// meaningless without knowing which partition it guards and what
    /// the number represents.
    pub(crate) fn rollback_detail_popup_view(&self) -> Element<'_, Message> {
        let Some(floors) = self.device_rollback_floors else {
            return container(text("")).into();
        };
        let slot = active_slot_suffix(Some(&self.device_slot));

        let title = text(self.t("rollback_popup_title").to_string())
            .size(theme::text_size::WIZARD_STEP_TITLE)
            .font(theme::emphasis::bold());
        let desc = text(self.t("rollback_popup_desc").to_string())
            .size(theme::text_size::BODY_MEDIUM)
            .style(muted_style)
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
            .width(Length::Fill);

        // Naming the active form turns the cycle from a hidden trick into
        // a legible control.
        let format_hint = row![
            text(self.t("rollback_format_label").to_string())
                .size(theme::text_size::LABEL_SMALL)
                .style(muted_style),
            text(self.t(self.rollback_value_format.label_key()).to_string())
                .size(theme::text_size::LABEL_SMALL)
                .font(theme::emphasis::medium())
                .style(accent_style),
            Space::new().width(Length::Fill),
            text(self.t("rollback_cycle_tip").to_string())
                .size(theme::text_size::LABEL_SMALL)
                .style(muted_style),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center);

        let rows = column![
            self.rollback_floor_row(&format!("boot{slot}"), floors.boot_index),
            self.rollback_floor_row(&format!("vbmeta_system{slot}"), floors.vbmeta_system_index),
        ]
        .spacing(4);

        let close_btn = m3_filled_button(self.t("btn_close").to_string())
            .on_press(Message::RollbackDetailClose);

        let content = column![
            title,
            desc,
            widget::rule::horizontal(1),
            format_hint,
            rows,
            row![Space::new().width(Length::Fill), close_btn].align_y(iced::Alignment::Center),
        ]
        .spacing(14)
        .padding(24)
        .width(480);

        m3_dialog(content.into())
    }

    /// PatchArb timestamp popup. Reads `adv_wizard.arb_index_buffer`
    /// for the in-flight typing and renders the UTC representation in
    /// real time once the buffer hits exactly 10 digits. OK is enabled
    /// only on a 10-digit buffer that parses to a `u64`.
    pub(crate) fn arb_index_popup_view(&self) -> Element<'_, Message> {
        let buf = self.adv_wizard.arb_index_buffer.clone();
        let valid = buf.len() == 10 && buf.parse::<u64>().is_ok();

        // UTC preview only when the buffer is exactly 10 digits, so
        // shrinking the value (e.g. backspacing while editing) makes
        // the preview disappear instead of jumping to a stale time.
        let utc_preview: Element<'_, Message> = if valid {
            let ts: u64 = buf.parse().unwrap_or(0);
            let formatted = format_unix_timestamp_utc(ts);
            text(formatted).size(13).style(success_style).into()
        } else {
            // Keep a fixed-height placeholder so the layout doesn't
            // jump when the preview appears / disappears.
            container(text("").size(13)).height(20).into()
        };

        let title = text(self.t("arb_index_popup_title").to_string())
            .size(theme::text_size::WIZARD_STEP_TITLE);
        let subtitle = text(self.t("arb_index_popup_subtitle").to_string())
            .size(12)
            .style(muted_style);

        let input = iced::widget::text_input(
            self.t("arb_index_popup_placeholder"),
            &self.adv_wizard.arb_index_buffer,
        )
        .on_input(|s| Message::Adv(AdvMsg::AdvWizArbIndexInput(s)))
        .on_submit(Message::Adv(AdvMsg::AdvWizArbIndexConfirm))
        .padding([8, 12])
        .size(14)
        .width(Length::Fill)
        .style(m3_text_input_style);

        let cancel_btn = m3_text_button(self.t("btn_cancel").to_string())
            .on_press(Message::Adv(AdvMsg::AdvWizArbIndexCancel));
        let ok_btn = {
            let btn = m3_filled_button(self.t("btn_ok").to_string());
            if valid {
                btn.on_press(Message::Adv(AdvMsg::AdvWizArbIndexConfirm))
            } else {
                btn
            }
        };

        let content = column![
            title,
            subtitle,
            utc_preview,
            input,
            iced::widget::row![Space::new().width(Length::Fill), cancel_btn, ok_btn]
                .spacing(8)
                .align_y(iced::Alignment::Center),
        ]
        .spacing(12)
        .padding(20)
        .width(420);

        m3_dialog(content.into())
    }

    /// Manual serial-number prompt for auto region detection. Shown by the
    /// Flash region-step Auto FAB when no usable polled serial is available
    /// (device not in ADB/fastboot, or a garbled read).
    pub(crate) fn flash_serial_prompt_view(&self) -> Element<'_, Message> {
        let Some(buf) = self.flash_serial_prompt.clone() else {
            return container(text("")).into();
        };
        let valid = !buf.trim().is_empty();
        let title = text(self.t("flash_serial_prompt_title").to_string())
            .size(theme::text_size::WIZARD_STEP_TITLE);
        let subtitle = text(self.t("flash_serial_prompt_subtitle").to_string())
            .size(12)
            .style(muted_style);
        let input = iced::widget::text_input(self.t("flash_serial_prompt_placeholder"), &buf)
            .on_input(|s| Message::Flash(FlashMsg::FlashSerialPromptInput(s)))
            .on_submit(Message::Flash(FlashMsg::FlashSerialPromptSubmit))
            .padding([8, 12])
            .size(14)
            .width(Length::Fill)
            .style(m3_text_input_style);
        let skip_btn = m3_text_button(self.t("flash_serial_prompt_skip").to_string())
            .on_press(Message::Flash(FlashMsg::FlashSerialPromptSkip));
        let ok_btn = {
            let btn = m3_filled_button(self.t("btn_ok").to_string());
            if valid {
                btn.on_press(Message::Flash(FlashMsg::FlashSerialPromptSubmit))
            } else {
                btn
            }
        };
        let content = column![
            title,
            subtitle,
            input,
            row![Space::new().width(Length::Fill), skip_btn, ok_btn]
                .spacing(8)
                .align_y(iced::Alignment::Center),
        ]
        .spacing(12)
        .padding(20)
        .width(420);
        m3_dialog(content.into())
    }

    pub(crate) fn country_popup_view(&self) -> Element<'_, Message> {
        let mut list = column![].spacing(2);
        let selected_code = self.country_popup_selected_code();
        // Flash wizard only — hide "Do not change" from the Advanced
        // PatchDevinfo flow because that action requires a concrete target
        // code to write into devinfo/persist.
        if !self.adv_needs_country {
            let no_change_selected = self.wf_config.country_action.target().is_none()
                && (!self.wf_config.wipe || self.wf_config.country_action.is_skipped());
            list = list.push(
                button(text(self.t("popup_country_do_not_change").to_string()).size(13))
                    .on_press(Message::SkipCountryPatch)
                    .padding([6, 14])
                    .width(Length::Fill)
                    .style(move |t: &Theme, status| {
                        let p = pal_of(t);
                        button::Style {
                            background: Some(if no_change_selected {
                                p.primary.into()
                            } else {
                                // M3 list-item state layer on hover / press.
                                let a = theme::state_alpha(status);
                                if a > 0.0 {
                                    with_alpha(p.on_surface, a).into()
                                } else {
                                    iced::Color::TRANSPARENT.into()
                                }
                            }),
                            text_color: if no_change_selected {
                                p.on_primary
                            } else {
                                p.on_surface
                            },
                            ..Default::default()
                        }
                    }),
            );
            list = list.push(widget::rule::horizontal(1));
        }
        // TB322FC PRC-only: only CN is selectable in the Flash wizard. Non-CN
        // rows render as disabled buttons so the constraint stays visible. The
        // Advanced "Change Country Code" op has no such restriction (any country,
        // any model), so the gate is lifted there. "Do not change" stays usable.
        let tb322fc = self.is_tb322fc() && !self.adv_needs_country;
        for entry in COUNTRY_CODES {
            let code = entry.code.to_string();
            let selected = selected_code == Some(entry.code);
            let label = format!("{} — {}", entry.code, entry.name);
            let disabled = tb322fc && !entry.code.eq_ignore_ascii_case("CN");
            let mut btn = button(text(label).size(13))
                .padding([6, 14])
                .width(Length::Fill)
                .style(move |t: &Theme, status| {
                    let p = pal_of(t);
                    if disabled {
                        return button::Style {
                            background: Some(iced::Color::TRANSPARENT.into()),
                            text_color: with_alpha(p.on_surface, 0.38),
                            ..Default::default()
                        };
                    }
                    button::Style {
                        background: Some(if selected {
                            p.primary.into()
                        } else {
                            // M3 list-item state layer on hover / press.
                            let a = theme::state_alpha(status);
                            if a > 0.0 {
                                with_alpha(p.on_surface, a).into()
                            } else {
                                iced::Color::TRANSPARENT.into()
                            }
                        }),
                        text_color: if selected { p.on_primary } else { p.on_surface },
                        ..Default::default()
                    }
                });
            if !disabled {
                btn = btn.on_press(Message::SelectCountry(code));
            }
            list = list.push(btn);
        }

        let popup_content: Element<'_, Message> = column![
            row![
                text(self.t("popup_select_country").to_string()).size(16),
                Space::new().width(Length::Fill),
                m3_text_button(self.t("btn_cancel").to_string())
                    .on_press(Message::DismissCountryPopup),
            ]
            .align_y(iced::Alignment::Center),
            widget::rule::horizontal(1),
            scrollable(list).style(m3_scrollable_style).height(300),
        ]
        .spacing(10)
        .padding(20)
        .width(400)
        .into();
        m3_dialog(popup_content)
    }

    /// PRC / ROW radio popup for the Advanced RegionConvert wizard.
    /// Smaller than the country popup (only two choices) so the
    /// content uses M3 radio rows in a fixed-width card.
    pub(crate) fn region_target_popup_view(&self) -> Element<'_, Message> {
        let selected = self.adv_wizard.region_target;
        let mut list = column![].spacing(2);
        for target in [DeviceRegion::Prc, DeviceRegion::Row] {
            let is_selected = selected == Some(target);
            let label = self.t(target.label_key()).to_string();
            list = list.push(
                button(text(label).size(13))
                    .on_press(Message::SelectRegionTarget(target))
                    .padding([6, 14])
                    .width(Length::Fill)
                    .style(move |t: &Theme, status| {
                        let p = pal_of(t);
                        let hover = matches!(status, button::Status::Hovered);
                        button::Style {
                            background: Some(if is_selected {
                                p.primary.into()
                            } else if hover {
                                with_alpha(p.primary, theme::state::HOVER).into()
                            } else {
                                iced::Color::TRANSPARENT.into()
                            }),
                            text_color: if is_selected {
                                p.on_primary
                            } else {
                                p.on_surface
                            },
                            ..Default::default()
                        }
                    }),
            );
        }

        let popup_content: Element<'_, Message> = column![
            row![
                text(self.t("popup_select_region_target").to_string()).size(16),
                Space::new().width(Length::Fill),
                m3_text_button(self.t("btn_cancel").to_string())
                    .on_press(Message::DismissRegionTargetPopup),
            ]
            .align_y(iced::Alignment::Center),
            widget::rule::horizontal(1),
            list,
        ]
        .spacing(10)
        .padding(20)
        .width(320)
        .into();
        m3_dialog(popup_content)
    }

    /// Flash-confirm "hidden dropdown" editor. A small radio popup (same
    /// shape as `region_target_popup_view`) listing the alternatives for
    /// whichever confirm row was clicked. Each pick writes straight to
    /// `wf_config`. `Country` is handled by the country popup, so it never
    /// reaches here.
    pub(crate) fn flash_confirm_edit_popup(&self, field: ConfirmField) -> Element<'_, Message> {
        // (label, selected, on_press, disabled)
        let cfg = &self.wf_config;
        let tb322 = self.is_tb322fc();
        let opts: Vec<(String, bool, Message, bool)> = match field {
            ConfirmField::Region => [DeviceRegion::Prc, DeviceRegion::Row]
                .into_iter()
                .map(|r| {
                    (
                        self.t(r.label_key()).to_string(),
                        cfg.device_region == Some(r),
                        Message::Flash(FlashMsg::FlashConfirmSetRegion(r)),
                        tb322 && r == DeviceRegion::Row,
                    )
                })
                .collect(),
            ConfirmField::Target => [FlashTarget::OtherRegion, FlashTarget::SameRegion]
                .into_iter()
                .map(|t| {
                    (
                        self.t(t.label_key()).to_string(),
                        cfg.modify_region == (t == FlashTarget::OtherRegion),
                        Message::Flash(FlashMsg::FlashConfirmSetTarget(t)),
                        tb322 && t == FlashTarget::OtherRegion,
                    )
                })
                .collect(),
            ConfirmField::Data => [DataMode::Keep, DataMode::Wipe]
                .into_iter()
                .map(|d| {
                    (
                        self.t(if d == DataMode::Wipe {
                            "flash_confirm_data_wipe"
                        } else {
                            "flash_confirm_data_keep"
                        })
                        .to_string(),
                        cfg.wipe == (d == DataMode::Wipe),
                        Message::Flash(FlashMsg::FlashConfirmSetData(d)),
                        false,
                    )
                })
                .collect(),
            ConfirmField::RegionEdit => [true, false]
                .into_iter()
                .map(|on| {
                    (
                        self.t(if on {
                            "flash_confirm_rb_on"
                        } else {
                            "flash_confirm_rb_off"
                        })
                        .to_string(),
                        cfg.modify_region == on,
                        Message::Flash(FlashMsg::FlashConfirmSetRegionEdit(on)),
                        // PRC-only TB322FC can't cross regions — disable "On"
                        // to match the Target editor's OtherRegion gate.
                        tb322 && on,
                    )
                })
                .collect(),
            ConfirmField::Rollback => [
                RollbackSetting::On,
                RollbackSetting::Auto,
                RollbackSetting::Off,
            ]
            .into_iter()
            .map(|s| {
                (
                    self.t(match s {
                        RollbackSetting::On => "flash_confirm_rb_on",
                        RollbackSetting::Auto => "flash_confirm_rb_auto",
                        RollbackSetting::Off => "flash_confirm_rb_off",
                    })
                    .to_string(),
                    cfg.modify_rollback == s,
                    Message::Flash(FlashMsg::FlashConfirmSetRollback(s)),
                    false,
                )
            })
            .collect(),
            // Country is routed to the dedicated country popup, never here.
            ConfirmField::Country => Vec::new(),
        };

        let mut list = column![].spacing(2);
        for (label, is_selected, on_press, disabled) in opts {
            let mut btn = button(text(label).size(13))
                .padding([6, 14])
                .width(Length::Fill)
                .style(move |t: &Theme, status| {
                    let p = pal_of(t);
                    if disabled {
                        return button::Style {
                            background: Some(iced::Color::TRANSPARENT.into()),
                            text_color: with_alpha(p.on_surface, 0.38),
                            ..Default::default()
                        };
                    }
                    let hover = matches!(status, button::Status::Hovered);
                    button::Style {
                        background: Some(if is_selected {
                            p.primary.into()
                        } else if hover {
                            with_alpha(p.primary, theme::state::HOVER).into()
                        } else {
                            iced::Color::TRANSPARENT.into()
                        }),
                        text_color: if is_selected {
                            p.on_primary
                        } else {
                            p.on_surface
                        },
                        ..Default::default()
                    }
                });
            if !disabled {
                btn = btn.on_press(on_press);
            }
            list = list.push(btn);
        }

        let popup_content: Element<'_, Message> = column![
            row![
                text(self.t("flash_confirm_edit_title").to_string()).size(16),
                Space::new().width(Length::Fill),
                m3_text_button(self.t("btn_cancel").to_string())
                    .on_press(Message::Flash(FlashMsg::FlashConfirmClose)),
            ]
            .align_y(iced::Alignment::Center),
            widget::rule::horizontal(1),
            list,
        ]
        .spacing(10)
        .padding(20)
        .width(320)
        .into();
        m3_dialog(popup_content)
    }

    pub(crate) fn rescue_region_popup_view(&self) -> Element<'_, Message> {
        let mk_option = |region: RescueRegion, desc_key: &'static str| {
            let label = self.t(region.label_key()).to_string();
            let desc = self.t(desc_key).to_string();
            let selected = self.sysupdate.rescue_region == Some(region);
            button(
                column![
                    text(label).size(15).style(on_surface_style),
                    text(desc).size(12).style(muted_style),
                ]
                .spacing(4),
            )
            .on_press(Message::Sys(SysMsg::SysRescueRegion(region)))
            .padding([10, 16])
            .width(Length::Fill)
            .style(move |t: &Theme, status| {
                let p = pal_of(t);
                let hover = matches!(status, button::Status::Hovered);
                let bg = if selected {
                    p.primary_container.into()
                } else if hover {
                    with_alpha(p.primary, theme::state::HOVER).into()
                } else {
                    iced::Color::TRANSPARENT.into()
                };
                button::Style {
                    background: Some(bg),
                    text_color: p.on_surface,
                    border: iced::Border {
                        color: if selected {
                            p.primary
                        } else {
                            p.outline_variant
                        },
                        width: 1.0,
                        radius: theme::shape::SM.into(),
                    },
                    ..Default::default()
                }
            })
        };
        let popup_content: Element<'_, Message> = column![
            row![
                text(self.t("rescue_region_popup_title").to_string()).size(16),
                Space::new().width(Length::Fill),
                m3_text_button(self.t("btn_cancel").to_string())
                    .on_press(Message::Sys(SysMsg::SysRescueRegionPopupDismiss)),
            ]
            .align_y(iced::Alignment::Center),
            widget::rule::horizontal(1),
            text(self.t("rescue_region_popup_subtitle").to_string())
                .size(12)
                .style(muted_style),
            mk_option(RescueRegion::Prc, "rescue_region_prc_desc"),
            mk_option(RescueRegion::Row, "rescue_region_row_desc"),
        ]
        .spacing(10)
        .padding(20)
        .width(420)
        .into();
        m3_dialog(popup_content)
    }

    /// Full-viewport log popup. Replaces the wizard body while open;
    /// dismissed via Close.
    pub(crate) fn log_popup_view(&self) -> Element<'_, Message> {
        let editor = iced::widget::text_editor(&self.log_editor)
            .on_action(Message::LogEditorAction)
            .size(11)
            .height(Length::Fill)
            .padding(iced::Padding {
                top: 0.0,
                right: 16.0,
                bottom: 10.0,
                left: 16.0,
            })
            .style(m3_log_text_editor_style);
        let body = column![
            row![
                text(self.t("log_popup_title").to_string()).size(theme::text_size::TITLE_LARGE),
                Space::new().width(Length::Fill),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            widget::rule::horizontal(1),
            m3_log_text_field(self.t("dash_log").to_string(), editor.into()),
        ]
        .spacing(12)
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill);
        let utility_actions = row![
            wizard_utility_action(
                icon::fab_save_log(),
                self.t("btn_save_log").to_string(),
                Some(Message::SaveLog),
            ),
            wizard_utility_action(
                icon::fab_cancel(),
                self.t("btn_close").to_string(),
                Some(Message::ToggleLogPopup(false)),
            ),
        ]
        .spacing(0)
        .align_y(iced::Alignment::Center)
        .height(Length::Fill);
        let actions = wizard_utility_toolbar(utility_actions);

        column![
            container(body).width(Length::Fill).height(Length::Fill),
            wizard_fab_footer(row![].height(Length::Fill), actions),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
