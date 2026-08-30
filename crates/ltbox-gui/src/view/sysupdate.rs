//! System-update wizard view + steps + the shared exec-step view. Extracted from `main.rs`.

use crate::*;
use iced::widget::{Space, button, column, container, row, text};
use iced::{Element, Length, Theme};
use ltbox_core::tr_args;
impl App {
    pub(crate) fn view_sysupdate_wizard(&self) -> Element<'_, Message> {
        // Exec-step log popup overlay — without this the "Show log" button
        // on the exec card was a no-op for System Update (Flash/Root/Unroot
        // all had it wired; SysUpdate had been missed).
        if self.log_popup_open && self.sysupdate.is_in_exec() {
            return self.log_popup_view();
        }
        let steps = self.sysupdate.steps();
        let step_labels: Vec<&str> = steps.iter().map(|k| self.t(k)).collect();
        let step_bar = wizard_step_bar(&step_labels, self.sysupdate.step);
        let is_rescue = self.sysupdate.is_rescue();
        let body = if is_rescue {
            match self.sysupdate.step {
                0 => self.sysupdate_action_step(),
                1 => self.sysupdate_rescue_folder_step(),
                2 => self.sysupdate_confirm_step(),
                _ => self.sysupdate_exec_step(),
            }
        } else {
            match self.sysupdate.step {
                0 => self.sysupdate_action_step(),
                1 => self.sysupdate_confirm_step(),
                _ => self.sysupdate_exec_step(),
            }
        };
        let last_nav_step = steps.len() - 2; // Exec step has no nav row.
        let nav = if self.sysupdate.step <= last_nav_step {
            let is_start = self.sysupdate.step == last_nav_step;
            let label_owned = if is_start {
                self.t("btn_start").to_string()
            } else {
                self.t("btn_next").to_string()
            };
            let can = self.sysupdate.can_next()
                && !(self.busy && is_start)
                && (!is_start || self.device_reachable());
            wizard_nav_generic(
                self.sysupdate.step > 0,
                &label_owned,
                can,
                self.t("btn_back"),
                Message::Sys(SysMsg::SysBack),
                Message::Sys(SysMsg::SysNext),
            )
        } else {
            empty_wizard_nav()
        };
        let mut layout = column![].width(Length::Fill).height(Length::Fill);
        if let Some(header) = self.sysupdate_action_bar() {
            layout = layout.push(header);
        }
        let core: Element<'_, Message> = layout
            .push(step_bar)
            .push(body)
            .push(nav)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        if self.sysupdate.rescue_region_popup_open {
            iced::widget::Stack::with_children(vec![core, self.rescue_region_popup_view()]).into()
        } else {
            core
        }
    }

    fn sysupdate_action_bar(&self) -> Option<Element<'_, Message>> {
        let rescue = self.sysupdate.is_rescue();
        let (title, subtitle) = match (rescue, self.sysupdate.step) {
            (_, 0) => (
                self.t("sysupdate_action_title").to_string(),
                self.t("sysupdate_action_subtitle").to_string(),
            ),
            (true, 1) => (
                self.t("edl_loader_title").to_string(),
                self.t("edl_loader_subtitle").to_string(),
            ),
            (true, 2) | (false, 1) => {
                let desc = self
                    .sysupdate
                    .action
                    .map(|a| self.t(a.desc_key()).to_string())
                    .unwrap_or_default();
                (self.t("sysupdate_confirm_title").to_string(), desc)
            }
            _ => return Some(self.exec_action_bar()),
        };
        Some(wizard_action_bar(title, Some(subtitle)))
    }

    pub(crate) fn sysupdate_action_step(&self) -> Element<'_, Message> {
        let d = self.density();
        let side = self.wizard_square_side();
        let off_icon = lucide_primary(icon::tile_update_off(), self.wizard_square_icon());
        let on_icon = lucide_primary(icon::tile_update_on(), self.wizard_square_icon());
        // TB323FU's vendor_boot/vbmeta sit on a different UFS LUN than the
        // Boot Recovery worker targets, so the flow can't run on it — disable
        // the card (alongside the non-Qualcomm platform gate).
        let rescue_disabled =
            self.platform_supported == Some(false) || self.is_tb323fu() || self.is_xiaoxin_pro13();
        // Gray the icon when disabled, matching the other wizards' disabled
        // option cards (region ROW / OtherRegion).
        let rescue_icon = if rescue_disabled {
            lucide_disabled(icon::tile_rescue(), self.wizard_square_icon())
        } else {
            lucide_primary(icon::tile_rescue(), self.wizard_square_icon())
        };
        let mut cards = row![
            icon_option_card_sub_square_sized(
                off_icon,
                self.t(SysUpdateAction::Disable.label_key()),
                self.t(SysUpdateAction::Disable.desc_key()),
                self.sysupdate.action == Some(SysUpdateAction::Disable),
                Message::Sys(SysMsg::SysAction(SysUpdateAction::Disable)),
                side,
            ),
            icon_option_card_sub_square_sized(
                on_icon,
                self.t(SysUpdateAction::Enable.label_key()),
                self.t(SysUpdateAction::Enable.desc_key()),
                self.sysupdate.action == Some(SysUpdateAction::Enable),
                Message::Sys(SysMsg::SysAction(SysUpdateAction::Enable)),
                side,
            ),
        ]
        .spacing(d.space(12.0));
        if rescue_disabled {
            // Disabled rescue card — no on_press, grayed out; still mirrors
            // the sub-row layout of the other tiles with the Qualcomm-required
            // hint so the label sits at the same height.
            let rescue_req = if self.is_tb323fu() {
                tr_args!("model_unsupported", model = "TB323FU")
            } else if self.is_xiaoxin_pro13() {
                tr_args!("model_unsupported", model = "TB376FC / TB390FU")
            } else {
                self.t("sysupdate_rescue_req").to_string()
            };
            let content = column![
                icon_tile(rescue_icon),
                text(self.t("sysupdate_rescue").to_string())
                    .size(d.text(13.0))
                    .width(Length::Fill)
                    .center()
                    .style(muted_style),
                text(rescue_req)
                    .size(d.text(11.0))
                    .width(Length::Fill)
                    .center()
                    .style(muted_style),
            ]
            .spacing(d.space(8.0))
            .align_x(iced::Alignment::Center);
            cards = cards.push(
                button(
                    container(content)
                        .padding(d.padding(20.0, 16.0))
                        .width(Length::Fixed(side))
                        .height(side)
                        .center_x(side)
                        .center_y(side)
                        .style(|t: &Theme| {
                            theme::surface_card_style(
                                t,
                                theme::SurfaceLevel::Lowest,
                                theme::shape::LG,
                                0,
                            )
                        }),
                )
                .padding(0)
                .width(Length::Fixed(side))
                .style(|t: &Theme, _s| button::Style {
                    background: None,
                    text_color: pal_of(t).on_surface,
                    ..Default::default()
                }),
            );
        } else {
            cards = cards.push(icon_option_card_sub_square_sized(
                rescue_icon,
                self.t(SysUpdateAction::Rescue.label_key()),
                self.t(SysUpdateAction::Rescue.desc_key()),
                self.sysupdate.action == Some(SysUpdateAction::Rescue),
                Message::Sys(SysMsg::SysAction(SysUpdateAction::Rescue)),
                side,
            ));
        }
        let col = column![cards,]
            .spacing(d.space(14.0))
            .padding(d.space(28.0))
            .width(Length::Fill)
            .align_x(iced::Alignment::Center);
        centered_step(col, self.square_step_max_width(3))
    }

    pub(crate) fn sysupdate_confirm_step(&self) -> Element<'_, Message> {
        let dash = "—".to_string();
        let action = self
            .sysupdate
            .action
            .map(|a| self.t(a.label_key()).to_string())
            .unwrap_or_else(|| dash.clone());
        let mut grid_rows = vec![info_kv_center(self.t("sysupdate_step_action"), &action)];
        let mut trailing_rows = Vec::new();
        // Rescue: echo the chosen firmware folder + region so the user
        // confirms exactly what's about to flash.
        if self.sysupdate.is_rescue() {
            let folder = self
                .sysupdate
                .rescue_folder
                .clone()
                .unwrap_or_else(|| dash.clone());
            let region = self
                .sysupdate
                .rescue_region
                .map(|r| self.t(r.label_key()).to_string())
                .unwrap_or_else(|| dash.clone());
            trailing_rows.push(info_kv_center(self.t("edl_loader_label"), &folder));
            grid_rows.push(info_kv_center(self.t("rescue_region_label"), &region));
        }
        self.confirm_step_frame(vec![], grid_rows, trailing_rows)
    }

    pub(crate) fn sysupdate_rescue_folder_step(&self) -> Element<'_, Message> {
        let d = self.density();
        // Boot Recovery now consumes only the EDL loader file —
        // dump+flash use GPT-by-name on a fixed LUN, no rawprogram*.xml
        // is read. Step layout still matches the flash / root / unroot
        // pickers (title + 280-wide card button + status path + recent
        // chips), just with file-picker semantics.
        let selected = self.sysupdate.rescue_folder.is_some();
        let status = if let Some(p) = &self.sysupdate.rescue_folder {
            p.clone()
        } else {
            self.t("edl_loader_placeholder").to_string()
        };
        let btn = button(
            container(
                column![
                    text(self.t("btn_browse_loader").to_string())
                        .size(d.text(14.0))
                        .center(),
                    text(self.loader_picker_desc())
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
        .on_press(Message::Sys(SysMsg::SysRescueSelectFolder))
        .padding(0)
        .style(move |t: &Theme, status| sel_card_btn_style(t, status, selected));
        // Loader recents share the File bucket with other loader
        // pickers (root, advanced) — filter to the same ext set the
        // dialog itself accepts.
        let chips = self.recent_file_chips(
            LOADER_PICKER_EXTS,
            |p| Message::Sys(SysMsg::SysRescueFolderChosen(Some(p))),
            "picker_recents",
        );
        let col = column![
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
            chips,
        ]
        .spacing(d.space(14.0))
        .padding(d.space(28.0))
        .width(Length::Fill)
        .align_x(iced::Alignment::Center);
        container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    pub(crate) fn sysupdate_exec_step(&self) -> Element<'_, Message> {
        self.exec_step_view()
    }

    pub(crate) fn exec_status_copy(&self) -> (String, String) {
        if self.busy {
            (
                self.t("exec_executing_title").to_string(),
                self.t("exec_executing_subtitle").to_string(),
            )
        } else if self.operation_error.is_some() {
            (
                self.t("exec_failed_title").to_string(),
                self.t("exec_failed_subtitle").to_string(),
            )
        } else {
            (
                self.t("exec_done_title").to_string(),
                self.t("exec_done_subtitle").to_string(),
            )
        }
    }

    pub(crate) fn exec_action_bar(&self) -> Element<'_, Message> {
        let (title, subtitle) = self.exec_status_copy();
        wizard_action_bar(title, Some(subtitle))
    }

    /// Reusable exec-step view with collapsible log panel.
    pub(crate) fn exec_step_view(&self) -> Element<'_, Message> {
        let d = self.density();
        let (_, detail) = self.exec_status_copy();
        let is_error = self.operation_error.is_some();
        let is_busy = self.busy;

        // Shared progress/result card for wizard exec steps. One scale on
        // both axes: the badge is a circle.
        let badge = d.size(80.0);
        // Shared progress/result card for wizard exec steps.
        let step_icon: Element<'_, Message> = if is_error {
            container(lucide_icon(
                icon::op_failed(),
                d.image(52.0),
                |t: &Theme| pal_of(t).error,
            ))
            .width(Length::Fixed(badge))
            .height(Length::Fixed(badge))
            .center_x(Length::Fixed(badge))
            .center_y(Length::Fixed(badge))
            .style(|t: &Theme| {
                let p = pal_of(t);
                container::Style {
                    background: Some(p.error_container.into()),
                    border: iced::Border {
                        radius: theme::shape::FULL.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .into()
        } else if is_busy {
            container(material_circular_progress(MaterialProgressSize::Hero))
                .width(Length::Fixed(badge))
                .height(Length::Fixed(badge))
                .center_x(Length::Fixed(badge))
                .center_y(Length::Fixed(badge))
                .into()
        } else {
            container(lucide_icon(icon::op_done(), d.image(52.0), |t: &Theme| {
                pal_of(t).success
            }))
            .width(Length::Fixed(badge))
            .height(Length::Fixed(badge))
            .center_x(Length::Fixed(badge))
            .center_y(Length::Fixed(badge))
            .style(|t: &Theme| {
                let p = pal_of(t);
                container::Style {
                    background: Some(
                        theme::mix_color(p.surface_container_high, p.success, 0.12).into(),
                    ),
                    border: iced::Border {
                        radius: theme::shape::FULL.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .into()
        };

        let (eyebrow_text, label_text) = if self.op_steps.is_empty() {
            (String::new(), detail.clone())
        } else {
            let idx = self.current_op_step.min(self.op_steps.len() - 1);
            let total = self.op_steps.len();
            let step = &self.op_steps[idx];
            let eyebrow_key = if is_error {
                "exec_step_eyebrow_failed"
            } else if is_busy {
                "exec_step_eyebrow_running"
            } else {
                "exec_step_eyebrow_done"
            };
            let eyebrow = tr_args!(
                eyebrow_key,
                n = (idx + 1).to_string(),
                total = total.to_string()
            );
            (eyebrow, step.label.clone())
        };

        let eyebrow_node: Element<'_, Message> = if eyebrow_text.is_empty() {
            Space::new().height(0).into()
        } else {
            text(eyebrow_text)
                .size(d.text(12.0))
                .style(move |t: &Theme| {
                    let p = pal_of(t);
                    let color = if is_error {
                        p.error
                    } else if is_busy {
                        p.primary
                    } else {
                        p.success
                    };
                    iced::widget::text::Style { color: Some(color) }
                })
                .into()
        };

        let mut card_body = column![
            eyebrow_node,
            text(label_text).size(d.text(18.0)).style(on_surface_style),
        ]
        .spacing(d.space(6.0))
        .width(Length::Fill);
        if let Some(progress_label) = self.firmware_flash_progress_label() {
            card_body = card_body.push(text(progress_label).size(d.text(13.0)).style(muted_style));
        }
        if is_error {
            if let Some(error) = self.operation_error.as_deref() {
                let summary = concise_error_summary(error, EXEC_ERROR_SUMMARY_MAX_CHARS);
                if !summary.is_empty() {
                    card_body =
                        card_body.push(text(summary).size(d.text(13.0)).style(|t: &Theme| {
                            iced::widget::text::Style {
                                color: Some(pal_of(t).error),
                            }
                        }));
                }
            }
            card_body = card_body.push(
                text(self.t("exec_error_log_hint").to_string())
                    .size(d.text(12.0))
                    .style(muted_style),
            );
        }
        let card_row = row![step_icon, card_body]
            .spacing(d.space(24.0))
            .align_y(iced::Alignment::Center);
        let step_card = container(card_row)
            .padding(d.padding(28.0, 32.0))
            .max_width(600)
            .width(Length::Fill)
            .style(move |t: &Theme| {
                let p = pal_of(t);
                let background = if is_error {
                    theme::mix_color(p.surface_container, p.error, 0.08)
                } else if is_busy {
                    p.surface_container_high
                } else {
                    theme::mix_color(p.surface_container, p.success, 0.08)
                };
                container::Style {
                    background: Some(background.into()),
                    border: iced::Border {
                        radius: theme::shape::XL.into(),
                        ..Default::default()
                    },
                    shadow: theme::elevation(1, theme::is_dark(t)),
                    ..Default::default()
                }
            });

        let has_output = !is_busy
            && self.current_view == View::Advanced
            && self.adv_wizard.output_dir.is_some()
            && self
                .adv_wizard
                .action
                .map(|action| action.produces_output())
                .unwrap_or(false);
        let action_layout = exec_action_layout(is_busy, is_error, has_output);
        let mut utility_actions = row![
            wizard_utility_action(
                icon::fab_show_log(),
                self.t("btn_show_log").to_string(),
                Some(Message::ToggleLogPopup(true)),
            ),
            wizard_utility_action(
                icon::fab_save_log(),
                self.t("btn_save_log").to_string(),
                Some(Message::SaveLog),
            ),
        ]
        .spacing(0)
        .align_y(iced::Alignment::Center);

        if action_layout.start_over_utility {
            utility_actions = utility_actions.push(wizard_utility_action(
                icon::fab_start_over(),
                self.t("btn_start_over").to_string(),
                Some(Message::StartOver),
            ));
        }

        let mut actions = row![wizard_utility_toolbar(utility_actions)]
            .spacing(WIZARD_FAB_SPACING)
            .align_y(iced::Alignment::Center)
            .height(Length::Fill);
        if let Some(primary) = action_layout.primary {
            actions = match primary {
                ExecPrimaryAction::StartOver => actions.push(wizard_primary_extended_fab(
                    icon::fab_start_over(),
                    self.t("btn_start_over").to_string(),
                    Some(Message::StartOver),
                    None,
                )),
                ExecPrimaryAction::OpenFolder => actions.push(wizard_primary_extended_fab(
                    icon::fab_open_folder(),
                    self.t("btn_open_folder").to_string(),
                    Some(Message::Adv(AdvMsg::AdvWizOpenOutputFolder)),
                    None,
                )),
            };
        }

        let col = column![step_card]
            .spacing(d.space(10.0))
            .padding(d.space(28.0))
            .width(Length::Fill)
            .align_x(iced::Alignment::Center);

        let body = container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill);

        column![
            body,
            wizard_fab_footer(row![].height(Length::Fill), actions),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
