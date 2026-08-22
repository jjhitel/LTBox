//! Reusable view components (dialogs, cards, step bar, icon tiles, lucide helpers). Extracted from `main.rs`.

use crate::*;
use iced::widget::{self, Space, button, column, container, responsive, row, text};
use iced::{Element, Length, Theme};
use theme::with_alpha;

/// Centered M3 dialog card on a scrim. Inner owns padding/width. MODAL: the
/// whole layer is wrapped in `opaque`, so it captures every pointer event and
/// nothing behind it reacts. Use for confirm dialogs (reboot, country,
/// region-target, rescue, root prompts) that must block the panel behind them.
pub(crate) fn m3_dialog(inner: Element<'_, Message>) -> Element<'_, Message> {
    // `opaque` makes the whole dialog layer capture every pointer event, so
    // hover/click can't fall through the parent `Stack` to the panel behind it
    // (the scrim alone only paints — it doesn't block). The card's own buttons
    // still receive their clicks.
    iced::widget::opaque(m3_dialog_layers(inner))
}

/// Like [`m3_dialog`] but MODELESS: no `opaque` wrapper, so pointer events fall
/// through the scrim to the panel behind. Use for the busy progress dialog — a
/// long-running flash must not trap the user; the sidebar (and current view)
/// stay clickable so they can navigate back to the op's progress screen.
pub(crate) fn m3_dialog_modeless(inner: Element<'_, Message>) -> Element<'_, Message> {
    m3_dialog_layers(inner)
}

pub(crate) fn m3_log_text_field<'a>(
    label: impl Into<String>,
    editor: Element<'a, Message>,
) -> Element<'a, Message> {
    // Titled like the other dashboard cards rather than like an M2 filled
    // text field. The old form put a `primary` caption at the top and a
    // 2 px `primary` active indicator at the very bottom — on a
    // full-height read-only log that indicator ended up as a stray blue
    // rule hundreds of pixels away from its label, marking "focus" on a
    // surface that is never focused.
    let label = label.into();
    let label_row = container(
        text(label)
            .size(theme::text_size::TITLE_SMALL)
            .font(theme::emphasis::medium())
            .line_height(1.0)
            .style(muted_style),
    )
    .padding(iced::Padding {
        top: 12.0,
        right: 18.0,
        bottom: 8.0,
        left: 18.0,
    })
    .width(Length::Fill);

    let field = column![
        label_row,
        container(editor).width(Length::Fill).height(Length::Fill),
    ]
    .spacing(0)
    .height(Length::Fill)
    .width(Length::Fill);

    container(field)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|t: &Theme| {
            theme::surface_card_style(t, theme::SurfaceLevel::Default, theme::shape::LG, 1)
        })
        .into()
}

