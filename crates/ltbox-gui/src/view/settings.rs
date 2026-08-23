//! Settings view (language, theme, default loader). Extracted from `main.rs`.

use crate::*;
use iced::widget::{self, Space, button, column, container, row, text};
use iced::{Element, Length, Theme};
use theme::{mix_color, with_alpha};

impl App {
    pub(crate) fn view_settings(&self) -> Element<'_, Message> {
        let s = &self.settings;
        let d = self.density();
        // One field padding for every control on the panel, so the pick lists
        // and the icon buttons keep the same relationship to their labels at
        // any window size.
        let field_padding = d.scale_padding(M3_FIELD_PADDING);
        let icon_btn = Length::Fixed(d.size(36.0));

        // Single untitled "Preferences" card holds language + theme.
        let lang_row = row![
            text(self.t("settings_language").to_string())
                .size(d.text(13.0))
                .width(Length::Fill),
            widget::pick_list(
                LANGUAGES.iter().map(|l| l.label()).collect::<Vec<_>>(),
                Some(s.language.label()),
                |selected| {
                    let l = LANGUAGES
                        .iter()
                        .find(|l| l.label() == selected)
                        .copied()
                        .unwrap_or(Language::En);
                    Message::Settings(SettingsMsg::SetLanguage(l))
                },
            )
            // Match the row label's 13 px size so the trigger button
            // doesn't tower over the "Language" label next to it. The
            // menu items inherit `text_size` for visual consistency
            // with the trigger.
            .text_size(d.text(13.0))
            // Forwarded to the dropdown items too (iced builds the menu
            // with the pick_list's padding), lifting both the trigger and
            // each option off the ~27 px they defaulted to.
            .padding(field_padding)
            .style(m3_pick_list_style)
            .menu_style(m3_pick_list_menu_style)
            .width(Length::Fixed(d.width(160.0))),
        ]
        .align_y(iced::Alignment::Center);

        let t_system = self.t(ThemeChoice::System.label_key()).to_string();
        let t_light = self.t(ThemeChoice::Light.label_key()).to_string();
        let t_dark = self.t(ThemeChoice::Dark.label_key()).to_string();
        let current_theme_label = match self.theme_choice {
            ThemeChoice::System => t_system.clone(),
            ThemeChoice::Light => t_light.clone(),
            ThemeChoice::Dark => t_dark.clone(),
        };
        let theme_options: Vec<String> = vec![t_system.clone(), t_light.clone(), t_dark.clone()];
        let theme_row = row![
            text(self.t("settings_theme").to_string())
                .size(d.text(13.0))
                .width(Length::Fill),
            widget::pick_list(theme_options, Some(current_theme_label), move |selected| {
                let choice = if selected == t_system {
                    ThemeChoice::System
                } else if selected == t_dark {
                    ThemeChoice::Dark
                } else {
                    ThemeChoice::Light
                };
                Message::SetTheme(choice)
            },)
            // Match the row label's 13 px size. Same rationale as the
            // language pick list.
            .text_size(d.text(13.0))
            // Forwarded to the dropdown items too (iced builds the menu
            // with the pick_list's padding), lifting both the trigger and
            // each option off the ~27 px they defaulted to.
            .padding(field_padding)
            .style(m3_pick_list_style)
            .menu_style(m3_pick_list_menu_style)
            .width(Length::Fixed(d.width(160.0))),
        ]
        .align_y(iced::Alignment::Center);

