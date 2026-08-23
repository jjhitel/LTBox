//! Unroot wizard view + steps. Extracted from `main.rs`.

use crate::*;
use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Theme};

impl App {
    pub(crate) fn view_unroot_wizard(&self) -> Element<'_, Message> {
        if self.log_popup_open && self.unroot.is_in_exec() {
            return self.log_popup_view();
        }
        let step_labels: Vec<&str> = UNROOT_STEPS.iter().map(|k| self.t(k)).collect();
        let step_bar = wizard_step_bar(&step_labels, self.unroot.step);
        let body = match self.unroot.step {
            0 => self.unroot_type_step(),
            1 => self.unroot_loader_step(),
            2 => self.unroot_folder_step(),
            3 => self.unroot_confirm_step(),
            _ => self.unroot_exec_step(),
        };
        let nav = if self.unroot.step < 4 {
            let is_start = self.unroot.step == 3;
            let label_owned = if is_start {
                self.t("btn_start").to_string()
            } else {
                self.t("btn_next").to_string()
            };
            let can = self.unroot.can_next()
                && !(self.busy && is_start)
                && (!is_start || self.device_reachable());
            wizard_nav_generic(
                self.unroot.step > 0,
                &label_owned,
                can,
                self.t("btn_back"),
                Message::Unroot(UnrootMsg::UnrootBack),
                Message::Unroot(UnrootMsg::UnrootNext),
            )
        } else {
            empty_wizard_nav()
        };
        let mut layout = column![].width(Length::Fill).height(Length::Fill);
        if let Some(header) = self.unroot_action_bar() {
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

    fn unroot_action_bar(&self) -> Option<Element<'_, Message>> {
        let (title, subtitle) = match self.unroot.step {
            0 => (
                self.t("unroot_method_title").to_string(),
                self.t("unroot_method_subtitle").to_string(),
            ),
            1 => (
                self.t("edl_loader_title").to_string(),
                self.t("edl_loader_subtitle").to_string(),
            ),
            2 => {
                let desc = self
                    .unroot
                    .unroot_type
                    .map(|t| self.t(t.folder_desc_key()).to_string())
                    .unwrap_or_else(|| self.t("unroot_folder_placeholder").to_string());
                (self.t("unroot_folder_title").to_string(), desc)
            }
            3 => (
                self.t("unroot_confirm_title").to_string(),
                self.t("unroot_confirm_subtitle").to_string(),
            ),
            _ => return Some(self.exec_action_bar()),
        };
        Some(wizard_action_bar(title, Some(subtitle)))
    }

    pub(crate) fn unroot_type_step(&self) -> Element<'_, Message> {
        let side = self.wizard_square_side();
        // Unroot reuses the Lucide puzzle/layers glyphs that the root
        // wizard uses for the LKM/GKI pick — context (title + label)
        // disambiguates.
        let lkm_icon = lucide_primary(icon::root_lkm(), self.wizard_square_icon());
        let gki_icon = lucide_primary(icon::root_gki(), self.wizard_square_icon());
        let col = column![
            row![
                icon_option_card_sub_square_sized(
                    lkm_icon,
                    self.t(UnrootType::MagiskLkm.label_key()),
                    self.t(UnrootType::MagiskLkm.desc_key()),
                    self.unroot.unroot_type == Some(UnrootType::MagiskLkm),
                    Message::Unroot(UnrootMsg::SetUnrootType(UnrootType::MagiskLkm)),
                    side,
                ),
                icon_option_card_sub_square_sized(
                    gki_icon,
                    self.t(UnrootType::APatchGki.label_key()),
                    self.t(UnrootType::APatchGki.desc_key()),
                    self.unroot.unroot_type == Some(UnrootType::APatchGki),
                    Message::Unroot(UnrootMsg::SetUnrootType(UnrootType::APatchGki)),
                    side,
                ),
            ]
            .spacing(12),
        ]
        .spacing(14)
        .padding(28)
        .width(Length::Fill)
        .align_x(iced::Alignment::Center);
        centered_step(col, self.square_step_max_width(2))
    }

    pub(crate) fn unroot_loader_step(&self) -> Element<'_, Message> {
        let selected = self.unroot.loader_path.is_some();
        let status = if let Some(p) = &self.unroot.loader_path {
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
        .on_press(Message::Unroot(UnrootMsg::UnrootSelectLoader))
        .padding(0)
        .style(move |t: &Theme, status| sel_card_btn_style(t, status, selected));
        // Recent loader picks — same shared `File` bucket (filtered to loader
        // extensions) as the Root / dump / flash loader pickers. This was the
        // only loader step missing its recents strip.
        let chips = self.recent_file_chips(
            LOADER_PICKER_EXTS,
            |p| Message::RecentFilePicked(PickerTarget::UnrootLoader, p),
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

    pub(crate) fn unroot_folder_step(&self) -> Element<'_, Message> {
        let selected = self.unroot.folder_path.is_some();
        let desc_owned = self
            .unroot
            .unroot_type
            .map(|t| self.t(t.folder_desc_key()).to_string())
            .unwrap_or_else(|| self.t("unroot_folder_placeholder").to_string());
        let status = if let Some(p) = &self.unroot.folder_path {
            p.clone()
        } else {
            self.t("flash_folder_placeholder").to_string()
        };
        let btn = button(
            container(
                column![
                    text(self.t("btn_browse_folder").to_string())
                        .size(14)
                        .center(),
                    text(desc_owned).size(11).style(muted_style).center(),
                ]
                .spacing(6)
                .width(Length::Fill)
                .align_x(iced::Alignment::Center),
            )
            .padding([20, 24])
            .width(280)
            .style(move |t: &Theme| sel_card_style(t, selected)),
        )
        .on_press(Message::Unroot(UnrootMsg::UnrootSelectFolder))
        .padding(0)
        .style(move |t: &Theme, status| sel_card_btn_style(t, status, selected));
        let chips = self.recent_chips(
            self.recent_paths
                .recent(PickerTarget::UnrootFolder.kind().storage_key()),
            |p| Message::RecentFolderPicked(PickerTarget::UnrootFolder, p),
            "picker_recents",
            false,
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

    pub(crate) fn unroot_confirm_step(&self) -> Element<'_, Message> {
        let dash = "—".to_string();
        let method = self
            .unroot
            .unroot_type
            .map(|t| self.t(t.label_key()).to_string())
            .unwrap_or_else(|| dash.clone());
        let loader = self
            .unroot
            .loader_path
            .clone()
            .unwrap_or_else(|| dash.clone());
        let folder = self
            .unroot
            .folder_path
            .clone()
            .unwrap_or_else(|| dash.clone());
        self.confirm_step_frame(
            vec![],
            vec![info_kv_center(self.t("unroot_step_method"), &method)],
            vec![
                info_kv_center(self.t("edl_loader_label"), &loader),
                info_kv_center(self.t("unroot_folder_title"), &folder),
            ],
        )
    }

    pub(crate) fn unroot_exec_step(&self) -> Element<'_, Message> {
        self.exec_step_view()
    }
}