/// Shared scrim + centered card layers behind [`m3_dialog`] /
/// [`m3_dialog_modeless`]. The scrim only paints its dim background (in iced a
/// plain `container` does not capture pointer events); modality is decided by
/// the caller wrapping this in `opaque` or not.
fn m3_dialog_layers(inner: Element<'_, Message>) -> Element<'_, Message> {
    let card = container(inner).style(|t: &Theme| {
        let p = pal_of(t);
        container::Style {
            background: Some(p.surface_container.into()),
            border: iced::Border {
                color: p.outline_variant,
                width: 1.0,
                // M3 dialogs sit at the extra-large step — now an actual
                // token rather than a literal that happened to match it.
                radius: theme::shape::XL.into(),
            },
            shadow: iced::Shadow {
                color: with_alpha(p.shadow, 0.3),
                offset: iced::Vector::new(0.0, 8.0),
                blur_radius: 24.0,
            },
            ..Default::default()
        }
    });
    let scrim = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        // M3 modal scrim: the `scrim` role (black) at 32%, not a hardcoded 45%.
        .style(|t: &Theme| container::Style {
            background: Some(with_alpha(pal_of(t).scrim, 0.32).into()),
            ..Default::default()
        });
    let centered = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);
    iced::widget::stack![scrim, centered].into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WizardStepState {
    Completed,
    Active,
    Upcoming,
}

pub(crate) fn wizard_step_state(index: usize, current: usize) -> WizardStepState {
    use WizardStepState::{Active, Completed, Upcoming};

    match index.cmp(&current) {
        std::cmp::Ordering::Less => Completed,
        std::cmp::Ordering::Equal => Active,
        std::cmp::Ordering::Greater => Upcoming,
    }
}

pub(crate) fn wizard_step_bar(steps: &[&str], current: usize) -> Element<'static, Message> {
    let labels: Vec<String> = steps.iter().map(|label| (*label).to_string()).collect();
    let steps_len = labels.len();

    responsive(move |size| {
        // Per step: 32 marker + 8 gap + 8 connector + label. The widest bundled
        // locale (ru) needs 801 px for the 7-step root flow and 651 px for the
        // 6-step flash flow, so 118 px per step plus the row's 48 px padding and
        // the active pill's 14 px tail clears every locale with margin. Scaling
        // by step count matters because flows carry 5, 6 or 7 steps.
        let wide = size.width >= steps_len as f32 * 118.0 + 80.0;
        let mut r = row![]
            .spacing(0)
            .align_y(iced::Alignment::Center)
            .padding([8, 24])
            .height(Length::Fixed(48.0));

        for (i, label) in labels.iter().enumerate() {
            if i > 0 {
                let completed = i <= current;
                r = r.push(container(text("")).width(Length::Fill).height(2).style(
                    move |t: &Theme| {
                        let p = pal_of(t);
                        let color = if completed {
                            p.primary
                        } else {
                            p.outline_variant
                        };
                        container::Style {
                            background: Some(color.into()),
                            ..Default::default()
                        }
                    },
                ));
            }

            let state = wizard_step_state(i, current);
            let marker_text = if state == WizardStepState::Completed {
                "\u{2713}".to_string()
            } else {
                (i + 1).to_string()
            };

            let marker = container(text(marker_text).size(12).center().style(move |t: &Theme| {
                let p = pal_of(t);
                let color = match state {
                    WizardStepState::Completed => p.on_primary_container,
                    WizardStepState::Active => p.on_primary,
                    WizardStepState::Upcoming => p.on_surface_variant,
                };
                iced::widget::text::Style { color: Some(color) }
            }))
            .width(32)
            .height(32)
            .center_x(32)
            .center_y(32)
            .style(move |t: &Theme| {
                let p = pal_of(t);
                let (background, border_color) = match state {
                    WizardStepState::Completed => (p.primary_container, p.primary_container),
                    WizardStepState::Active => (p.primary, p.primary),
                    WizardStepState::Upcoming => (p.surface_container_high, p.outline_variant),
                };
                container::Style {
                    background: Some(background.into()),
                    border: iced::Border {
                        color: border_color,
                        width: 1.0,
                        radius: theme::shape::FULL.into(),
                    },
                    ..Default::default()
                }
            });

            let step_node: Element<'static, Message> = if state == WizardStepState::Active {
                container(
                    row![
                        marker,
                        text(label.clone())
                            .size(12)
                            .wrapping(iced::widget::text::Wrapping::None)
                            .style(move |t: &Theme| iced::widget::text::Style {
                                color: Some(pal_of(t).on_primary_container),
                            }),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                )
                .height(Length::Fixed(40.0))
                .padding(iced::Padding {
                    top: 4.0,
                    right: 14.0,
                    bottom: 4.0,
                    left: 4.0,
                })
                .style(|t: &Theme| container::Style {
                    background: Some(pal_of(t).primary_container.into()),
                    border: iced::Border {
                        radius: theme::shape::FULL.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into()
            } else if wide {
                row![
                    marker,
                    text(label.clone())
                        .size(12)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .style(move |t: &Theme| {
                            let p = pal_of(t);
                            let color = match state {
                                WizardStepState::Completed => p.on_primary_container,
                                WizardStepState::Upcoming => p.on_surface_variant,
                                WizardStepState::Active => unreachable!(),
                            };
                            iced::widget::text::Style { color: Some(color) }
                        }),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .into()
            } else {
                widget::tooltip(
                    marker,
                    container(text(label.clone()).size(12))
                        .padding([6, 10])
                        .style(|t: &Theme| theme::tooltip_style(t, theme::shape::SM)),
                    widget::tooltip::Position::Bottom,
                )
                .into()
            };
            r = r.push(step_node);
        }

        column![
            container(r)
                .width(Length::Fill)
                .style(|t: &Theme| container::Style {
                    background: Some(pal_of(t).surface_container_low.into()),
                    ..Default::default()
                }),
            widget::rule::horizontal(1).style(shell_rule_style),
        ]
        .into()
    })
    .into()
}

/// Large flexible top app bar for screen or wizard title/description.
pub(crate) fn large_top_app_bar<'a>(
    title: String,
    subtitle: Option<String>,
) -> Element<'a, Message> {
    let mut content = column![
        text(title)
            .size(theme::text_size::HEADLINE_MEDIUM)
            .font(theme::emphasis::bold())
            .style(on_surface_style)
            .width(Length::Fill)
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
    ]
    .spacing(6)
    .width(Length::Fill)
    .align_x(iced::Alignment::Start);

    if let Some(subtitle) = subtitle.filter(|s| !s.trim().is_empty()) {
        content = content.push(
            text(subtitle)
                .size(theme::text_size::BODY_MEDIUM)
                .style(muted_style)
                .width(Length::Fill)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        );
    }

    column![
        container(
            container(content)
                .width(Length::Fill)
                .max_width(WIZARD_TOP_APP_BAR_MAX_WIDTH)
        )
        .width(Length::Fill)
        .height(Length::Fixed(WIZARD_TOP_APP_BAR_HEIGHT))
        .padding(iced::Padding {
            top: 18.0,
            right: 24.0,
            bottom: 22.0,
            left: 24.0,
        })
        .align_x(iced::Alignment::Start)
        .align_y(iced::Alignment::End)
        .style(|t: &Theme| panel_bg(t)),
        widget::rule::horizontal(1).style(shell_rule_style),
    ]
    .into()
}

/// Large flexible top app bar for wizard step title/description. The
/// app-bar surface owns the step context, while the body can focus on
/// the actual controls.
pub(crate) fn wizard_action_bar<'a>(
    title: String,
    subtitle: Option<String>,
) -> Element<'a, Message> {
    large_top_app_bar(title, subtitle)
}