        let seed_indigo = self.t(ThemeSeed::Indigo.label_key()).to_string();
        let seed_teal = self.t(ThemeSeed::Teal.label_key()).to_string();
        let seed_rose = self.t(ThemeSeed::Rose.label_key()).to_string();
        let current_seed_label = match self.theme_seed {
            ThemeSeed::Indigo => seed_indigo.clone(),
            ThemeSeed::Teal => seed_teal.clone(),
            ThemeSeed::Rose => seed_rose.clone(),
        };
        let seed_options: Vec<String> =
            vec![seed_indigo.clone(), seed_teal.clone(), seed_rose.clone()];
        let seed_row = row![
            text(self.t("settings_theme_seed").to_string())
                .size(d.text(13.0))
                .width(Length::Fill),
            widget::pick_list(seed_options, Some(current_seed_label), move |selected| {
                let seed = if selected == seed_teal {
                    ThemeSeed::Teal
                } else if selected == seed_rose {
                    ThemeSeed::Rose
                } else {
                    ThemeSeed::Indigo
                };
                Message::Settings(SettingsMsg::SetThemeSeed(seed))
            },)
            .text_size(d.text(13.0))
            // Forwarded to the dropdown items too (iced builds the menu
            // with the pick_list's padding), lifting both the trigger and
            // each option off the ~27 px they defaulted to.
            .padding(field_padding)
            .style(m3_pick_list_style)
            .menu_style(m3_pick_list_menu_style)
            .width(Length::Fixed(d.width(160.0))),
        ]
        .align_y(iced::Alignment::Center);

