//! Reboot view + confirm popup. Extracted from `main.rs`.

use crate::*;
use iced::widget::{self, Space, button, column, container, row, text};
use iced::{Element, Length, Theme};
use ltbox_core::tr_args;
use theme::with_alpha;

impl App {
    pub(crate) fn view_reboot(&self) -> Element<'_, Message> {
        let conn = self.connection;
        let icon_size = self.wizard_list_icon(32.0);
        let (label_size, desc_size) = self.wizard_list_text(18.0, 12.0);
        let row_height = self.wizard_list_row_height();
        // Disabled cards: M3 tokens (12% surface alpha, 38% text alpha).
        // Same inset as the root list so both single-column lists sit on the
        // same margins.
        let mut list = column![].spacing(10).padding([20, 28]).width(Length::Fill);
        for &target in RebootTarget::all().iter() {
            let available = target.available_from(conn);
            let label = self.t(target.label_key()).to_string();
            let desc = self.t(target.desc_key()).to_string();
            let label_style = if available {
                on_surface_style
            } else {
                |t: &Theme| iced::widget::text::Style {
                    color: Some(with_alpha(pal_of(t).on_surface, 0.38)),
                }
            };
            let desc_style = if available {
                muted_style
            } else {
                |t: &Theme| iced::widget::text::Style {
                    color: Some(with_alpha(pal_of(t).on_surface, 0.38)),
                }
            };
            // Empty desc → single label; non-empty keeps the stack.
            let label_col: Element<'_, Message> = if desc.is_empty() {
                text(label)
                    .size(label_size)
                    .style(label_style)
                    .width(Length::Fill)
                    .into()
            } else {
                column![
                    text(label).size(label_size).style(label_style),
                    text(desc).size(desc_size).style(desc_style),
                ]
                .spacing(6)
                .width(Length::Fill)
                .into()
            };
            let card_content = row![icon_tile(target.icon(icon_size)), label_col]
                .spacing(16)
                .align_y(iced::Alignment::Center);
            let card_inner = container(card_content)
                .padding([10, 16])
                .width(Length::Fill)
                .height(Length::Fixed(row_height))
                .center_y(Length::Fixed(row_height))
                .style(move |t: &Theme| {
                    let p = pal_of(t);
                    if available {
                        sel_card_style(t, false)
                    } else {
                        container::Style {
                            background: Some(with_alpha(p.on_surface, 0.12).into()),
                            border: iced::Border {
                                color: iced::Color::TRANSPARENT,
                                width: 0.0,
                                radius: theme::shape::LG.into(),
                            },
                            ..Default::default()
                        }
                    }
                });
            let btn: Element<'_, Message> = if available {
                button(card_inner)
                    .on_press(Message::Reboot(RebootMsg::RebootRequest(target)))
                    .padding(0)
                    .width(Length::Fill)
                    .style(|t: &Theme, status| sel_card_btn_style(t, status, false))
                    .into()
            } else {
                card_inner.into()
            };
            list = list.push(btn);
        }

        let body = container(centered_step(
            list,
            self.wizard_list_max_width(WIZARD_LIST_MAX_WIDTH),
        ))
        .padding(24)
        .width(Length::Fill)
        .height(Length::Fill);

        column![
            large_top_app_bar(
                self.t("reboot_title").to_string(),
                Some(self.t("reboot_subtitle").to_string()),
            ),
            body,
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// M3 confirm dialog for the Reboot panel.
    pub(crate) fn reboot_confirm_popup(&self, target: RebootTarget) -> Element<'_, Message> {
        let short = self.t(target.short_name_key()).to_string();
        let title = tr_args!("reboot_confirm_title", target = short);
        let body = tr_args!("reboot_confirm_body", target = short);
        let content = column![
            text(title).size(20),
            text(body).size(13).style(muted_style),
            widget::rule::horizontal(1),
            row![
                Space::new().width(Length::Fill),
                m3_text_button(self.t("btn_cancel").to_string())
                    .on_press(Message::Reboot(RebootMsg::RebootDismiss)),
                {
                    // Mid-popup disconnect → drop the on_press so the
                    // confirm button reads as disabled instead of
                    // firing a reboot worker on a vanished transport.
                    let mut b = m3_filled_button(self.t("btn_reboot_confirm").to_string());
                    if self.device_reachable() {
                        b = b.on_press(Message::Reboot(RebootMsg::RebootConfirm));
                    }
                    b
                },
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(14)
        .padding(24)
        .width(380);
        m3_dialog(content.into())
    }
}