pub(crate) fn sec_hdr<'a>(label: &str, label_alpha: f32) -> Element<'a, Message> {
    if label_alpha <= 0.0 {
        return container(text(""))
            .height(Length::Fixed(SEC_HDR_HEIGHT))
            .into();
    }
    let owned = label.to_string();
    let alpha = label_alpha;
    container(
        text(owned)
            .size(theme::text_size::LABEL_SMALL)
            // Same no-wrap rationale as nav_btn — section header text
            // ("Tools" / "도구") must not flow into two lines mid-tween.
            .wrapping(iced::widget::text::Wrapping::None)
            .style(move |t: &Theme| iced::widget::text::Style {
                color: Some(with_alpha(pal_of(t).on_surface_variant, alpha)),
            }),
    )
    .padding([10, 22])
    .height(Length::Fixed(SEC_HDR_HEIGHT))
    .into()
}

fn card_content<'a>(title: &str, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![
        text(title.to_string())
            .size(theme::text_size::TITLE_SMALL)
            .font(theme::emphasis::medium())
            .style(muted_style)
            .line_height(1.0),
        content.into(),
    ]
    .spacing(6)
    .padding(iced::Padding {
        top: 10.0,
        right: 18.0,
        bottom: 14.0,
        left: 18.0,
    })
    .width(Length::Fill)
    .into()
}

pub(crate) fn card<'a>(
    title: &str,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(card_content(title, content))
        .width(Length::Fill)
        .style(|t: &Theme| {
            theme::surface_card_style(t, theme::SurfaceLevel::Default, theme::shape::LG, 1)
        })
        .into()
}