        let driver_userspace = self.t("settings_qcom_driver_mode_userspace").to_string();
        let driver_kernel = self.t("settings_qcom_driver_mode_kernel").to_string();
        let current_driver_label = match self.qcom_driver_mode {
            ltbox_device::driver::QcomDriverMode::Userspace => driver_userspace.clone(),
            ltbox_device::driver::QcomDriverMode::Kernel => driver_kernel.clone(),
        };
        // Kernel mode is unusable on macOS and on non-Debian Linux (no
        // `dpkg-query`); there the picker is locked to userspace and the help
        // text explains why.
        let kernel_mode_supported = ltbox_device::driver::kernel_mode_supported();
        let driver_help_key = if cfg!(target_os = "macos") {
            "settings_qcom_driver_mode_macos"
        } else if !kernel_mode_supported {
            // Reachable only on non-Debian Linux — Windows and Debian Linux
            // support kernel mode, macOS is handled above.
            "settings_qcom_driver_mode_linux_unsupported"
        } else {
            "settings_qcom_driver_mode_help"
        };
        let driver_help_icon = widget::tooltip(
            container(text("?").size(d.text(11.0)).style(muted_style))
                .padding([2, 6])
                .style(|t: &Theme| {
                    let p = pal_of(t);
                    container::Style {
                        background: Some(with_alpha(p.on_surface_variant, 0.10).into()),
                        border: iced::Border {
                            radius: theme::shape::FULL.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }),
            container(text(self.t(driver_help_key).to_string()).size(d.text(11.0)))
                .padding([6, 10])
                .max_width(d.width(280.0))
                .style(|t: &Theme| theme::tooltip_style(t, theme::shape::SM)),
            widget::tooltip::Position::Right,
        );
        let driver_control: Element<'_, Message> = if self.busy || !kernel_mode_supported {
            // Same geometry as the pick_list this replaces, at M3's disabled
            // tones (38% content, 12% outline), so it reads as that control
            // unavailable rather than as a different kind of field.
            container(
                text(current_driver_label)
                    .size(d.text(13.0))
                    .style(|t: &Theme| iced::widget::text::Style {
                        color: Some(with_alpha(pal_of(t).on_surface, 0.38)),
                    }),
            )
            .padding(field_padding)
            .width(Length::Fixed(d.width(160.0)))
            .style(|t: &Theme| {
                let p = pal_of(t);
                container::Style {
                    background: None,
                    border: iced::Border {
                        color: with_alpha(p.on_surface, 0.12),
                        width: 1.0,
                        radius: theme::shape::SM.into(),
                    },
                    ..Default::default()
                }
            })
            .into()
        } else {
            let driver_kernel_for_pick = driver_kernel.clone();
            widget::pick_list(
                vec![driver_kernel, driver_userspace],
                Some(current_driver_label),
                move |selected| {
                    let mode = if selected == driver_kernel_for_pick {
                        ltbox_device::driver::QcomDriverMode::Kernel
                    } else {
                        ltbox_device::driver::QcomDriverMode::Userspace
                    };
                    Message::Settings(SettingsMsg::SetQcomDriverMode(mode))
                },
            )
            .text_size(d.text(13.0))
            // Forwarded to the dropdown items too (iced builds the menu
            // with the pick_list's padding), lifting both the trigger and
            // each option off the ~27 px they defaulted to.
            .padding(field_padding)
            .style(m3_pick_list_style)
            .menu_style(m3_pick_list_menu_style)
            .width(Length::Fixed(d.width(160.0)))
            .into()
        };
        let driver_label = row![
            text(self.t("settings_qcom_driver_mode").to_string()).size(d.text(13.0)),
            driver_help_icon,
        ]
        .spacing(d.space(6.0))
        .align_y(iced::Alignment::Center)
        .width(Length::Fill);
        let driver_row = row![driver_label, driver_control].align_y(iced::Alignment::Center);

        // Default EDL loader used to auto-fill loader pickers.
        let default_loader_help = self.t("settings_default_loader_help").to_string();
        let help_icon = widget::tooltip(
            container(text("?").size(d.text(11.0)).style(muted_style))
                .padding([2, 6])
                .style(|t: &Theme| {
                    let p = pal_of(t);
                    container::Style {
                        background: Some(with_alpha(p.on_surface_variant, 0.10).into()),
                        border: iced::Border {
                            radius: theme::shape::FULL.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }),
            container(text(default_loader_help).size(d.text(11.0)))
                .padding([6, 10])
                .max_width(d.width(280.0))
                .style(|t: &Theme| theme::tooltip_style(t, theme::shape::SM)),
            widget::tooltip::Position::Right,
        );

        // Icon-only actions keep tooltips for accessibility.
        let browse_btn = button(
            container(lucide_icon(
                icon::settings_browse(),
                d.image(18.0),
                |t: &Theme| pal_of(t).on_secondary_container,
            ))
            .width(icon_btn)
            .height(icon_btn)
            .center_x(icon_btn)
            .center_y(icon_btn),
        )
        .on_press(Message::Settings(SettingsMsg::SettingsPickDefaultLoader))
        .padding(0)
        .style(|t: &Theme, status| {
            let p = pal_of(t);
            let base = p.secondary_container;
            // iced's `button::Style::background` only accepts a single
            // color/gradient, so the M3 state-layer (semi-transparent
            // on_X over the tonal base) is pre-composited into one
            // opaque tint via `mix_color`.
            let bg = match status {
                button::Status::Hovered => {
                    mix_color(base, p.on_secondary_container, theme::state::HOVER)
                }
                button::Status::Pressed => {
                    mix_color(base, p.on_secondary_container, theme::state::PRESSED)
                }
                _ => base,
            };
            let bg = Some(bg.into());
            button::Style {
                background: bg,
                border: iced::Border {
                    radius: theme::shape::FULL.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        });
        let browse_tip = widget::tooltip(
            browse_btn,
            container(
                text(self.t("settings_default_loader_browse").to_string()).size(d.text(11.0)),
            )
            .padding([6, 10])
            .style(|t: &Theme| theme::tooltip_style(t, theme::shape::XS)),
            widget::tooltip::Position::Top,
        );

        let mut default_loader_actions = row![browse_tip,]
            .spacing(d.space(8.0))
            .align_y(iced::Alignment::Center);
        if self.default_loader_path.is_some() {
            let clear_btn = button(
                container(lucide_icon(
                    icon::settings_clear(),
                    d.image(18.0),
                    |t: &Theme| pal_of(t).on_error_container,
                ))
                .width(icon_btn)
                .height(icon_btn)
                .center_x(icon_btn)
                .center_y(icon_btn),
            )
            .on_press(Message::Settings(SettingsMsg::SettingsClearDefaultLoader))
            .padding(0)
            .style(|t: &Theme, status| {
                let p = pal_of(t);
                let base = p.error_container;
                let bg = match status {
                    button::Status::Hovered => {
                        Some(mix_color(base, p.on_error_container, theme::state::HOVER).into())
                    }
                    button::Status::Pressed => {
                        Some(mix_color(base, p.on_error_container, theme::state::PRESSED).into())
                    }
                    _ => Some(base.into()),
                };
                button::Style {
                    background: bg,
                    border: iced::Border {
                        radius: theme::shape::FULL.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            });
            let clear_tip = widget::tooltip(
                clear_btn,
                container(
                    text(self.t("settings_default_loader_clear").to_string()).size(d.text(11.0)),
                )
                .padding([6, 10])
                .style(|t: &Theme| theme::tooltip_style(t, theme::shape::XS)),
                widget::tooltip::Position::Top,
            );
            default_loader_actions = default_loader_actions.push(clear_tip);
        }

        let default_loader_top = row![
            text(self.t("settings_default_loader").to_string())
                .size(d.text(13.0))
                .line_height(1.0),
            help_icon,
            Space::new().width(Length::Fill),
            default_loader_actions,
        ]
        .spacing(d.space(8.0))
        .align_y(iced::Alignment::Center);

        let default_loader_path_str = self
            .default_loader_path
            .clone()
            .unwrap_or_else(|| self.t("settings_default_loader_unset").to_string());
        let default_loader_row = column![
            default_loader_top,
            text(default_loader_path_str)
                .size(d.text(11.0))
                .style(muted_style),
        ]
        .spacing(d.space(6.0));

        let prefs_card = container(
            column![lang_row, theme_row, seed_row,]
                .spacing(d.space(14.0))
                .padding(d.padding(14.0, 18.0))
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .style(|t: &Theme| {
            let p = pal_of(t);
            container::Style {
                background: Some(p.surface_container.into()),
                border: iced::Border {
                    color: p.outline_variant,
                    width: 1.0,
                    radius: theme::shape::LG.into(),
                },
                shadow: theme::elevation(1, theme::is_dark(t)),
                ..Default::default()
            }
        });
        let prefs_card: Element<'_, Message> =
            centered_max_width(prefs_card, d.width(SETTINGS_PANEL_MAX_WIDTH));

        // --- Maintenance card: clean leftover temp/scratch files ----------
        // Enabled only once a scan has found something to remove and no
        // device op is live (a live op owns the very dirs we'd delete).
        let cleanup_enabled =
            !self.busy && !self.cleaning_temp && matches!(self.temp_files_bytes, Some(b) if b > 0);
        let cleanup_help_icon = widget::tooltip(
            container(text("?").size(d.text(11.0)).style(muted_style))
                .padding([2, 6])
                .style(|t: &Theme| {
                    let p = pal_of(t);
                    container::Style {
                        background: Some(with_alpha(p.on_surface_variant, 0.10).into()),
                        border: iced::Border {
                            radius: theme::shape::FULL.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }),
            container(text(self.t("settings_cleanup_help").to_string()).size(d.text(11.0)))
                .padding([6, 10])
                .max_width(d.width(280.0))
                .style(|t: &Theme| theme::tooltip_style(t, theme::shape::SM)),
            widget::tooltip::Position::Right,
        );

        // Icon-only tonal action, mirroring the default-loader browse/clear
        // buttons: no label, the description surfaces on hover. Enabled only
        // when a scan found something removable and no op is live.
        let cleanup_tip_label = if self.cleaning_temp {
            self.t("settings_cleanup_busy").to_string()
        } else {
            self.t("settings_cleanup_button").to_string()
        };
        let mut cleanup_btn = button(
            container(lucide_icon(
                icon::settings_cleanup(),
                d.image(18.0),
                move |t: &Theme| {
                    let p = pal_of(t);
                    if cleanup_enabled {
                        p.on_secondary_container
                    } else {
                        with_alpha(p.on_surface, 0.38)
                    }
                },
            ))
            .width(icon_btn)
            .height(icon_btn)
            .center_x(icon_btn)
            .center_y(icon_btn),
        )
        .padding(0)
        .style(move |t: &Theme, status| {
            let p = pal_of(t);
            // Greyed M3 disabled affordance when there's nothing to clean.
            if !cleanup_enabled || matches!(status, button::Status::Disabled) {
                return button::Style {
                    background: Some(with_alpha(p.on_surface, 0.12).into()),
                    border: iced::Border {
                        radius: theme::shape::FULL.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                };
            }
            let base = p.secondary_container;
            let bg = match status {
                button::Status::Hovered => {
                    mix_color(base, p.on_secondary_container, theme::state::HOVER)
                }
                button::Status::Pressed => {
                    mix_color(base, p.on_secondary_container, theme::state::PRESSED)
                }
                _ => base,
            };
            button::Style {
                background: Some(bg.into()),
                border: iced::Border {
                    radius: theme::shape::FULL.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        });
        if cleanup_enabled {
            cleanup_btn = cleanup_btn.on_press(Message::Settings(SettingsMsg::CleanupTempFiles));
        }
        let cleanup_action = widget::tooltip(
            cleanup_btn,
            container(text(cleanup_tip_label).size(d.text(11.0)))
                .padding([6, 10])
                .style(|t: &Theme| theme::tooltip_style(t, theme::shape::XS)),
            widget::tooltip::Position::Top,
        );

        // Size readout sits in parens between the label and the help icon;
        // shown once a scan has landed. Explanation lives only in the tooltip.
        let cleanup_size: Element<'_, Message> = match self.temp_files_bytes {
            Some(bytes) => text(format!("({})", format_bytes_auto(bytes)))
                .size(d.text(13.0))
                .style(muted_style)
                .into(),
            None => Space::new().width(0).height(0).into(),
        };
        let cleanup_top = row![
            text(self.t("settings_cleanup").to_string())
                .size(d.text(13.0))
                .line_height(1.0),
            cleanup_size,
            cleanup_help_icon,
            Space::new().width(Length::Fill),
            cleanup_action,
        ]
        .spacing(d.space(8.0))
        .width(Length::Fill)
        .align_y(iced::Alignment::Center);

        // Device / maintenance card: Qualcomm driver + default loader sit
        // above, with the temp-file cleanup row kept at the very bottom.
        let cleanup_card = container(
            column![driver_row, default_loader_row, cleanup_top]
                .spacing(d.space(14.0))
                .padding(d.padding(14.0, 18.0))
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .style(|t: &Theme| {
            let p = pal_of(t);
            container::Style {
                background: Some(p.surface_container.into()),
                border: iced::Border {
                    color: p.outline_variant,
                    width: 1.0,
                    radius: theme::shape::LG.into(),
                },
                shadow: theme::elevation(1, theme::is_dark(t)),
                ..Default::default()
            }
        });
        let cleanup_card: Element<'_, Message> =
            centered_max_width(cleanup_card, d.width(SETTINGS_PANEL_MAX_WIDTH));

        let mut col = column![].spacing(d.space(14.0)).width(Length::Fill);
        // Surface the driver install / update banner here too, so switching the
        // driver mode above shows the prompt without a trip to the dashboard.
        if let Some(banner) = self.driver_install_banner() {
            col = col.push(centered_max_width(
                banner,
                d.width(SETTINGS_PANEL_MAX_WIDTH),
            ));
        }
        col = col.push(prefs_card);
        col = col.push(cleanup_card);

        let body = iced::widget::scrollable(container(col).padding(24).width(Length::Fill))
            .style(m3_scrollable_style)
            .width(Length::Fill)
            .height(Length::Fill);

        column![
            large_top_app_bar(
                self.t("settings_title").to_string(),
                Some(self.t("settings_subtitle").to_string()),
            ),
            body,
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
