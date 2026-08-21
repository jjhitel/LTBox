//! Root wizard view + steps + superkey/run-id/kernel-version popups. Extracted from `main.rs`.

use crate::*;
use iced::widget::{Space, button, column, container, row, text};
use iced::{Element, Length, Theme};
use ltbox_core::tr_args;
use theme::with_alpha;

impl App {
    pub(crate) fn view_root_wizard(&self) -> Element<'_, Message> {
        // Superkey / Run-ID / Kernel-version popups all render as
        // top-level M3 dialog overlays via `view()`'s layer stack —
        // do NOT early-return for any of them here, otherwise the
        // KPM step underneath would unmount and Cancel couldn't
        // restore the curated list.
        if self.log_popup_open && self.root.is_in_exec() {
            return self.log_popup_view();
        }
        let steps = self.root.active_steps();
        let step_labels: Vec<&str> = steps.iter().map(|k| self.t(k)).collect();
        let step_bar = wizard_step_bar(&step_labels, self.root.display_step());
        let body = match self.root.step {
            0 => self.root_family_step(),
            1 => {
                if self.root.is_skroot() {
                    self.root_skroot_flavor_step()
                } else {
                    self.root_mode_step()
                }
            }
            2 => {
                if self.root.is_gki() {
                    self.root_file_step(self.t("root_kernel_subtitle"))
                } else {
                    self.root_provider_step()
                }
            }
            3 => {
                if self.root.is_forks() {
                    self.root_file_step(self.t("root_apk_subtitle"))
                } else {
                    self.root_version_step()
                }
            }
            4 => self.root_nightly_source_step(),
            5 => self.root_folder_step(),
            6 => self.root_confirm_step(),
            8 => self.root_kpm_step(),
            _ => self.root_flash_step(),
        };
        // Step 7 is in-progress — no nav. Step 8 (APatch KPM) needs
        // the normal Back/Next bar, so exclude only 7 explicitly.
        let nav = if self.root.step != 7 {
            let is_start = self.root.step == 6;
            let label_owned = if is_start {
                self.t("btn_start").to_string()
            } else {
                self.t("btn_next").to_string()
            };
            let can = self.root.can_next()
                && !(self.busy && is_start)
                && (!is_start || self.device_reachable());
            wizard_nav(self.root.step > 0, &label_owned, can, self.t("btn_back"))
        } else {
            empty_wizard_nav()
        };
        let mut layout = column![].width(Length::Fill).height(Length::Fill);
        if let Some(header) = self.root_action_bar() {
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

    fn root_action_bar(&self) -> Option<Element<'_, Message>> {
        let header = match self.root.step {
            0 => (
                self.t("root_type_title").to_string(),
                self.t("root_type_subtitle").to_string(),
            ),
            1 if self.root.is_skroot() => (
                self.t("root_skroot_flavor_title").to_string(),
                self.t("root_skroot_flavor_subtitle").to_string(),
            ),
            1 => {
                let family = self
                    .root
                    .family
                    .map(|f| self.t(f.label_key()))
                    .unwrap_or("?");
                (
                    tr_args!("root_mode_title_tmpl", family = family),
                    self.t("root_mode_subtitle").to_string(),
                )
            }
            2 if self.root.is_gki() => (
                self.t("root_kernel_title").to_string(),
                self.t("root_kernel_subtitle").to_string(),
            ),
            2 => {
                let family = self.root.family.unwrap_or(Family::KernelSU);
                (
                    tr_args!(
                        "root_provider_title_tmpl",
                        family = self.t(family.label_key())
                    ),
                    self.t("root_provider_subtitle").to_string(),
                )
            }
            3 if self.root.is_forks() => (
                self.t("root_apk_title").to_string(),
                self.t("root_apk_subtitle").to_string(),
            ),
            3 => (
                self.t("root_version_title").to_string(),
                self.t("root_version_subtitle").to_string(),
            ),
            4 => (
                self.t("root_source_title").to_string(),
                self.t("root_source_subtitle").to_string(),
            ),
            5 => (
                self.t("edl_loader_title").to_string(),
                self.t("edl_loader_subtitle").to_string(),
            ),
            6 => (
                self.t("root_confirm_title").to_string(),
                self.t("root_confirm_subtitle").to_string(),
            ),
            7 => return Some(self.exec_action_bar()),
            8 => (
                self.t("root_kpm_title").to_string(),
                self.t("root_kpm_subtitle").to_string(),
            ),
            _ => return None,
        };
        Some(wizard_action_bar(header.0, Some(header.1)))
    }