pub(crate) fn clickable_card<'a>(
    title: &str,
    content: impl Into<Element<'a, Message>>,
    message: Message,
) -> Element<'a, Message> {
    button(card_content(title, content))
        .on_press(message)
        .padding(0)
        .width(Length::Fill)
        .style(|t: &Theme, status| {
            let p = pal_of(t);
            button::Style {
                background: Some(
                    theme::mix_color(p.surface_container, p.primary, theme::state_alpha(status))
                        .into(),
                ),
                text_color: p.on_surface,
                border: iced::Border {
                    color: p.outline_variant,
                    width: 1.0,
                    radius: theme::shape::LG.into(),
                },
                shadow: theme::elevation(1, theme::is_dark(t)),
                ..Default::default()
            }
        })
        .into()
}

pub(crate) fn info_kv<'a>(label: &str, value: &str) -> Element<'a, Message> {
    column![
        text(label.to_string())
            .size(theme::text_size::LABEL_SMALL)
            .style(muted_style),
        // Value outranks its caption on weight and color rather than
        // size — at `BODY_LARGE` the kv grid competed with the device
        // name above it, which is what pushed that name oversized in the
        // first place.
        text(value.to_string())
            .size(theme::text_size::BODY_MEDIUM)
            .font(theme::emphasis::medium()),
    ]
    .spacing(3)
    .into()
}

pub(crate) fn info_kv_center<'a>(label: &str, value: &str) -> Element<'a, Message> {
    column![
        text(label.to_string())
            .size(11)
            .style(muted_style)
            .width(Length::Fill)
            .center(),
        // `WordOrGlyph` so a long, space-less file path wraps at glyph
        // boundaries within the panel instead of overflowing + clipping.
        text(value.to_string())
            .size(14)
            .width(Length::Fill)
            .center()
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
    ]
    .spacing(3)
    .width(Length::Fill)
    .align_x(iced::Alignment::Center)
    .into()
}

