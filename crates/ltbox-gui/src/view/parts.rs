//! Partition + physical-storage dump/flash wizard views + steps. Extracted from `main.rs`.

use crate::*;
use iced::widget::{self, Space, button, column, container, row, scrollable, text};
use iced::{Element, Length, Theme};

fn m3_erase_marker() -> Element<'static, Message> {
    let dash = container(Space::new())
        .width(Length::Fixed(FLASH_PARTS_ERASE_DASH_WIDTH))
        .height(Length::Fixed(FLASH_PARTS_ERASE_DASH_HEIGHT))
        .style(|t: &Theme| {
            let p = pal_of(t);
            container::Style {
                background: Some(p.on_error.into()),
                border: iced::Border {
                    radius: theme::shape::FULL.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        });

    container(dash)
        .width(Length::Fixed(FLASH_PARTS_MARKER_SIZE))
        .height(Length::Fixed(FLASH_PARTS_MARKER_SIZE))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(|t: &Theme| {
            let p = pal_of(t);
            container::Style {
                background: Some(p.error.into()),
                border: iced::Border {
                    color: p.error,
                    width: 2.0,
                    radius: theme::shape::XS.into(),
                },
                ..Default::default()
            }
        })
        .into()
}

impl App {
    pub(crate) fn view_flash_parts_wizard(&self) -> Element<'_, Message> {
        if self.log_popup_open && self.flash_parts.step >= 3 {
            return self.log_popup_view();
        }

        let step_labels: Vec<&str> = FLASH_PARTS_STEPS.iter().map(|k| self.t(k)).collect();
        let step_bar = wizard_step_bar(&step_labels, self.flash_parts.step);

        let body: Element<'_, Message> = match self.flash_parts.step {
            0 => self.flash_parts_loader_step(),
            1 => self.flash_parts_select_step(),
            2 => self.flash_parts_confirm_step(),
            _ => self.exec_step_view(),
        };

        let nav = if self.flash_parts.step < 3 {
            let label = match self.flash_parts.step {
                0 => self.t("btn_scan").to_string(),
                1 => self.t("btn_next").to_string(),
                2 => self.t("btn_start").to_string(),
                _ => self.t("btn_next").to_string(),
            };
            let is_start = self.flash_parts.step == 2 || self.flash_parts.step == 0;
            // No loader-fit gate here: by the Confirm step the device is already
            // in EDL (the GPT scan transitioned it) where the model can't be
            // polled, and the loader was already validated by that scan.
            let can = self.flash_parts.can_next()
                && !(self.busy && is_start)
                && (!is_start || self.device_reachable());
            let leading_action = if self.flash_parts.step == 1 {
                partition_table_leading_action(self.flash_parts.entry_connection)
            } else {
                WizardLeadingAction::Back
            };
            let leading_label = if leading_action == WizardLeadingAction::Cancel {
                self.t("btn_cancel")
            } else {
                self.t("btn_back")
            };
            wizard_nav_generic_with_leading_action(
                leading_action,
                &label,
                can,
                leading_label,
                if self.flash_parts.step == 0 {
                    Message::FlashParts(FlashPartsMsg::FlashPartsClose)
                } else {
                    Message::FlashParts(FlashPartsMsg::FlashPartsBack)
                },
                Message::FlashParts(FlashPartsMsg::FlashPartsNext),
            )
        } else {
            empty_wizard_nav()
        };

        let mut layout = column![].width(Length::Fill).height(Length::Fill);
        if let Some(header) = self.flash_parts_action_bar() {
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

    fn flash_parts_action_bar(&self) -> Option<Element<'_, Message>> {
        let (title, subtitle) = match self.flash_parts.step {
            0 => (
                self.t("edl_loader_title").to_string(),
                self.t("edl_loader_subtitle").to_string(),
            ),
            1 => (
                self.t("flash_parts_select_title").to_string(),
                self.t("flash_parts_select_subtitle").to_string(),
            ),
            2 => (
                self.t("flash_parts_confirm_title").to_string(),
                self.t("flash_parts_confirm_subtitle").to_string(),
            ),
            _ => return Some(self.exec_action_bar()),
        };
        Some(wizard_action_bar(title, Some(subtitle)))
    }

    /// Shared loader-picker card for the EDL parts / physical-storage
    /// wizards: a Browse-loader button, the resolved-path / error status
    /// line, and the recent-loader chips. Only the wizard's loader fields
    /// and the two Message variants differ between callers, so they are
    /// threaded in as params; the title / placeholder / accepted
    /// extensions / colors are identical across all four wizards.
    pub(crate) fn loader_picker_card<'a>(
        &'a self,
        loader_path: &'a Option<String>,
        loader_error: &'a Option<String>,
        on_select: Message,
        on_chosen: impl Fn(String) -> Message,
    ) -> Element<'a, Message> {
        let selected = loader_path.is_some();
        let status = match (loader_path, loader_error) {
            (_, Some(e)) => format!("⚠ {e}"),
            (Some(p), None) => p.clone(),
            _ => self.t("edl_loader_placeholder").to_string(),
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
        .on_press(on_select)
        .padding(0)
        .style(move |t: &Theme, status| sel_card_btn_style(t, status, selected));
        let has_error = loader_error.is_some();
        let status_style = move |t: &Theme| {
            let p = pal_of(t);
            iced::widget::text::Style {
                color: Some(if has_error {
                    p.error
                } else if selected {
                    p.success
                } else {
                    p.outline
                }),
            }
        };
        let chips = self.recent_file_chips(LOADER_PICKER_EXTS, on_chosen, "picker_recents");
        let col = column![
            btn,
            text(status)
                .size(12)
                .width(Length::Fill)
                .style(status_style)
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

    pub(crate) fn flash_parts_loader_step(&self) -> Element<'_, Message> {
        self.loader_picker_card(
            &self.flash_parts.loader_path,
            &self.flash_parts.scan_error,
            Message::FlashParts(FlashPartsMsg::FlashPartsSelectLoader),
            |p| Message::FlashParts(FlashPartsMsg::FlashPartsLoaderChosen(Some(p))),
        )
    }

    /// Shared frame for the partition / physical-storage select tables.
    fn select_step_frame<'a>(
        &'a self,
        list: iced::widget::Column<'a, Message>,
    ) -> Element<'a, Message> {
        let scrolled = scrollable(list)
            .style(m3_scrollable_style)
            .height(Length::Fill)
            .width(Length::Fill);
        let col = column![scrolled,]
            .spacing(10)
            .padding(20)
            .width(Length::Fill)
            .align_x(iced::Alignment::Center);
        container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(crate) fn flash_parts_select_step(&self) -> Element<'_, Message> {
        let active = self.flash_parts.sort_col;
        let desc = self.flash_parts.sort_desc;
        let mk_msg = |c: PartsSortColumn| Message::FlashParts(FlashPartsMsg::FlashPartsSortBy(c));
        let header = row![
            text(" ").size(11).width(FLASH_PARTS_MARKER_CELL_WIDTH), // checkbox col
            parts_sort_header(
                self.t("flash_parts_col_lun").to_string(),
                active == PartsSortColumn::Lun,
                desc,
                Length::Fixed(50.0),
                mk_msg(PartsSortColumn::Lun),
            ),
            parts_sort_header(
                self.t("flash_parts_col_label").to_string(),
                active == PartsSortColumn::Label,
                desc,
                Length::FillPortion(3),
                mk_msg(PartsSortColumn::Label),
            ),
            parts_sort_header(
                self.t("flash_parts_col_start").to_string(),
                active == PartsSortColumn::Start,
                desc,
                Length::FillPortion(2),
                mk_msg(PartsSortColumn::Start),
            ),
            parts_sort_header(
                self.t("dump_parts_col_size").to_string(),
                active == PartsSortColumn::Size,
                desc,
                Length::FillPortion(2),
                mk_msg(PartsSortColumn::Size),
            ),
            parts_sort_header(
                self.t("flash_parts_col_file").to_string(),
                active == PartsSortColumn::File,
                desc,
                Length::FillPortion(3),
                mk_msg(PartsSortColumn::File),
            ),
        ]
        .spacing(8)
        .padding([6, 10])
        .align_y(iced::Alignment::Center);

        let mut list = column![header, widget::rule::horizontal(1)].spacing(0);
        for (idx, r) in self.flash_parts.rows.iter().enumerate() {
            // Fixed-width tri-state marker: skip, flash, or erase.
            let marker: Element<'_, Message> = match r.state {
                FlashRowState::Unchecked => iced::widget::checkbox(false)
                    .size(FLASH_PARTS_MARKER_SIZE)
                    .on_toggle(move |_| {
                        Message::FlashParts(FlashPartsMsg::FlashPartsToggleRow(idx))
                    })
                    .style(m3_checkbox_style)
                    .into(),
                FlashRowState::Flash => iced::widget::checkbox(true)
                    .size(FLASH_PARTS_MARKER_SIZE)
                    .on_toggle(move |_| {
                        Message::FlashParts(FlashPartsMsg::FlashPartsToggleRow(idx))
                    })
                    .style(m3_checkbox_style)
                    .into(),
                FlashRowState::Erase => m3_erase_marker(),
            };
            let marker_btn = button(
                container(marker)
                    .width(FLASH_PARTS_MARKER_CELL_WIDTH)
                    .height(FLASH_PARTS_MARKER_CELL_HEIGHT)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .padding(0)
            .on_press(Message::FlashParts(FlashPartsMsg::FlashPartsToggleRow(idx)))
            .style(|_t: &Theme, _s| button::Style {
                background: None,
                ..Default::default()
            });

            // Filename column: short display only.
            let file_disp = r
                .file_path
                .as_ref()
                .map(|p| {
                    std::path::Path::new(p)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| p.clone())
                })
                .unwrap_or_default();

            let data_row = iced::widget::row![
                container(marker_btn).width(FLASH_PARTS_MARKER_CELL_WIDTH),
                text(r.lun.to_string()).size(12).width(50),
                text(r.label.clone()).size(12).width(Length::FillPortion(3)),
                text(r.start_sector.to_string())
                    .size(12)
                    .width(Length::FillPortion(2)),
                text(format_bytes_auto(r.size_bytes))
                    .size(12)
                    .width(Length::FillPortion(2)),
                text(file_disp).size(12).width(Length::FillPortion(3)),
            ]
            .spacing(8)
            .padding([4, 10])
            .align_y(iced::Alignment::Center);

            // Tint the whole row by its tri-state so flash/erase pop
            // visually; light/dark both pull from the M3 container roles.
            let row_state = r.state;
            let tinted = container(data_row).width(Length::Fill).style(
                move |t: &Theme| -> container::Style {
                    let p = pal_of(t);
                    let bg = match row_state {
                        FlashRowState::Flash => Some(p.primary_container),
                        FlashRowState::Erase => Some(p.error_container),
                        FlashRowState::Unchecked => None,
                    };
                    container::Style {
                        background: bg.map(iced::Background::Color),
                        ..Default::default()
                    }
                },
            );

            // Whole row is a double-click target for the file picker.
            let clickable = iced::widget::mouse_area(tinted).on_double_click(Message::FlashParts(
                FlashPartsMsg::FlashPartsPickRowFile(idx),
            ));
            list = list.push(clickable);
        }

        self.select_step_frame(list)
    }

    pub(crate) fn flash_parts_confirm_step(&self) -> Element<'_, Message> {
        let rows = self.flash_parts.active_rows();
        let erase_rows: Vec<&FlashPartRow> = rows
            .iter()
            .filter(|r| r.state == FlashRowState::Erase)
            .collect();
        let flash_rows: Vec<&FlashPartRow> = rows
            .iter()
            .filter(|r| r.state == FlashRowState::Flash)
            .collect();

        let mut leading: Vec<Element<'_, Message>> = Vec::new();

        // ERASE block first, error-toned and loud.
        if !erase_rows.is_empty() {
            let mut erase_col = column![
                text(self.t("flash_parts_confirm_erase_warn").to_string())
                    .size(14)
                    .style(|t: &Theme| iced::widget::text::Style {
                        color: Some(pal_of(t).error),
                    })
            ]
            .spacing(4);
            for r in &erase_rows {
                erase_col = erase_col.push(
                    text(format!(
                        "⛔ {} (LUN {}, {})",
                        r.label,
                        r.lun,
                        format_bytes_auto(r.size_bytes)
                    ))
                    .size(13)
                    .style(|t: &Theme| iced::widget::text::Style {
                        color: Some(pal_of(t).error),
                    }),
                );
            }
            leading.push(
                container(erase_col)
                    .padding(14)
                    .width(Length::Fill)
                    .style(move |t: &Theme| container::Style {
                        background: Some(iced::Background::Color(pal_of(t).error_container)),
                        border: iced::Border {
                            color: pal_of(t).error,
                            width: 1.0,
                            radius: theme::shape::SM.into(),
                        },
                        text_color: Some(pal_of(t).on_error_container),
                        ..Default::default()
                    })
                    .into(),
            );
        }

        // FLASH block.
        if !flash_rows.is_empty() {
            let mut flash_col = column![
                text(self.t("flash_parts_confirm_flash_hdr").to_string())
                    .size(14)
                    .style(on_surface_style)
            ]
            .spacing(4);
            for r in &flash_rows {
                let fname = r
                    .file_path
                    .as_ref()
                    .map(|p| {
                        std::path::Path::new(p)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| p.clone())
                    })
                    .unwrap_or_default();
                flash_col = flash_col.push(
                    text(format!("• {} (LUN {}) ← {}", r.label, r.lun, fname))
                        .size(12)
                        .style(muted_style),
                );
            }
            leading.push(container(flash_col).padding(14).width(Length::Fill).into());
        }

        self.confirm_step_frame(leading, vec![], vec![])
    }

    pub(crate) fn view_dump_parts_wizard(&self) -> Element<'_, Message> {
        if self.log_popup_open && self.dump_parts.step >= 2 {
            return self.log_popup_view();
        }

        let step_labels: Vec<&str> = DUMP_PARTS_STEPS.iter().map(|k| self.t(k)).collect();
        let step_bar = wizard_step_bar(&step_labels, self.dump_parts.step);

        let body: Element<'_, Message> = match self.dump_parts.step {
            0 => self.dump_parts_loader_step(),
            1 => self.dump_parts_select_step(),
            _ => self.exec_step_view(),
        };

        let nav = if self.dump_parts.step < 2 {
            let is_dump_step = self.dump_parts.step == 1;
            let label = if is_dump_step {
                self.t("btn_dump").to_string()
            } else {
                self.t("btn_scan").to_string()
            };
            // DumpParts touches EDL on both Scan (step 0) and Dump
            // (step 1) — both spawn workers that talk to the device.
            // Gate both buttons on reachability.
            let can = self.dump_parts.can_next() && !self.busy && self.device_reachable();
            let leading_action = if self.dump_parts.step == 1 {
                partition_table_leading_action(self.dump_parts.entry_connection)
            } else {
                WizardLeadingAction::Back
            };
            let leading_label = if leading_action == WizardLeadingAction::Cancel {
                self.t("btn_cancel")
            } else {
                self.t("btn_back")
            };
            wizard_nav_generic_with_leading_action(
                leading_action,
                &label,
                can,
                leading_label,
                if self.dump_parts.step == 0 {
                    Message::DumpParts(DumpPartsMsg::DumpPartsClose)
                } else {
                    Message::DumpParts(DumpPartsMsg::DumpPartsBack)
                },
                Message::DumpParts(DumpPartsMsg::DumpPartsNext),
            )
        } else {
            empty_wizard_nav()
        };

        let mut layout = column![].width(Length::Fill).height(Length::Fill);
        if let Some(header) = self.dump_parts_action_bar() {
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

    fn dump_parts_action_bar(&self) -> Option<Element<'_, Message>> {
        let (title, subtitle) = match self.dump_parts.step {
            0 => (
                self.t("edl_loader_title").to_string(),
                self.t("edl_loader_subtitle").to_string(),
            ),
            1 => (
                self.t("dump_parts_select_title").to_string(),
                self.t("dump_parts_select_subtitle").to_string(),
            ),
            _ => return Some(self.exec_action_bar()),
        };
        Some(wizard_action_bar(title, Some(subtitle)))
    }

    pub(crate) fn dump_parts_loader_step(&self) -> Element<'_, Message> {
        self.loader_picker_card(
            &self.dump_parts.loader_path,
            &self.dump_parts.scan_error,
            Message::DumpParts(DumpPartsMsg::DumpPartsSelectLoader),
            |p| Message::DumpParts(DumpPartsMsg::DumpPartsLoaderChosen(Some(p))),
        )
    }

    pub(crate) fn dump_parts_select_step(&self) -> Element<'_, Message> {
        let active = self.dump_parts.sort_col;
        let desc = self.dump_parts.sort_desc;
        let mk_msg = |c: PartsSortColumn| Message::DumpParts(DumpPartsMsg::DumpPartsSortBy(c));
        // Header select-all: checked iff every row is selected (and there
        // is at least one row). Click flips toward whichever direction
        // would change state for the majority — full-select if any are
        // unchecked, else clear.
        let all_checked =
            !self.dump_parts.rows.is_empty() && self.dump_parts.rows.iter().all(|r| r.selected);
        let header_cb = iced::widget::checkbox(all_checked)
            .style(m3_checkbox_style)
            .on_toggle(|_| Message::DumpParts(DumpPartsMsg::DumpPartsToggleAll));
        let header = row![
            container(header_cb).width(32),
            parts_sort_header(
                self.t("flash_parts_col_lun").to_string(),
                active == PartsSortColumn::Lun,
                desc,
                Length::Fixed(50.0),
                mk_msg(PartsSortColumn::Lun),
            ),
            parts_sort_header(
                self.t("flash_parts_col_label").to_string(),
                active == PartsSortColumn::Label,
                desc,
                Length::FillPortion(3),
                mk_msg(PartsSortColumn::Label),
            ),
            parts_sort_header(
                self.t("flash_parts_col_start").to_string(),
                active == PartsSortColumn::Start,
                desc,
                Length::FillPortion(2),
                mk_msg(PartsSortColumn::Start),
            ),
            parts_sort_header(
                self.t("dump_parts_col_size").to_string(),
                active == PartsSortColumn::Size,
                desc,
                Length::FillPortion(2),
                mk_msg(PartsSortColumn::Size),
            ),
        ]
        .spacing(8)
        .padding([6, 10])
        .align_y(iced::Alignment::Center);

        let mut list = column![header, widget::rule::horizontal(1)].spacing(0);
        for (idx, row) in self.dump_parts.rows.iter().enumerate() {
            let cb = iced::widget::checkbox(row.selected)
                .style(m3_checkbox_style)
                .on_toggle(move |_| Message::DumpParts(DumpPartsMsg::DumpPartsToggleRow(idx)));
            let data_row = iced::widget::row![
                container(cb).width(32),
                text(row.lun.to_string()).size(12).width(50),
                text(row.label.clone())
                    .size(12)
                    .width(Length::FillPortion(3)),
                text(row.start_sector.to_string())
                    .size(12)
                    .width(Length::FillPortion(2)),
                text(format_bytes_auto(row.size_bytes))
                    .size(12)
                    .width(Length::FillPortion(2)),
            ]
            .spacing(8)
            .padding([4, 10])
            .align_y(iced::Alignment::Center);
            // Tint selected rows so the dump set is visible at a glance.
            let selected = row.selected;
            let tinted = container(data_row).width(Length::Fill).style(
                move |t: &Theme| -> container::Style {
                    let p = pal_of(t);
                    container::Style {
                        background: if selected {
                            Some(iced::Background::Color(p.primary_container))
                        } else {
                            None
                        },
                        ..Default::default()
                    }
                },
            );
            list = list.push(tinted);
        }

        self.select_step_frame(list)
    }

    pub(crate) fn view_dump_phys_wizard(&self) -> Element<'_, Message> {
        if self.log_popup_open && self.dump_phys.step >= 2 {
            return self.log_popup_view();
        }

        let step_labels: Vec<&str> = DUMP_PHYS_STEPS.iter().map(|k| self.t(k)).collect();
        let step_bar = wizard_step_bar(&step_labels, self.dump_phys.step);

        let body: Element<'_, Message> = match self.dump_phys.step {
            0 => self.dump_phys_loader_step(),
            1 => self.dump_phys_select_step(),
            _ => self.exec_step_view(),
        };

        let nav = if self.dump_phys.step < 2 {
            let is_dump_step = self.dump_phys.step == 1;
            let label = if is_dump_step {
                self.t("btn_dump").to_string()
            } else {
                self.t("btn_next").to_string()
            };
            // DumpPhys talks to EDL — gate both Scan + Dump on a
            // reachable device.
            let can = self.dump_phys.can_next() && !self.busy && self.device_reachable();
            wizard_nav_generic(
                true,
                &label,
                can,
                self.t("btn_back"),
                if self.dump_phys.step == 0 {
                    Message::DumpPhys(DumpPhysMsg::DumpPhysClose)
                } else {
                    Message::DumpPhys(DumpPhysMsg::DumpPhysBack)
                },
                Message::DumpPhys(DumpPhysMsg::DumpPhysNext),
            )
        } else {
            empty_wizard_nav()
        };

        let mut layout = column![].width(Length::Fill).height(Length::Fill);
        if let Some(header) = self.dump_phys_action_bar() {
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

    fn dump_phys_action_bar(&self) -> Option<Element<'_, Message>> {
        let (title, subtitle) = match self.dump_phys.step {
            0 => (
                self.t("edl_loader_title").to_string(),
                self.t("edl_loader_subtitle").to_string(),
            ),
            1 => (
                self.t("phys_select_title").to_string(),
                self.t("phys_select_subtitle").to_string(),
            ),
            _ => return Some(self.exec_action_bar()),
        };
        Some(wizard_action_bar(title, Some(subtitle)))
    }

    pub(crate) fn dump_phys_loader_step(&self) -> Element<'_, Message> {
        self.loader_picker_card(
            &self.dump_phys.loader_path,
            &self.dump_phys.loader_error,
            Message::DumpPhys(DumpPhysMsg::DumpPhysSelectLoader),
            |p| Message::DumpPhys(DumpPhysMsg::DumpPhysLoaderChosen(Some(p))),
        )
    }

    pub(crate) fn dump_phys_select_step(&self) -> Element<'_, Message> {
        let header = row![
            text(" ").size(11).width(32),
            text(self.t("phys_col_storage").to_string())
                .size(11)
                .width(Length::Fill)
                .style(muted_style),
        ]
        .spacing(8)
        .padding([6, 10])
        .align_y(iced::Alignment::Center);

        let mut list = column![header, widget::rule::horizontal(1)].spacing(0);
        for idx in 0..PHYS_LUN_COUNT {
            let checked = self.dump_phys.selected[idx];
            let cb = iced::widget::checkbox(checked)
                .style(m3_checkbox_style)
                .on_toggle(move |_| Message::DumpPhys(DumpPhysMsg::DumpPhysToggleRow(idx)));
            let data_row = iced::widget::row![
                container(cb).width(32),
                text(format!("LUN {idx}")).size(12).width(Length::Fill),
            ]
            .spacing(8)
            .padding([4, 10])
            .align_y(iced::Alignment::Center);
            list = list.push(data_row);
        }

        self.select_step_frame(list)
    }

    pub(crate) fn view_flash_phys_wizard(&self) -> Element<'_, Message> {
        if self.log_popup_open && self.flash_phys.step >= 3 {
            return self.log_popup_view();
        }

        let step_labels: Vec<&str> = FLASH_PHYS_STEPS.iter().map(|k| self.t(k)).collect();
        let step_bar = wizard_step_bar(&step_labels, self.flash_phys.step);

        let body: Element<'_, Message> = match self.flash_phys.step {
            0 => self.flash_phys_loader_step(),
            1 => self.flash_phys_select_step(),
            2 => self.flash_phys_confirm_step(),
            _ => self.exec_step_view(),
        };

        let nav = if self.flash_phys.step < 3 {
            let label = match self.flash_phys.step {
                0 => self.t("btn_next").to_string(),
                1 => self.t("btn_next").to_string(),
                2 => self.t("btn_start").to_string(),
                _ => self.t("btn_next").to_string(),
            };
            let is_start = self.flash_phys.step == 2;
            // No loader-fit gate here: by the Confirm step the device is already
            // in EDL where the model can't be polled, and the loader was already
            // used to open the session.
            let can = self.flash_phys.can_next()
                && !(self.busy && is_start)
                && (!is_start || self.device_reachable());
            wizard_nav_generic(
                true,
                &label,
                can,
                self.t("btn_back"),
                if self.flash_phys.step == 0 {
                    Message::FlashPhys(FlashPhysMsg::FlashPhysClose)
                } else {
                    Message::FlashPhys(FlashPhysMsg::FlashPhysBack)
                },
                Message::FlashPhys(FlashPhysMsg::FlashPhysNext),
            )
        } else {
            empty_wizard_nav()
        };

        let mut layout = column![].width(Length::Fill).height(Length::Fill);
        if let Some(header) = self.flash_phys_action_bar() {
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

    fn flash_phys_action_bar(&self) -> Option<Element<'_, Message>> {
        let (title, subtitle) = match self.flash_phys.step {
            0 => (
                self.t("edl_loader_title").to_string(),
                self.t("edl_loader_subtitle").to_string(),
            ),
            1 => (
                self.t("phys_select_title").to_string(),
                self.t("flash_phys_select_subtitle").to_string(),
            ),
            2 => (
                self.t("flash_parts_confirm_title").to_string(),
                self.t("flash_phys_confirm_subtitle").to_string(),
            ),
            _ => return Some(self.exec_action_bar()),
        };
        Some(wizard_action_bar(title, Some(subtitle)))
    }

    pub(crate) fn flash_phys_loader_step(&self) -> Element<'_, Message> {
        self.loader_picker_card(
            &self.flash_phys.loader_path,
            &self.flash_phys.loader_error,
            Message::FlashPhys(FlashPhysMsg::FlashPhysSelectLoader),
            |p| Message::FlashPhys(FlashPhysMsg::FlashPhysLoaderChosen(Some(p))),
        )
    }

    pub(crate) fn flash_phys_select_step(&self) -> Element<'_, Message> {
        let header = row![
            text(" ").size(11).width(32),
            text(self.t("phys_col_storage").to_string())
                .size(11)
                .width(Length::FillPortion(2))
                .style(muted_style),
            text(self.t("flash_parts_col_file").to_string())
                .size(11)
                .width(Length::FillPortion(3))
                .style(muted_style),
        ]
        .spacing(8)
        .padding([6, 10])
        .align_y(iced::Alignment::Center);

        let mut list = column![header, widget::rule::horizontal(1)].spacing(0);
        for idx in 0..PHYS_LUN_COUNT {
            let checked = self.flash_phys.selected[idx];
            let cb = iced::widget::checkbox(checked)
                .style(m3_checkbox_style)
                .on_toggle(move |_| Message::FlashPhys(FlashPhysMsg::FlashPhysToggleRow(idx)));

            let file_disp = self.flash_phys.file_paths[idx]
                .as_ref()
                .map(|p| {
                    std::path::Path::new(p)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| p.clone())
                })
                .unwrap_or_default();

            let data_row = iced::widget::row![
                container(cb).width(32),
                text(format!("LUN {idx}"))
                    .size(12)
                    .width(Length::FillPortion(2)),
                text(file_disp).size(12).width(Length::FillPortion(3)),
            ]
            .spacing(8)
            .padding([4, 10])
            .align_y(iced::Alignment::Center);

            let clickable = iced::widget::mouse_area(data_row)
                .on_double_click(Message::FlashPhys(FlashPhysMsg::FlashPhysPickRowFile(idx)));
            list = list.push(clickable);
        }

        self.select_step_frame(list)
    }

    pub(crate) fn flash_phys_confirm_step(&self) -> Element<'_, Message> {
        let pairs = self.flash_phys.active_pairs();

        let mut leading: Vec<Element<'_, Message>> = Vec::new();

        if !pairs.is_empty() {
            let mut list = column![
                text(self.t("flash_parts_confirm_flash_hdr").to_string())
                    .size(14)
                    .style(on_surface_style)
            ]
            .spacing(4);
            for (lun, path) in &pairs {
                let fname = std::path::Path::new(path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.clone());
                list = list.push(
                    text(format!("• LUN {lun} ← {fname}"))
                        .size(12)
                        .style(muted_style),
                );
            }
            leading.push(container(list).padding(14).width(Length::Fill).into());
        }

        self.confirm_step_frame(leading, vec![], vec![])
    }
}