    pub(crate) fn root_kpm_step(&self) -> Element<'_, Message> {
        // No recents here — the KPM list already competes for vertical space.
        let kpm_selected = !self.root.kpm_paths.is_empty();
        let pick_btn = button(
            container(
                column![
                    text(self.t("btn_browse_kpm").to_string()).size(14).center(),
                    text(self.t("root_kpm_desc").to_string())
                        .size(11)
                        .style(muted_style)
                        .center(),
                ]
                .spacing(6)
                .width(Length::Fill)
                .align_x(iced::Alignment::Center),
            )
            .padding([20, 24])
            .width(KPM_COLUMN_WIDTH)
            .style(move |t: &Theme| sel_card_style(t, kpm_selected)),
        )
        .on_press(Message::Root(RootMsg::RootSelectKpm))
        .padding(0)
        .style(move |t: &Theme, status| sel_card_btn_style(t, status, kpm_selected));

        // Same width as the browse card, not `Fill`. A fill-width child
        // ignores the parent column's centering and spans the whole
        // content area, which packed every row against the far left edge
        // while the card it belongs to sat centred — the two read as
        // unrelated. Matching widths makes them one column.
        let mut list = column![].spacing(4).width(KPM_COLUMN_WIDTH);
        for path in &self.root.kpm_paths {
            let name = std::path::Path::new(path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());
            let p_copy = path.clone();
            let remove = m3_icon_button(icon::kpm_remove(), 16.0, |t: &Theme, status| {
                let p = pal_of(t);
                button::Style {
                    background: Some(
                        with_alpha(p.on_surface, 0.10 + theme::state_alpha(status)).into(),
                    ),
                    text_color: p.on_surface,
                    border: iced::Border {
                        radius: theme::shape::FULL.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .on_press(Message::Root(RootMsg::RootKpmRemove(p_copy)));
            list = list.push(
                row![
                    remove,
                    // Module filenames can be long and have no spaces, so
                    // break at glyph boundaries rather than overflowing
                    // the column the list now shares with the card.
                    text(name)
                        .size(12)
                        .style(on_surface_style)
                        .width(Length::Fill)
                        .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            );
        }

        let col = column![pick_btn, list,]
            .spacing(14)
            .padding(28)
            .width(Length::Fill)
            .align_x(iced::Alignment::Center);
        container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    pub(crate) fn root_superkey_popup(&self) -> Element<'_, Message> {
        // M3 text-input dialog — same shape as root_run_id_popup /
        // root_kernel_version_popup so the three APatch-flow popups
        // feel consistent (380 wide, outlined Cancel + filled OK,
        // shared `m3_dialog` scrim + 28-radius card).
        let input = iced::widget::text_input(
            self.t("apatch_superkey_placeholder"),
            &self.root.superkey_buffer,
        )
        .on_input(|__v| Message::Root(RootMsg::RootSuperkeyInput(__v)))
        .on_submit(Message::Root(RootMsg::RootSuperkeyConfirm))
        .secure(true)
        .padding([10, 12])
        .width(Length::Fill)
        .style(m3_text_input_style);

        let err: Element<'_, Message> = match &self.error_msg {
            Some(e) => text(e.clone())
                .size(12)
                .style(|t: &Theme| {
                    let p = pal_of(t);
                    iced::widget::text::Style {
                        color: Some(p.error),
                    }
                })
                .into(),
            None => Space::new().height(0).into(),
        };

        // Two-stage flow: first-entry vs verification re-entry. The
        // title + subtitle swap so the user knows the first Confirm
        // didn't commit the key yet, plus the password-manager / form
        // autofill heuristics in the OS see "different" prompts.
        let on_verify_stage = self.root.superkey_first_entry.is_some();
        let title_key = if on_verify_stage {
            "apatch_superkey_verify_title"
        } else {
            "apatch_superkey_title"
        };
        let subtitle_key = if on_verify_stage {
            "apatch_superkey_verify_subtitle"
        } else {
            "apatch_superkey_subtitle"
        };

        let content = column![
            text(self.t(title_key).to_string()).size(20),
            text(self.t(subtitle_key).to_string())
                .size(13)
                .style(muted_style),
            input,
            err,
            row![
                Space::new().width(Length::Fill),
                m3_text_button(self.t("btn_cancel").to_string())
                    .on_press(Message::Root(RootMsg::RootSuperkeyCancel)),
                m3_filled_button(self.t("btn_ok").to_string())
                    .on_press(Message::Root(RootMsg::RootSuperkeyConfirm)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(14)
        .padding(24)
        .width(380);

        m3_dialog(content.into())
    }

    pub(crate) fn root_run_id_popup(&self) -> Element<'_, Message> {
        // M3 text-input dialog — 380 wide, outlined Cancel + filled OK.
        let input = iced::widget::text_input(
            self.t("nightly_manual_placeholder"),
            &self.root.run_id_buffer,
        )
        .on_input(|__v| Message::Root(RootMsg::RootRunIdInput(__v)))
        .on_submit(Message::Root(RootMsg::RootRunIdConfirm))
        .padding([10, 12])
        .width(Length::Fill)
        .style(m3_text_input_style);

        let err: Element<'_, Message> = match &self.error_msg {
            Some(e) => text(e.clone())
                .size(12)
                .style(|t: &Theme| {
                    let p = pal_of(t);
                    iced::widget::text::Style {
                        color: Some(p.error),
                    }
                })
                .into(),
            None => Space::new().height(0).into(),
        };

        let content = column![
            text(self.t("nightly_manual_title").to_string()).size(20),
            text(self.t("nightly_manual_subtitle").to_string())
                .size(13)
                .style(muted_style),
            input,
            err,
            row![
                Space::new().width(Length::Fill),
                m3_text_button(self.t("btn_cancel").to_string())
                    .on_press(Message::Root(RootMsg::RootRunIdCancel)),
                m3_filled_button(self.t("btn_ok").to_string())
                    .on_press(Message::Root(RootMsg::RootRunIdConfirm)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(14)
        .padding(24)
        .width(380);

        m3_dialog(content.into())
    }

    pub(crate) fn root_kernel_version_popup(&self) -> Element<'_, Message> {
        let input = iced::widget::text_input(
            self.t("root_kernel_version_placeholder"),
            &self.root.kernel_version_buffer,
        )
        .on_input(|__v| Message::Root(RootMsg::RootKernelVersionInput(__v)))
        .on_submit(Message::Root(RootMsg::RootKernelVersionConfirm))
        .padding([10, 12])
        .width(Length::Fill)
        .style(m3_text_input_style);

        let err: Element<'_, Message> = match &self.error_msg {
            Some(e) => text(e.clone())
                .size(12)
                .style(|t: &Theme| {
                    let p = pal_of(t);
                    iced::widget::text::Style {
                        color: Some(p.error),
                    }
                })
                .into(),
            None => Space::new().height(0).into(),
        };

        let content = column![
            text(self.t("root_kernel_version_manual_title").to_string()).size(20),
            text(self.t("root_kernel_version_manual_subtitle").to_string())
                .size(13)
                .style(muted_style),
            input,
            err,
            row![
                Space::new().width(Length::Fill),
                m3_text_button(self.t("btn_cancel").to_string())
                    .on_press(Message::Root(RootMsg::RootKernelVersionCancel)),
                m3_filled_button(self.t("btn_ok").to_string())
                    .on_press(Message::Root(RootMsg::RootKernelVersionConfirm)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(14)
        .padding(24)
        .width(380);

        m3_dialog(content.into())
    }

    fn root_list_option_card(
        icon: Element<'static, Message>,
        label: &str,
        sub: &str,
        selected: bool,
        msg: Option<Message>,
    ) -> Element<'static, Message> {
        let enabled = msg.is_some();
        let label_style_fn = if enabled {
            on_surface_style
        } else {
            muted_style
        };
        let desc: Element<'static, Message> = if sub.is_empty() {
            text(" ").size(12).width(Length::Fill).into()
        } else {
            text(sub.to_string())
                .size(12)
                .style(muted_style)
                .width(Length::Fill)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                .into()
        };
        let text_block = container(
            column![
                text(label.to_string())
                    .size(16)
                    .style(label_style_fn)
                    .width(Length::Fill),
                desc,
            ]
            .spacing(3)
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_y(Length::Fill);
        let body = row![icon_tile(icon), text_block]
            .spacing(16)
            .align_y(iced::Alignment::Center);

        let inner = container(body)
            .padding([10, 16])
            .width(Length::Fill)
            .height(Length::Fixed(ROOT_WIZARD_LIST_CARD_HEIGHT))
            .center_y(Length::Fixed(ROOT_WIZARD_LIST_CARD_HEIGHT))
            .style(move |t: &Theme| sel_card_style(t, selected && enabled));
        let btn = button(inner)
            .padding(0)
            .width(Length::Fill)
            .height(Length::Fixed(ROOT_WIZARD_LIST_CARD_HEIGHT));
        match msg {
            Some(m) => btn
                .on_press(m)
                .style(move |t: &Theme, status| sel_card_btn_style(t, status, selected))
                .into(),
            None => btn
                .style(|t: &Theme, _status| {
                    let p = pal_of(t);
                    button::Style {
                        background: Some(with_alpha(p.surface_container_low, 0.5).into()),
                        text_color: with_alpha(p.on_surface, 0.38),
                        border: iced::Border {
                            color: with_alpha(p.outline_variant, 0.6),
                            width: 1.0,
                            radius: theme::shape::LG.into(),
                        },
                        ..Default::default()
                    }
                })
                .into(),
        }
    }

    pub(crate) fn root_family_step(&self) -> Element<'_, Message> {
        let families = [
            Family::Magisk,
            Family::KernelSU,
            Family::APatch,
            Family::Skroot,
        ];
        let icon_size = 44.0;
        let mk = |f: Family| -> Element<'_, Message> {
            let card = Self::root_list_option_card(
                f.icon_sized(icon_size),
                self.t(f.label_key()),
                self.t(f.desc_key()),
                self.root.family == Some(f),
                Some(Message::Root(RootMsg::RootFamily(f))),
            );
            if f == Family::KernelSU {
                recommended_overlay(card, self.t("root_recommended_tip").to_string())
            } else {
                card
            }
        };

        let mut cards = column![].spacing(8).width(Length::Fill);
        for f in families {
            cards = cards.push(mk(f));
        }

        let col = column![cards,]
            .spacing(14)
            .padding([20, 28])
            .width(Length::Fill)
            .align_x(iced::Alignment::Center);
        centered_step(col, ROOT_WIZARD_LIST_MAX_WIDTH)
    }

    pub(crate) fn root_provider_step(&self) -> Element<'_, Message> {
        let family = self.root.family.unwrap_or(Family::KernelSU);
        let providers = family.providers();

        if providers.len() > 2 {
            let mut cards = column![].spacing(8).width(Length::Fill);
            for &p in providers {
                let sub = p.desc_key().map(|k| self.t(k)).unwrap_or("");
                let card = Self::root_list_option_card(
                    p.icon_sized(44.0),
                    self.t(p.label_key()),
                    sub,
                    self.root.provider == Some(p),
                    Some(Message::Root(RootMsg::RootProvider(p))),
                );
                let card = if p == Provider::KernelSU {
                    recommended_overlay(card, self.t("root_recommended_tip").to_string())
                } else {
                    card
                };
                cards = cards.push(card);
            }

            let col = column![cards,]
                .spacing(14)
                .padding([20, 28])
                .width(Length::Fill)
                .align_x(iced::Alignment::Center);
            return centered_step(col, ROOT_WIZARD_LIST_MAX_WIDTH);
        }

        let side = self.wizard_square_side();
        let card = |p: Provider, selected: bool| -> Element<'_, Message> {
            let sub = p.desc_key().map(|k| self.t(k)).unwrap_or("");
            // Smaller brand logo (52 vs 72) so the 72px SVG doesn't
            // overflow the fixed 200px square once the label/desc wraps.
            icon_option_card_sub_square_sized(
                p.icon_sized(52.0),
                self.t(p.label_key()),
                sub,
                selected,
                Message::Root(RootMsg::RootProvider(p)),
                side,
            )
        };

        let mut cards = row![].spacing(12).align_y(iced::Alignment::Center);
        for &p in providers {
            cards = cards.push(card(p, self.root.provider == Some(p)));
        }

        let col = column![cards,]
            .spacing(14)
            .padding(28)
            .width(Length::Fill)
            .align_x(iced::Alignment::Center);
        centered_step(col, self.square_step_max_width(providers.len()))
    }

    pub(crate) fn root_file_step(&self, subtitle: &str) -> Element<'_, Message> {
        let selected = self.root.file_path.is_some();
        let status_text = if let Some(p) = &self.root.file_path {
            p.clone()
        } else {
            self.t("flash_folder_placeholder").to_string()
        };

        let btn_label = if self.root.is_gki() {
            self.t("btn_browse_kernel_image")
        } else {
            self.t("btn_browse_apk")
        };

        let btn = button(
            container(
                column![
                    text(btn_label.to_string()).size(14).center(),
                    text(subtitle.to_string())
                        .size(11)
                        .style(muted_style)
                        .center(),
                ]
                .spacing(6)
                .width(Length::Fill)
                .align_x(iced::Alignment::Center),
            )
            .padding([20, 24])
            .width(280)
            .style(move |t: &Theme| sel_card_style(t, selected)),
        )
        .on_press(Message::Root(RootMsg::RootSelectFile))
        .padding(0)
        .style(move |t: &Theme, status| sel_card_btn_style(t, status, selected));

        // Root OTA file picker flips between AnyKernel3 zip + raw
        // boot.img (GKI route) and provider APK (Magisk fork / APatch
        // manual) — mirror the dialog filter so recents don't surface
        // the wrong family.
        let accepted: &[&str] = if self.root.is_gki() {
            &["zip", "img"]
        } else {
            &["apk"]
        };
        let chips = self.recent_file_chips(
            accepted,
            |p| Message::RecentFilePicked(PickerTarget::RootFile, p),
            "picker_recents",
        );
        let col = column![
            btn,
            text(status_text)
                .size(12)
                .width(Length::Fill)
                .style(move |t: &Theme| {
                    let p = pal_of(t);
                    iced::widget::text::Style {
                        color: Some(if selected { p.success } else { p.outline }),
                    }
                })
                .center()
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
            chips,
        ]
        .spacing(14)
        .padding(28)
        .width(Length::Fill)
        .align_x(iced::Alignment::Center);
        container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    pub(crate) fn root_folder_step(&self) -> Element<'_, Message> {
        // Root pipeline now needs only the EDL loader (`.melf`) — the
        // full firmware folder was dropped when dump/flash stopped
        // depending on `rawprogram*.xml` and started resolving partition
        // names against the device's on-storage GPT. File-pick only.
        let selected = self.root.folder_path.is_some();
        let status = if let Some(p) = &self.root.folder_path {
            p.clone()
        } else {
            self.t("edl_loader_placeholder").to_string()
        };
        let btn = button(
            container(
                column![
                    text(self.t("btn_browse_loader").to_string())
                        .size(14)
                        .center(),
                    text(self.loader_picker_desc())
                        .size(11)
                        .style(muted_style)
                        .center(),
                ]
                .spacing(6)
                .width(Length::Fill)
                .align_x(iced::Alignment::Center),
            )
            .padding([20, 24])
            .width(280)
            .style(move |t: &Theme| sel_card_style(t, selected)),
        )
        .on_press(Message::Root(RootMsg::RootSelectFolder))
        .padding(0)
        .style(move |t: &Theme, status| sel_card_btn_style(t, status, selected));
        let chips = self.recent_file_chips(
            LOADER_PICKER_EXTS,
            |p| Message::RecentFilePicked(PickerTarget::RootLoader, p),
            "picker_recents",
        );
        let col = column![
            btn,
            text(status)
                .size(12)
                .width(Length::Fill)
                .style(move |t: &Theme| {
                    let p = pal_of(t);
                    iced::widget::text::Style {
                        color: Some(if selected { p.success } else { p.outline }),
                    }
                })
                .center()
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
            chips,
        ]
        .spacing(14)
        .padding(28)
        .width(Length::Fill)
        .align_x(iced::Alignment::Center);
        container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    pub(crate) fn root_mode_step(&self) -> Element<'_, Message> {
        let side = self.wizard_square_side();
        let tb323fu = self.is_tb323fu();
        let unsupported_tb323fu = tr_args!("model_unsupported", model = "TB323FU");
        let lkm_card = icon_option_card_sub_square_sized(
            RootMode::Lkm.icon(),
            self.t(RootMode::Lkm.label_key()),
            self.t(RootMode::Lkm.desc_key()),
            self.root.mode == Some(RootMode::Lkm),
            Message::Root(RootMsg::RootMode(RootMode::Lkm)),
            side,
        );
        let lkm_card = recommended_overlay(lkm_card, self.t("root_recommended_tip").to_string());
        // TODO(root): LTBox currently only swaps the boot.img Image for
        // GKI, which corrupts boot on TB323FU. Keep GKI disabled until
        // vbmeta handling is added.
        let gki_card: Element<'_, Message> = if tb323fu {
            icon_option_card_sub_square_disabled_sized(
                RootMode::Gki.icon_disabled(),
                self.t(RootMode::Gki.label_key()),
                &unsupported_tb323fu,
                side,
            )
        } else {
            icon_option_card_sub_square_sized(
                RootMode::Gki.icon(),
                self.t(RootMode::Gki.label_key()),
                self.t(RootMode::Gki.desc_key()),
                self.root.mode == Some(RootMode::Gki),
                Message::Root(RootMsg::RootMode(RootMode::Gki)),
                side,
            )
        };
        let col = column![row![lkm_card, gki_card,].spacing(12),]
            .spacing(14)
            .padding(28)
            .width(Length::Fill)
            .align_x(iced::Alignment::Center);
        centered_step(col, self.square_step_max_width(2))
    }

    pub(crate) fn root_skroot_flavor_step(&self) -> Element<'_, Message> {
        let side = self.wizard_square_side();
        let lite = icon_option_card_sub_square_sized(
            SkrootFlavor::Lite.icon(),
            self.t(SkrootFlavor::Lite.label_key()),
            self.t(SkrootFlavor::Lite.desc_key()),
            self.root.skroot_flavor == Some(SkrootFlavor::Lite),
            Message::Root(RootMsg::RootSkrootFlavor(SkrootFlavor::Lite)),
            side,
        );
        let pro = icon_option_card_sub_square_disabled_sized(
            SkrootFlavor::Pro.icon_disabled(),
            self.t(SkrootFlavor::Pro.label_key()),
            self.t(SkrootFlavor::Pro.desc_key()),
            side,
        );

        let col = column![row![lite, pro].spacing(12),]
            .spacing(14)
            .padding(28)
            .width(Length::Fill)
            .align_x(iced::Alignment::Center);
        centered_step(col, self.square_step_max_width(2))
    }