/// Centered summary row that opens an immediate picker/action. Unlike
/// `info_kv_center_editable`, this is not an override row, so it only uses the
/// standard state layer on hover/press and never shows the warning accent.
pub(crate) fn info_kv_center_action<'a>(
    label: &str,
    value: &str,
    on_press: Message,
) -> Element<'a, Message> {
    let inner = column![
        text(label.to_string())
            .size(11)
            .style(muted_style)
            .width(Length::Fill)
            .center(),
        text(value.to_string())
            .size(14)
            .width(Length::Fill)
            .center()
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
    ]
    .spacing(3)
    .width(Length::Fill)
    .align_x(iced::Alignment::Center);

    button(inner)
        .on_press(on_press)
        .padding([6, 10])
        .width(Length::Fill)
        .style(|t: &Theme, status| {
            let p = pal_of(t);
            let alpha = theme::state_alpha(status);
            button::Style {
                background: (alpha > 0.0).then(|| with_alpha(p.on_surface, alpha).into()),
                text_color: p.on_surface,
                border: iced::Border {
                    radius: theme::shape::SM.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .into()
}

/// Like [`info_kv_center`] but the value is a click-to-edit "hidden
/// dropdown": pixel-identical to the static row until pressed, so casual
/// users never notice it. When `changed` (the picked option diverges from
/// the confirm baseline) the row takes an accent background + border and a
/// hover caution spelling out that this is a power-user override.
pub(crate) fn info_kv_center_editable<'a>(
    label: &str,
    value: &str,
    changed: bool,
    caution: &str,
    on_open: Message,
) -> Element<'a, Message> {
    let inner = column![
        text(label.to_string())
            .size(11)
            .style(muted_style)
            .width(Length::Fill)
            .center(),
        text(value.to_string())
            .size(14)
            .width(Length::Fill)
            .center()
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
    ]
    .spacing(3)
    .width(Length::Fill)
    .align_x(iced::Alignment::Center);

    let btn = button(inner)
        .on_press(on_open)
        .padding([6, 10])
        .width(Length::Fill)
        .style(move |t: &Theme, status| {
            let p = pal_of(t);
            // Unchanged: no fill even on hover, so the row reads as plain
            // text. Changed: accent tint that deepens slightly on hover/press.
            let bg = if changed {
                with_alpha(p.primary, 0.16 + theme::state_alpha(status))
            } else {
                iced::Color::TRANSPARENT
            };
            button::Style {
                background: Some(bg.into()),
                text_color: p.on_surface,
                border: iced::Border {
                    color: if changed {
                        p.primary
                    } else {
                        iced::Color::TRANSPARENT
                    },
                    width: if changed { 1.0 } else { 0.0 },
                    radius: theme::shape::SM.into(),
                },
                ..Default::default()
            }
        });

    if !changed {
        return btn.into();
    }

    iced::widget::tooltip(
        btn,
        container(
            text(caution.to_string())
                .size(12)
                .style(warning_style)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        )
        .padding([8, 12])
        .max_width(280)
        .style(|t: &Theme| {
            let p = pal_of(t);
            container::Style {
                background: Some(p.surface_container_high.into()),
                border: iced::Border {
                    color: p.outline_variant,
                    width: 1.0,
                    radius: theme::shape::SM.into(),
                },
                ..Default::default()
            }
        }),
        iced::widget::tooltip::Position::Top,
    )
    .gap(6)
    .into()
}

/// Overlay a small "recommended" star badge on the top-right corner of an
/// option card. Hovering the badge surfaces `tip`. The badge lives in a
/// non-interactive overlay layer, so the card underneath stays fully
/// clickable (same `stack` pattern as the dashboard save-FAB).
pub(crate) fn recommended_overlay(
    card: Element<'static, Message>,
    tip: String,
) -> Element<'static, Message> {
    let badge = container(lucide_icon(icon::rec_badge(), 13.0, |t: &Theme| {
        pal_of(t).on_primary
    }))
    .width(22)
    .height(22)
    .center_x(22)
    .center_y(22)
    .style(|t: &Theme| {
        let p = pal_of(t);
        container::Style {
            background: Some(p.primary.into()),
            border: iced::Border {
                radius: theme::shape::FULL.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    });
    let badge_tip = iced::widget::tooltip(
        badge,
        container(
            text(tip)
                .size(12)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        )
        .padding([8, 12])
        .max_width(240)
        .style(|t: &Theme| {
            let p = pal_of(t);
            container::Style {
                background: Some(p.surface_container_high.into()),
                border: iced::Border {
                    color: p.outline_variant,
                    width: 1.0,
                    radius: theme::shape::SM.into(),
                },
                ..Default::default()
            }
        }),
        iced::widget::tooltip::Position::Top,
    )
    .gap(6);
    // Full-size overlay pins the badge to the top-right; padding insets it
    // from the card edge. The inset has to clear the card's corner arc —
    // at 8 px against an `LG` radius the badge sat on the curve and read
    // as clipped by it.
    let overlay = container(badge_tip)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(12)
        .align_x(iced::Alignment::End)
        .align_y(iced::Alignment::Start);
    iced::widget::stack![card, overlay].into()
}

pub(crate) fn adv_grid_btn<'a>(item: AdvAction, label: &str) -> Element<'a, Message> {
    // Inner container: border-only via `sel_card_style`. Earlier
    // version used `theme::surface_card_style` which paints an opaque
    // bg — that bg sat on top of the button's hover fill, swallowing
    // the highlight and making the grid feel dead on hover.
    let destructive = item.is_destructive();
    let content = container(
        text(label.to_string())
            .size(12)
            .center()
            .width(Length::Fill)
            .style(move |t: &Theme| {
                let p = pal_of(t);
                iced::widget::text::Style {
                    color: Some(if destructive { p.error } else { p.on_surface }),
                }
            }),
    )
    .padding([18, 12])
    .width(Length::Fill)
    .center_x(Length::Fill)
    .style(move |t: &Theme| sel_card_style_for(t, false, destructive));

    button(content)
        .on_press(Message::Adv(AdvMsg::AdvConfirm(item)))
        .padding(0)
        .width(Length::Fill)
        .style(move |t: &Theme, status| sel_card_btn_style_for(t, status, false, destructive))
        .into()
}

pub(crate) fn svg_icon(bytes: &'static [u8], size: f32) -> Element<'static, Message> {
    iced::widget::svg(iced::widget::svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .into()
}

static SKROOT_ICON_HANDLE: std::sync::LazyLock<iced::widget::image::Handle> =
    std::sync::LazyLock::new(|| {
        iced::widget::image::Handle::from_bytes(
            include_bytes!("../../assets/icons/skroot.png").as_slice(),
        )
    });

pub(crate) fn skroot_icon(size: f32) -> Element<'static, Message> {
    widget::image(SKROOT_ICON_HANDLE.clone())
        .width(size)
        .height(size)
        .content_fit(iced::ContentFit::ScaleDown)
        .into()
}

/// Primary-coloured Lucide icon sized to `size`. Matches the colour
/// role the old per-asset SVG glyphs used for wizard tiles, status
/// markers, and confirm-step eyebrows.
pub(crate) fn lucide_primary(
    icon: iced::widget::Text<'static, Theme, iced::Renderer>,
    size: f32,
) -> Element<'static, Message> {
    icon.size(size)
        .style(|t: &Theme| iced::widget::text::Style {
            color: Some(pal_of(t).primary),
        })
        .into()
}

/// Error-coloured Lucide icon. Pairs with
/// [`icon_option_card_sub_square_destructive_sized`] so a data-erasing
/// option reads as destructive at the glyph, not just at the border.
pub(crate) fn lucide_error(
    icon: iced::widget::Text<'static, Theme, iced::Renderer>,
    size: f32,
) -> Element<'static, Message> {
    icon.size(size)
        .style(|t: &Theme| iced::widget::text::Style {
            color: Some(pal_of(t).error),
        })
        .into()
}

/// Disabled-state Lucide icon — `on_surface` at 0.38 alpha (M3 disabled
/// content tone). Pair with [`icon_option_card_sub_disabled`] so the
/// whole card reads as "not pickable on this device".
pub(crate) fn lucide_disabled(
    icon: iced::widget::Text<'static, Theme, iced::Renderer>,
    size: f32,
) -> Element<'static, Message> {
    icon.size(size)
        .style(|t: &Theme| iced::widget::text::Style {
            color: Some(with_alpha(pal_of(t).on_surface, 0.38)),
        })
        .into()
}

