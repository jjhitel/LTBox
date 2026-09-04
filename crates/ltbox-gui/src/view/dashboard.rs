//! Dashboard view (device status, action tiles). Extracted from `main.rs`.

use crate::*;
use iced::widget::{Space, button, column, container, row, text};
use iced::{Element, Length, Theme};

impl App {
    /// Two-item dropdown under the firmware version: QFIL flash-tool package
    /// or OTA update — the device's two distinct firmware sources.
    fn firmware_menu(&self) -> Element<'_, Message> {
        // Anchored to the firmware cell, so it follows the card's density
        // rather than a dialog's.
        let d = self.density();
        let item = move |label: String, msg: Message| -> Element<'_, Message> {
            button(text(label).size(d.text(13.0)).width(Length::Fill))
                .on_press(msg)
                .padding(d.padding(8.0, 14.0))
                .width(Length::Fill)
                .style(dash_clickable_btn_style)
                .into()
        };
        container(
            column![
                item(self.t("firmware_menu_qfil").to_string(), Message::QfilOpen),
                item(self.t("firmware_menu_ota").to_string(), Message::OtaOpen),
            ]
            .spacing(2),
        )
        .padding(d.space(4.0))
        .width(Length::Fixed(d.width(200.0)))
        .style(|t: &Theme| {
            let p = pal_of(t);
            container::Style {
                background: Some(p.surface_container_high.into()),
                border: iced::Border {
                    color: p.outline_variant,
                    width: 1.0,
                    radius: theme::shape::SM.into(),
                },
                shadow: theme::elevation(2, theme::is_dark(t)),
                ..Default::default()
            }
        })
        .into()
    }

    pub(crate) fn view_dashboard(&self) -> Element<'_, Message> {
        let d = self.density();
        let model = if self.device_model.is_empty() {
            "—"
        } else {
            &self.device_model
        };
        let slot = if self.device_slot.is_empty() {
            "—"
        } else {
            &self.device_slot
        };
        let firmware = if self.device_firmware.is_empty() {
            "—"
        } else {
            &self.device_firmware
        };
        // i18n key (`arb_*`) or numeric from fastboot vars; translation
        // layer passes numerics through.
        let arb_raw = self.device_arb.clone();
        let arb_display = if arb_raw.is_empty() {
            "—".to_string()
        } else if arb_raw.starts_with("arb_") {
            self.t(&arb_raw).to_string()
        } else {
            arb_raw
        };
        let arb = arb_display.as_str();
        let ram = if self.device_ram.is_empty() {
            "—"
        } else {
            &self.device_ram
        };
        let storage = if self.device_storage.is_empty() {
            "—"
        } else {
            &self.device_storage
        };
        let op_text: Element<'_, Message> = if self.busy {
            let base = self.t("dash_operation_in_progress").to_string();
            let label = format!("{} - {base}", self.busy_operation_label());
            text(label).size(d.text(13.0)).style(accent_style).into()
        } else {
            text(self.t("dash_no_operation").to_string())
                .size(d.text(13.0))
                .style(muted_style)
                .into()
        };
        let can_resume = busy_navigation_target(self.busy, self.busy_view).is_some();

        // Title + divider dropped — sidebar already labels the active view,
        // so the duplicate header was eating vertical space without telling
        // the user anything new. `height(Fill)` so the log card (the last
        // child) can claim the remaining vertical space — keeps the top +
        // bottom dashboard margins symmetric.
        let mut content = column![]
            .spacing(d.space(14.0))
            .width(Length::Fill)
            .height(Length::Fill);

        // Unauthorized ADB wins over the platform warning — empty
        // `ro.boot.hardware` otherwise reads as "unsupported platform".
        if self.connection == ConnectionStatus::AdbServerBlocking {
            let msg = text(self.t("dash_adb_server_blocking").to_string())
                .size(d.text(theme::text_size::BODY_SMALL))
                .style(warning_container_text_style)
                .width(Length::Fill);
            let kill_btn = button(
                text(self.t("btn_kill_adb_server").to_string())
                    .size(d.text(theme::text_size::LABEL_LARGE))
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .on_press(Message::KillAdbServer)
            .padding(d.padding(10.0, 18.0))
            .height(Length::Fixed(d.size(40.0)))
            .style(banner_filled_btn_style);
            content = content.push(
                self.warning_banner(
                    row![msg, kill_btn]
                        .spacing(d.space(12.0))
                        .width(Length::Fill)
                        .align_y(iced::Alignment::Center),
                ),
            );
        } else if self.connection == ConnectionStatus::AdbUnauthorized {
            content = content.push(
                self.warning_banner(
                    text(self.t("dash_adb_unauthorized").to_string())
                        .size(d.text(theme::text_size::BODY_SMALL))
                        .style(warning_container_text_style)
                        .width(Length::Fill),
                ),
            );
        } else if self.platform_supported == Some(false) {
            content = content.push(
                self.warning_banner(
                    text(self.t("dash_unsupported_platform").to_string())
                        .size(d.text(theme::text_size::BODY_SMALL))
                        .style(warning_container_text_style)
                        .width(Length::Fill),
                ),
            );
        }

        if let Some(banner) = self.driver_install_banner() {
            content = content.push(banner);
        }

        let mut device_col = column![].spacing(0).width(Length::Fill);
        device_col = device_col.push(
            text(self.t("dash_device").to_string())
                .size(d.text(theme::text_size::TITLE_SMALL))
                .font(theme::emphasis::medium())
                .style(muted_style)
                .line_height(1.0),
        );
        device_col = device_col.push(Space::new().height(4));
        if !self.device_market_name.is_empty() {
            // Top of the card's hierarchy, but only just: `TITLE_LARGE` +
            // bold made a tablet model name shout over a card that is
            // mostly reference data. It keeps its original size and earns
            // the rank from `medium` weight plus the step down to
            // `BODY_MEDIUM` on the kv values below.
            device_col = device_col.push(
                text(self.device_market_name.clone())
                    .size(d.text(theme::text_size::TITLE_MEDIUM))
                    .font(theme::emphasis::medium())
                    .line_height(1.0),
            );
        }
        device_col = device_col.push(Space::new().height(d.space(12.0)));
        device_col = device_col.push(
            row![
                info_kv(d, self.t("device_model"), model),
                info_kv(d, self.t("device_ram"), ram),
                info_kv(d, self.t("device_storage"), storage),
                info_kv(d, self.t("device_slot"), slot),
            ]
            .spacing(d.space(40.0)),
        );
        device_col = device_col.push(Space::new().height(d.space(6.0)));
        // Firmware kv is clickable when a firmware id is populated —
        // tap to fetch the matching Lenovo OTA update payload. Wrap in
        // a `button` with `dash_clickable_btn_style` so the cell stays
        // flush with the card at rest but tints on hover, making the
        // click affordance visible. The previous `mouse_area` only set
        // the cursor — users on a stable pointer (touchpad tap) had no
        // visual cue.
        // Firmware kv uses vertical-only padding so the clickable
        // hover bg still has breathing room top/bottom around the
        // label, but the label's left edge stays at the cell's x=0
        // — keeping it column-aligned with the row above (모델 / RAM
        // / 저장소 / 슬롯). Horizontal hover padding would push the
        // label right and break that alignment.
        let firmware_kv: Element<'_, Message> = if self.device_firmware.is_empty() {
            info_kv(d, self.t("device_firmware"), firmware)
        } else {
            // Clicking firmware opens a small dropdown offering the QFIL
            // flash-tool package or the OTA update — the two distinct
            // firmware sources for this device.
            let anchor = button(info_kv(d, self.t("device_firmware"), firmware))
                .on_press(Message::FirmwareMenu(!self.firmware_menu_open))
                .padding([4, 0])
                .style(dash_clickable_btn_style);
            iced_aw::DropDown::new(anchor, self.firmware_menu(), self.firmware_menu_open)
                .on_dismiss(Message::FirmwareMenu(false))
                .alignment(iced_aw::drop_down::Alignment::Bottom)
                .into()
        };
        // Row align_y(Center) handles the vertical mismatch: the
        // firmware button is `4 + label + 4` tall while the bare ARB
        // kv is just `label` tall — centering both within the row's
        // max height puts the two labels at the same y without
        // introducing any horizontal offset on the ARB cell.
        // The cell always answers yes/no by model, on every transport. In
        // bootloader mode the device additionally reports its committed
        // floors, and the cell becomes a click target for the breakdown —
        // same hover-tint affordance as the firmware cell beside it.
        let arb_kv: Element<'_, Message> = if self.rollback_detail_available() {
            iced::widget::tooltip(
                button(info_kv(d, self.t("device_arb"), arb))
                    .on_press(Message::RollbackDetailOpen)
                    .padding([4, 0])
                    .style(dash_clickable_btn_style),
                container(text(self.t("rollback_open_tip").to_string()).size(11))
                    .padding([6, 10])
                    .style(|t: &Theme| theme::tooltip_style(t, theme::shape::SM)),
                iced::widget::tooltip::Position::Top,
            )
            .into()
        } else {
            info_kv(d, self.t("device_arb"), arb)
        };
        device_col = device_col.push(
            row![arb_kv, firmware_kv,]
                .spacing(d.space(40.0))
                .align_y(iced::Alignment::Center),
        );

        // Pin the inner row to 160 px regardless of whether the device is
        // populated. Without this the empty-state card collapses to the
        // text column's natural height, then jumps taller once a device
        // connects — same card, two different sizes. The portrait branch
        // already used `height(160)`; the empty branch now matches so the
        // dashboard layout doesn't reflow on connect.
        let card_height = d.size(DEVICE_CARD_HEIGHT);
        let device_card_inner: Element<'_, Message> = if self.device_model.is_empty() {
            container(device_col)
                .width(Length::Fill)
                .height(Length::Fixed(card_height))
                .into()
        } else {
            let portrait: Element<'_, Message> = match device_portrait(&self.device_model) {
                DevicePortrait::Png(h) => iced::widget::image(h)
                    .height(Length::Fill)
                    .content_fit(iced::ContentFit::ScaleDown)
                    .into(),
                DevicePortrait::Svg(h) => iced::widget::svg(h)
                    .height(Length::Fill)
                    .content_fit(iced::ContentFit::ScaleDown)
                    .into(),
            };
            // Click on the portrait fires the Lenovo PTSTPD lookup popup.
            // Skip when no serial was captured (e.g. EDL connection) so
            // the click is a clear no-op rather than triggering an empty
            // upstream query.
            let portrait_w = d.image(220.0);
            let portrait_box = container(portrait)
                .width(Length::Fixed(portrait_w))
                .height(Length::Fill)
                .center_x(Length::Fixed(portrait_w))
                .center_y(Length::Fill);
            let portrait_clickable: Element<'_, Message> = if self.device_serial.is_empty() {
                portrait_box.into()
            } else {
                // Same hover-tint pattern as the firmware kv so both
                // dashboard click targets look identically interactive.
                button(portrait_box)
                    .on_press(Message::DeviceInfoOpen)
                    .padding(0)
                    .style(dash_clickable_btn_style)
                    .into()
            };
            row![device_col, portrait_clickable,]
                .spacing(d.space(16.0))
                .align_y(iced::Alignment::Center)
                .height(Length::Fixed(card_height))
                .into()
        };
        content = content.push(
            container(
                // Padding has to scale with the corner radius: M3 states
                // the relationship as `outer radius - padding = inner
                // radius`, so the 10/18 inset that suited a 12 px corner
                // leaves text crowding the curve at 32 px. A uniform 24
                // keeps a comfortable 8 px inner radius and reads evenly
                // on all four sides, which the old asymmetric values did
                // not.
                container(device_card_inner)
                    .padding(d.space(DEVICE_CARD_PADDING))
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .style(|t: &Theme| {
                // LTBox's hero moment. M3 asks for one or two per product
                // and says to build them by combining tactics, so this
                // card stacks three: it breaks from the surrounding
                // `LG` shape story to a much rounder silhouette, takes
                // the brightest surface the mode offers, and sits a
                // step higher in elevation than its siblings.
                //
                // Deliberately no type escalation — the connected device
                // is the subject of the whole app, but the card is mostly
                // reference data and shouting it was already tried and
                // rejected.
                theme::surface_card_style(
                    t,
                    theme::SurfaceLevel::Brightest,
                    theme::shape::XL_INCREASED,
                    2,
                )
            }),
        );
        let operation_card = if can_resume {
            let open_action = row![
                text(self.t("dash_open_operation").to_string())
                    .size(d.text(12.0))
                    .style(accent_style),
                icon::fab_next().size(d.image(18.0)).style(accent_style),
            ]
            .spacing(d.space(4.0))
            .align_y(iced::Alignment::Center);
            clickable_card(
                d,
                self.t("dash_current_operation"),
                row![op_text, Space::new().width(Length::Fill), open_action]
                    .spacing(d.space(12.0))
                    .align_y(iced::Alignment::Center),
                Message::ResumeBusyOperation,
            )
        } else {
            card(d, self.t("dash_current_operation"), op_text)
        };
        content = content.push(operation_card);
        // Read-only text_editor so drag-select + Ctrl+C work. `Length::Fill`
        // height lets the M3 text field claim the remaining dashboard space
        // directly, without wrapping the log in a second card.
        let dash_log_editor: Element<'_, Message> = iced::widget::text_editor(&self.log_editor)
            .on_action(Message::LogEditorAction)
            .size(d.text(11.0))
            .height(Length::Fill)
            .padding(iced::Padding {
                top: 0.0,
                right: d.space(16.0),
                bottom: d.space(10.0),
                left: d.space(16.0),
            })
            .style(m3_log_text_editor_style)
            .into();
        content = content.push(m3_log_text_field(
            d,
            self.t("dash_log").to_string(),
            dash_log_editor,
        ));
        content.into()
    }
}