    pub(crate) fn root_version_step(&self) -> Element<'_, Message> {
        let side = self.wizard_square_side();
        let mk = |choice: VerChoice| -> Element<'_, Message> {
            let card = icon_option_card_sub_square_sized(
                choice.icon(),
                self.t(choice.label_key()),
                self.t(choice.desc_key()),
                self.root.version == Some(choice),
                Message::Root(RootMsg::RootVersion(choice)),
                side,
            );
            if choice == VerChoice::Stable {
                recommended_overlay(card, self.t("root_recommended_tip").to_string())
            } else {
                card
            }
        };

        // ReSukiSU ships nightlies only — hide the Stable card so users
        // can't pick a channel that has no release assets. Other providers
        // keep both.
        let version_row = if self.root.provider == Some(Provider::ReSukiSU) {
            row![mk(VerChoice::Nightly)].spacing(12)
        } else {
            row![mk(VerChoice::Stable), mk(VerChoice::Nightly)].spacing(12)
        };

        let col = column![version_row,]
            .spacing(14)
            .padding(28)
            .width(Length::Fill)
            .align_x(iced::Alignment::Center);
        let columns = if self.root.provider == Some(Provider::ReSukiSU) {
            1
        } else {
            2
        };
        centered_step(col, self.square_step_max_width(columns))
    }

    pub(crate) fn root_nightly_source_step(&self) -> Element<'_, Message> {
        let side = self.wizard_square_side();
        let mk = |src: NightlySource| -> Element<'_, Message> {
            icon_option_card_sub_square_sized(
                src.icon(),
                self.t(src.label_key()),
                self.t(src.desc_key()),
                self.root.nightly_source == Some(src),
                Message::Root(RootMsg::RootNightlySource(src)),
                side,
            )
        };

        // Committed ManualInput shows a chip beneath the cards; click re-opens.
        let chip: Element<'_, Message> =
            match (self.root.nightly_source, self.root.run_id.as_deref()) {
                (Some(NightlySource::ManualInput), Some(id)) if !id.is_empty() => {
                    let label = tr_args!("nightly_manual_committed", id = id);
                    button(text(label).size(13).style(on_surface_style))
                        .padding([8, 14])
                        .on_press(Message::Root(RootMsg::RootNightlySource(
                            NightlySource::ManualInput,
                        )))
                        .style(|t: &Theme, status| {
                            let p = pal_of(t);
                            let bg_a = match status {
                                button::Status::Hovered => 0.18,
                                _ => 0.10,
                            };
                            button::Style {
                                background: Some(with_alpha(p.on_surface, bg_a).into()),
                                text_color: p.on_surface,
                                border: iced::Border {
                                    radius: 6.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        })
                        .into()
                }
                _ => Space::new().height(0).into(),
            };

        let col = column![
            row![
                mk(NightlySource::AutoDetect),
                mk(NightlySource::ManualInput)
            ]
            .spacing(12),
            chip,
        ]
        .spacing(14)
        .padding(28)
        .width(Length::Fill)
        .align_x(iced::Alignment::Center);
        centered_step(col, self.square_step_max_width(2))
    }

    pub(crate) fn root_confirm_step(&self) -> Element<'_, Message> {
        let dash = "—".to_string();
        let fam = self
            .root
            .family
            .map(|f| self.t(f.label_key()).to_string())
            .unwrap_or_else(|| dash.clone());

        let mut grid_rows = vec![info_kv_center(self.t("root_step_type"), &fam)];
        let mut trailing_rows = Vec::new();

        if self.root.is_skroot() {
            let flavor = self
                .root
                .skroot_flavor
                .map(|f| self.t(f.label_key()).to_string())
                .unwrap_or_else(|| dash.clone());
            grid_rows.push(info_kv_center(self.t("root_step_skroot_flavor"), &flavor));
        } else {
            let mode = self
                .root
                .mode
                .map(|m| self.t(m.label_key()).to_string())
                .unwrap_or_else(|| dash.clone());
            grid_rows.push(info_kv_center(self.t("root_step_mode"), &mode));
        }

        if self.root.is_gki() {
            let path = self.root.file_path.clone().unwrap_or_else(|| dash.clone());
            trailing_rows.push(info_kv_center(self.t("root_step_kernel"), &path));
        } else if self.root.is_forks() {
            let path = self.root.file_path.clone().unwrap_or_else(|| dash.clone());
            grid_rows.push(info_kv_center(
                self.t("root_step_provider"),
                self.t("provider_magisk_forks"),
            ));
            trailing_rows.push(info_kv_center(self.t("root_step_apk"), &path));
        } else if !self.root.is_skroot() {
            let prov = self
                .root
                .provider
                .map(|p| self.t(p.label_key()).to_string())
                .unwrap_or_else(|| dash.clone());
            let ver = self
                .root
                .version
                .map(|v| self.t(v.label_key()).to_string())
                .unwrap_or_else(|| dash.clone());
            grid_rows.push(info_kv_center(self.t("root_step_provider"), &prov));
            grid_rows.push(info_kv_center(self.t("root_step_version"), &ver));
            if self.root.is_nightly() {
                let src = self
                    .root
                    .nightly_source
                    .map(|s| self.t(s.label_key()).to_string())
                    .unwrap_or_else(|| dash.clone());
                grid_rows.push(info_kv_center(self.t("root_step_source"), &src));
                if self.root.nightly_source == Some(NightlySource::ManualInput) {
                    let id = self.root.run_id.clone().unwrap_or_else(|| dash.clone());
                    grid_rows.push(info_kv_center(self.t("nightly_run_id_label"), &id));
                }
            }
        }

        if self.root.is_apatch() {
            // Count only — don't echo paths (noisy) or the superkey (secret).
            let kpm_summary = if self.root.kpm_paths.is_empty() {
                self.t("root_kpm_none").to_string()
            } else {
                tr_args!(
                    "root_kpm_count_tmpl",
                    n = self.root.kpm_paths.len().to_string()
                )
            };
            grid_rows.push(info_kv_center(self.t("root_step_kpm"), &kpm_summary));
        }

        let folder = self
            .root
            .folder_path
            .clone()
            .unwrap_or_else(|| dash.clone());
        trailing_rows.push(info_kv_center(self.t("edl_loader_label"), &folder));

        self.confirm_step_frame(vec![], grid_rows, trailing_rows)
    }

    pub(crate) fn root_flash_step(&self) -> Element<'_, Message> {
        self.exec_step_view()
    }
}