/// Lucide icon coloured by an arbitrary theme-driven closure. Used
/// where colour depends on widget state (nav active / disabled,
/// op success / failure, title-bar hover).
pub(crate) fn lucide_icon(
    icon: iced::widget::Text<'static, Theme, iced::Renderer>,
    size: f32,
    color: impl Fn(&Theme) -> iced::Color + 'static,
) -> Element<'static, Message> {
    icon.size(size)
        .style(move |t: &Theme| iced::widget::text::Style {
            color: Some(color(t)),
        })
        .into()
}

pub(crate) fn icon_option_card_sub_square_sized(
    icon: Element<'static, Message>,
    label: &str,
    sub: &str,
    selected: bool,
    msg: Message,
    side: f32,
) -> Element<'static, Message> {
    option_card(icon, label, sub, selected, Some(msg), Some(side), false)
}

/// Square option card for a choice that destroys data. Renders on the
/// `error` role instead of `primary` so "wipe" never looks like "keep"
/// with a different label — pair it with [`lucide_error`].
pub(crate) fn icon_option_card_sub_square_destructive_sized(
    icon: Element<'static, Message>,
    label: &str,
    sub: &str,
    selected: bool,
    msg: Message,
    side: f32,
) -> Element<'static, Message> {
    option_card(icon, label, sub, selected, Some(msg), Some(side), true)
}

pub(crate) fn icon_option_card_sub_square_disabled_sized(
    icon: Element<'static, Message>,
    label: &str,
    sub: &str,
    side: f32,
) -> Element<'static, Message> {
    option_card(icon, label, sub, false, None, Some(side), false)
}

/// Shared body for the vertical icon → title → description option card.
/// `msg = None` renders the disabled affordance; `square_side` swaps the
/// full-width × fixed-height box for a fixed 1:1 square; `destructive`
/// swaps the accent role from `primary` to `error`.
fn option_card(
    icon: Element<'static, Message>,
    label: &str,
    sub: &str,
    selected: bool,
    msg: Option<Message>,
    square_side: Option<f32>,
    destructive: bool,
) -> Element<'static, Message> {
    let enabled = msg.is_some();
    let square = square_side.is_some();
    let side = square_side.unwrap_or(WIZARD_CARD_SQUARE);
    let label_style_fn = if enabled {
        on_surface_style
    } else {
        muted_style
    };
    // Sub text centres vertically inside the fixed box — top-aligning
    // left long gaps between short descs and the label above.
    let sub_text: Element<'static, Message> = if sub.is_empty() {
        text(" ").size(11).width(Length::Fill).center().into()
    } else {
        text(sub.to_string())
            .size(11)
            .style(muted_style)
            .width(Length::Fill)
            .center()
            .into()
    };
    // Square cards are narrower (fixed side), so longer localized
    // descriptions wrap to more lines. The 200px-tall square has vertical
    // slack below the icon + label, so give it a taller sub-row to absorb
    // ~4 lines instead of clipping; the standard card keeps its 2-line row.
    let sub_h = if square {
        WIZARD_CARD_SQUARE_SUB_HEIGHT
    } else {
        SUB_ROW_HEIGHT
    };
    let sub_row = container(sub_text)
        .width(Length::Fill)
        .height(Length::Fixed(sub_h))
        .align_y(iced::alignment::Vertical::Center);
    // Explicit icon→label vs label→desc gaps — a single `spacing` read
    // unbalanced because the centred sub-row adds ~9 px padding.
    let content = column![
        icon_tile(icon),
        Space::new().height(14),
        text(label.to_string())
            .size(theme::text_size::TITLE_MEDIUM)
            .font(theme::emphasis::medium())
            .style(label_style_fn)
            .width(Length::Fill)
            .center(),
        Space::new().height(4),
        sub_row,
    ]
    .spacing(0)
    .align_x(iced::Alignment::Center);

    // Square → fixed side both ways so the row shrink-wraps and centres;
    // otherwise full width × the standard card height.
    let card_w: Length = if square {
        Length::Fixed(side)
    } else {
        Length::Fill
    };
    let card_h: f32 = if square { side } else { WIZARD_CARD_HEIGHT };

    let inner = container(content)
        .padding([20, 16])
        .width(card_w)
        .height(card_h)
        .center_x(card_w)
        .center_y(card_h)
        .style(move |t: &Theme| sel_card_style_for(t, selected && enabled, destructive && enabled));

    let btn = button(inner).padding(0).width(card_w);
    match msg {
        Some(m) => btn
            .on_press(m)
            .style(move |t: &Theme, status| {
                sel_card_btn_style_for(t, status, selected, destructive)
            })
            .into(),
        None => btn
            // No `on_press` — iced reports Status::Disabled. Stronger M3
            // disabled affordance: dimmer surface + a thin outline_variant
            // border so the inert card reads distinctly against active ones.
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

/// Wrap a wizard icon. Icons already carry their own rounded-rect bg,
/// so no outer border.
pub(crate) fn icon_tile(icon: Element<'static, Message>) -> Element<'static, Message> {
    container(icon).padding(0).into()
}

impl RebootTarget {
    pub(crate) fn icon(self) -> Element<'static, Message> {
        let glyph = match self {
            Self::System => icon::reboot_system(),
            Self::Recovery => icon::reboot_recovery(),
            Self::Bootloader => icon::reboot_bootloader(),
            Self::Edl => icon::reboot_edl(),
        };
        lucide_primary(glyph, 32.0)
    }
}

impl Family {
    pub(crate) fn icon_sized(self, size: f32) -> Element<'static, Message> {
        // Kept as bundled SVG assets — these are per-brand logos, not
        // monochrome glyphs, so Lucide's icon set doesn't cover them.
        let bytes: &'static [u8] = match self {
            Self::Magisk => include_bytes!("../../assets/icons/magisk.svg"),
            Self::KernelSU => include_bytes!("../../assets/icons/kernelsu.svg"),
            Self::APatch => include_bytes!("../../assets/icons/apatch.svg"),
            Self::Skroot => return skroot_icon(size),
        };
        svg_icon(bytes, size)
    }
}

impl Provider {
    /// Provider brand logo at an explicit size. The 2-provider square cards
    /// pass a smaller value so the 72px logo doesn't overflow the fixed
    /// square; the full-width grid cards keep the default 72px.
    pub(crate) fn icon_sized(self, size: f32) -> Element<'static, Message> {
        // Provider brand logos — kept as bespoke SVG, not Lucide.
        let bytes: &'static [u8] = match self {
            Self::Magisk => include_bytes!("../../assets/icons/magisk.svg"),
            Self::MagiskForks => include_bytes!("../../assets/icons/magisk_forks.svg"),
            Self::KernelSU => include_bytes!("../../assets/icons/kernelsu.svg"),
            Self::KernelSUNext => include_bytes!("../../assets/icons/kernelsu_next.svg"),
            Self::SukiSU => include_bytes!("../../assets/icons/sukisu.svg"),
            Self::ReSukiSU => include_bytes!("../../assets/icons/sukisu.svg"),
            Self::APatch => include_bytes!("../../assets/icons/apatch.svg"),
            Self::FolkPatch => include_bytes!("../../assets/icons/folkpatch.svg"),
        };
        svg_icon(bytes, size)
    }
}

impl RootMode {
    pub(crate) fn icon(self) -> Element<'static, Message> {
        // Lucide chip/layers glyphs in place of the old bespoke SVGs.
        let glyph = match self {
            Self::Lkm => icon::root_lkm(),
            Self::Gki => icon::root_gki(),
        };
        lucide_primary(glyph, 57.6)
    }
}

impl VerChoice {
    pub(crate) fn icon(self) -> Element<'static, Message> {
        let glyph = match self {
            Self::Stable => icon::ver_stable(),
            Self::Nightly => icon::ver_nightly(),
        };
        lucide_primary(glyph, 57.6)
    }
}

impl NightlySource {
    pub(crate) fn icon(self) -> Element<'static, Message> {
        let glyph = match self {
            Self::AutoDetect => icon::nightly_auto(),
            Self::ManualInput => icon::nightly_manual(),
        };
        lucide_primary(glyph, 57.6)
    }
}

impl App {
    /// Shared confirm-screen frame when the step title/description already
    /// live in the wizard action bar. Leading rows are full-width callouts or
    /// lists, short values form a two-column grid, and trailing rows hold
    /// full-width paths or supporting details.
    pub(crate) fn confirm_step_frame<'a>(
        &self,
        leading: Vec<Element<'a, Message>>,
        grid: Vec<Element<'a, Message>>,
        trailing: Vec<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let mut groups: Vec<Element<'a, Message>> = Vec::new();

        if !leading.is_empty() {
            groups.push(
                column(leading)
                    .spacing(8)
                    .width(Length::Fill)
                    .align_x(iced::Alignment::Center)
                    .into(),
            );
        }

        if !grid.is_empty() {
            let mut grid_rows = column![].spacing(8).width(Length::Fill);
            let mut cells = grid.into_iter();
            while let Some(left) = cells.next() {
                grid_rows = if let Some(right) = cells.next() {
                    grid_rows.push(row![left, right].spacing(12).width(Length::Fill))
                } else {
                    // A final unpaired scalar uses the whole line instead of
                    // leaving an empty half-cell beside it.
                    grid_rows.push(container(left).width(Length::Fill))
                };
            }
            groups.push(grid_rows.into());
        }

        if !trailing.is_empty() {
            groups.push(
                column(trailing)
                    .spacing(8)
                    .width(Length::Fill)
                    .align_x(iced::Alignment::Center)
                    .into(),
            );
        }

        let mut content = column![]
            .spacing(8)
            .padding([18, 28])
            .width(Length::Fill)
            .align_x(iced::Alignment::Center);
        for (index, group) in groups.into_iter().enumerate() {
            if index > 0 {
                content = content.push(widget::rule::horizontal(1));
            }
            content = content.push(group);
        }

        // The scroller itself shrinks when content is short, allowing the
        // outer fill-height container to center it. Its child deliberately
        // has no fill height: a scrollable measures content with an unbounded
        // vertical limit, where Fill cannot resolve to the viewport.
        let summary = iced::widget::scrollable(content)
            .style(m3_scrollable_style)
            .height(Length::Shrink)
            .width(Length::Fill);

        container(
            container(summary)
                .width(Length::Fill)
                .max_width(WIZARD_CONFIRM_MAX_WIDTH),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }
}
