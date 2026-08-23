//! Wizard navigation bars and small view widgets/helpers (nav buttons,
//! color blend/easing, device portrait, layout consts). Extracted from main.rs.

use crate::*;
use iced::widget::{Space, button, canvas, container, row, text};
use iced::{Element, Length, Point, Radians, Rectangle, Renderer, Theme, mouse, window};

const MATERIAL_PROGRESS_PERIOD: std::time::Duration = std::time::Duration::from_millis(1_400);
const MATERIAL_PROGRESS_FRAME: std::time::Duration = std::time::Duration::from_millis(33);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaterialProgressSize {
    Standard,
    Hero,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct MaterialProgressMetrics {
    diameter: f32,
    stroke_width: f32,
    track_gap: f32,
}

fn material_progress_metrics(size: MaterialProgressSize) -> MaterialProgressMetrics {
    match size {
        MaterialProgressSize::Standard => MaterialProgressMetrics {
            diameter: 40.0,
            stroke_width: 4.0,
            track_gap: 4.0,
        },
        MaterialProgressSize::Hero => MaterialProgressMetrics {
            diameter: 52.0,
            stroke_width: 8.0,
            track_gap: 4.0,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct MaterialProgressArc {
    start_angle: f32,
    sweep_angle: f32,
}

fn material_progress_arc(phase: f32) -> MaterialProgressArc {
    let phase = phase.rem_euclid(1.0);
    let pulse = 0.5 - 0.5 * (std::f32::consts::TAU * phase).cos();
    MaterialProgressArc {
        start_angle: -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * phase,
        sweep_angle: std::f32::consts::TAU * (0.12 + 0.55 * pulse),
    }
}

fn material_progress_gap_angle(metrics: MaterialProgressMetrics, radius: f32) -> f32 {
    // Round caps extend half a stroke beyond each path endpoint. Add one full
    // stroke width to the token gap so the visible cap-to-cap space stays 4px.
    (metrics.track_gap + metrics.stroke_width) / radius
}

#[derive(Debug, Clone, Copy)]
struct MaterialProgress {
    size: MaterialProgressSize,
}

#[derive(Debug, Default)]
struct MaterialProgressState {
    started_at: Option<iced::time::Instant>,
    phase: f32,
}

impl canvas::Program<Message> for MaterialProgress {
    type State = MaterialProgressState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let canvas::Event::Window(window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        let started_at = *state.started_at.get_or_insert(*now);
        state.phase =
            now.duration_since(started_at).as_secs_f32() / MATERIAL_PROGRESS_PERIOD.as_secs_f32();
        state.phase = state.phase.rem_euclid(1.0);
        Some(canvas::Action::request_redraw_at(
            *now + MATERIAL_PROGRESS_FRAME,
        ))
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let metrics = material_progress_metrics(self.size);
        let arc = material_progress_arc(state.phase);
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let radius = (bounds.width.min(bounds.height) - metrics.stroke_width) / 2.0;
        let gap_angle = material_progress_gap_angle(metrics, radius);
        let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);

        let active_end = arc.start_angle + arc.sweep_angle;
        let track_start = active_end + gap_angle;
        let track_end = arc.start_angle + std::f32::consts::TAU - gap_angle;
        let track = canvas::Path::new(|builder| {
            builder.arc(canvas::path::Arc {
                center,
                radius,
                start_angle: Radians(track_start),
                end_angle: Radians(track_end),
            });
        });
        let active = canvas::Path::new(|builder| {
            builder.arc(canvas::path::Arc {
                center,
                radius,
                start_angle: Radians(arc.start_angle),
                end_angle: Radians(active_end),
            });
        });
        let palette = pal_of(theme);
        let stroke = canvas::Stroke::default()
            .with_width(metrics.stroke_width)
            .with_line_cap(canvas::LineCap::Round);
        frame.stroke(&track, stroke.with_color(palette.surface_container_highest));
        frame.stroke(&active, stroke.with_color(palette.primary));
        vec![frame.into_geometry()]
    }
}

pub(crate) fn material_circular_progress(size: MaterialProgressSize) -> Element<'static, Message> {
    let metrics = material_progress_metrics(size);
    canvas::Canvas::new(MaterialProgress { size })
        .width(Length::Fixed(metrics.diameter))
        .height(Length::Fixed(metrics.diameter))
        .into()
}

/// One full pass through the shape sequence.
const LOADING_INDICATOR_PERIOD: std::time::Duration = std::time::Duration::from_millis(4_000);
/// Active-indicator diameter. M3 scales this component; 48 is the size
/// that fits the popup loading slot the app already reserves.
pub(crate) const LOADING_INDICATOR_SIZE: f32 = 48.0;
/// Lobe counts the indicator morphs through. M3's loading indicator is a
/// loop over seven Material shapes; expressing them as harmonic lobe
/// counts gives the same "one shape flows into the next" reading without
/// needing a shape library and a polygon-interpolation pass.
const LOADING_INDICATOR_LOBES: [f32; 7] = [3.0, 4.0, 5.0, 4.0, 6.0, 5.0, 3.0];
/// How far the lobes push off the base circle, as a fraction of radius.
const LOADING_INDICATOR_AMPLITUDE: f32 = 0.13;
/// Points sampled around the outline. Enough that the fill reads as a
/// smooth curve rather than a polygon at this diameter.
const LOADING_INDICATOR_SAMPLES: usize = 160;

#[derive(Debug, Clone, Copy)]
struct MaterialLoadingIndicator;

/// Outline radius at angle `theta`, morphing between the lobe count at
/// `phase` and the next one. Crossfading two cosine harmonics keeps the
/// transition continuous — stepping the lobe count directly would jump.
fn loading_indicator_radius(base: f32, theta: f32, phase: f32) -> f32 {
    let span = LOADING_INDICATOR_LOBES.len() as f32;
    let pos = phase.rem_euclid(1.0) * span;
    let index = pos.floor() as usize % LOADING_INDICATOR_LOBES.len();
    let next = (index + 1) % LOADING_INDICATOR_LOBES.len();
    // Smoothstep the blend so each shape holds briefly before flowing on,
    // instead of the whole loop reading as one continuous wobble.
    let raw = pos - pos.floor();
    let blend = raw * raw * (3.0 - 2.0 * raw);

    let from = (LOADING_INDICATOR_LOBES[index] * theta).cos();
    let to = (LOADING_INDICATOR_LOBES[next] * theta).cos();
    let lobe = from * (1.0 - blend) + to * blend;
    base * (1.0 + LOADING_INDICATOR_AMPLITUDE * lobe)
}

impl canvas::Program<Message> for MaterialLoadingIndicator {
    type State = MaterialProgressState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let canvas::Event::Window(window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        let started_at = *state.started_at.get_or_insert(*now);
        state.phase =
            now.duration_since(started_at).as_secs_f32() / LOADING_INDICATOR_PERIOD.as_secs_f32();
        state.phase = state.phase.rem_euclid(1.0);
        Some(canvas::Action::request_redraw_at(
            *now + MATERIAL_PROGRESS_FRAME,
        ))
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        // Leave room for the lobes so the shape never clips its bounds.
        let base = (bounds.width.min(bounds.height) / 2.0) / (1.0 + LOADING_INDICATOR_AMPLITUDE);
        // Rotation is what turns a pulsing outline into something that
        // reads as active; one turn per shape cycle.
        let spin = std::f32::consts::TAU * state.phase;

        let path = canvas::Path::new(|builder| {
            for i in 0..=LOADING_INDICATOR_SAMPLES {
                let theta = std::f32::consts::TAU * (i as f32 / LOADING_INDICATOR_SAMPLES as f32);
                let radius = loading_indicator_radius(base, theta, state.phase);
                let angle = theta + spin;
                let point = Point::new(
                    center.x + radius * angle.cos(),
                    center.y + radius * angle.sin(),
                );
                if i == 0 {
                    builder.move_to(point);
                } else {
                    builder.line_to(point);
                }
            }
            builder.close();
        });

        frame.fill(&path, pal_of(theme).primary);
        vec![frame.into_geometry()]
    }
}

/// M3 Expressive loading indicator — the specified replacement for an
/// indeterminate circular progress indicator on short waits (roughly
/// 200 ms to 5 s). Unlike a progress ring it communicates through shape
/// and motion rather than an arc sweep, and it is never decorative.
pub(crate) fn material_loading_indicator() -> Element<'static, Message> {
    canvas::Canvas::new(MaterialLoadingIndicator)
        .width(Length::Fixed(LOADING_INDICATOR_SIZE))
        .height(Length::Fixed(LOADING_INDICATOR_SIZE))
        .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecPrimaryAction {
    StartOver,
    OpenFolder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExecActionLayout {
    pub(crate) primary: Option<ExecPrimaryAction>,
    pub(crate) start_over_utility: bool,
}

pub(crate) const fn exec_action_layout(
    is_busy: bool,
    is_error: bool,
    has_output: bool,
) -> ExecActionLayout {
    if is_busy {
        ExecActionLayout {
            primary: None,
            start_over_utility: false,
        }
    } else if has_output && !is_error {
        ExecActionLayout {
            primary: Some(ExecPrimaryAction::OpenFolder),
            start_over_utility: true,
        }
    } else {
        ExecActionLayout {
            primary: Some(ExecPrimaryAction::StartOver),
            start_over_utility: false,
        }
    }
}

/// True for the localized "Start" / "Dump" labels. Drives the red Cancel
/// button in the footer helpers.
pub(crate) fn is_start_label(label: &str) -> bool {
    label == ltbox_core::i18n::tr("btn_start").as_str()
        || label == ltbox_core::i18n::tr("btn_dump").as_str()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WizardNavLayout {
    pub(crate) grouped_leading: bool,
    pub(crate) extended_primary: bool,
}

pub(crate) fn wizard_nav_layout(next_label: &str) -> WizardNavLayout {
    let confirmation = is_start_label(next_label);
    WizardNavLayout {
        grouped_leading: confirmation,
        extended_primary: true,
    }
}

fn fab_icon_content(
    icon: iced::widget::Text<'static, Theme, iced::Renderer>,
) -> Element<'static, Message> {
    container(icon.size(22))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn fab_next_icon(next_label: &str) -> iced::widget::Text<'static, Theme, iced::Renderer> {
    if is_start_label(next_label) {
        icon::fab_start()
    } else {
        icon::fab_next()
    }
}

fn fab_elevation_level(status: button::Status) -> u8 {
    match status {
        button::Status::Disabled => 0,
        button::Status::Hovered => 4,
        _ => 3,
    }
}

fn fab_style(t: &Theme, status: button::Status, bg: iced::Color, fg: iced::Color) -> button::Style {
    let p = pal_of(t);
    if matches!(status, button::Status::Disabled) {
        return button::Style {
            background: Some(with_alpha(p.on_surface, 0.12).into()),
            text_color: with_alpha(p.on_surface, 0.38),
            // M3 defines no disabled FAB at all — the token set carries
            // neither `disabled-*` nor any `outline-*` entry, and Material
            // Web draws no disabled state. So the container/content pair
            // borrows M3's generic disabled *button* treatment
            // (`on_surface @ 12%` / `@ 38%`), and no outline is drawn:
            // there is no token to justify one. A disabled Next still reads
            // apart from an enabled *surface* FAB (Back) because elevation
            // drops to 0 while Back rests at level 3, and the icon sits at
            // 38% rather than `on_surface_variant`.
            border: iced::Border {
                radius: theme::shape::FULL.into(),
                ..Default::default()
            },
            shadow: theme::elevation(0, theme::is_dark(t)),
            ..Default::default()
        };
    }

    button::Style {
        background: Some(theme::mix_color(bg, fg, theme::state_alpha(status)).into()),
        text_color: fg,
        border: iced::Border {
            radius: theme::shape::FULL.into(),
            ..Default::default()
        },
        shadow: theme::elevation(fab_elevation_level(status), theme::is_dark(t)),
        ..Default::default()
    }
}

/// The wizard's single most important control. M3's default FAB color
/// pair is `primary_container`/`on_primary_container`, which on the
/// indigo seed is a near-neutral `0xDDE1FF` that reads as pale grey next
/// to the surface FABs beside it. Expressive asks the one hero action to
/// take the loudest available role, so this uses the `primary` pair.
fn fab_primary_style(t: &Theme, status: button::Status) -> button::Style {
    let p = pal_of(t);
    fab_style(t, status, p.primary, p.on_primary)
}

fn fab_surface_style(t: &Theme, status: button::Status) -> button::Style {
    let p = pal_of(t);
    fab_style(t, status, p.surface_container_high, p.on_surface_variant)
}

fn utility_action_style(t: &Theme, status: button::Status) -> button::Style {
    let p = pal_of(t);
    button::Style {
        background: Some(
            theme::mix_color(
                p.surface_container_high,
                p.on_surface_variant,
                theme::state_alpha(status),
            )
            .into(),
        ),
        text_color: p.on_surface_variant,
        border: iced::Border {
            radius: theme::shape::FULL.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn utility_error_action_style(t: &Theme, status: button::Status) -> button::Style {
    let p = pal_of(t);
    button::Style {
        background: Some(
            theme::mix_color(
                p.surface_container_high,
                p.error,
                theme::state_alpha(status),
            )
            .into(),
        ),
        text_color: p.error,
        border: iced::Border {
            radius: theme::shape::FULL.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn extended_fab_primary_style(t: &Theme, status: button::Status) -> button::Style {
    let mut style = fab_primary_style(t, status);
    style.border.radius = theme::shape::LG.into();
    style
}

fn fab_tooltip<'a>(inner: Element<'a, Message>, label: String) -> Element<'a, Message> {
    iced::widget::tooltip(
        inner,
        container(text(label).size(12))
            .padding([6, 10])
            .style(|t: &Theme| {
                let p = pal_of(t);
                container::Style {
                    background: Some(p.surface_container_high.into()),
                    text_color: Some(p.on_surface),
                    border: iced::Border {
                        color: p.outline_variant,
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    shadow: theme::elevation(2, theme::is_dark(t)),
                    ..Default::default()
                }
            }),
        iced::widget::tooltip::Position::Top,
    )
    .into()
}

fn wizard_fab<'a>(
    icon: iced::widget::Text<'static, Theme, iced::Renderer>,
    label: String,
    msg: Option<Message>,
    style: fn(&Theme, button::Status) -> button::Style,
    disabled_hint: Option<String>,
) -> Element<'a, Message> {
    let mut btn = button(fab_icon_content(icon))
        .width(Length::Fixed(WIZARD_FAB_SIZE))
        .height(Length::Fixed(WIZARD_FAB_SIZE))
        .padding(0)
        .style(style);
    if let Some(msg) = msg {
        btn = btn.on_press(msg);
    }

    let tooltip = disabled_hint.unwrap_or(label);
    fab_tooltip(btn.into(), tooltip)
}

pub(crate) fn wizard_surface_fab<'a>(
    icon: iced::widget::Text<'static, Theme, iced::Renderer>,
    label: String,
    msg: Option<Message>,
) -> Element<'a, Message> {
    wizard_fab(icon, label, msg, fab_surface_style, None)
}

pub(crate) fn wizard_utility_action<'a>(
    icon: iced::widget::Text<'static, Theme, iced::Renderer>,
    label: String,
    msg: Option<Message>,
) -> Element<'a, Message> {
    let mut action = button(fab_icon_content(icon))
        .width(Length::Fixed(48.0))
        .height(Length::Fixed(48.0))
        .padding(0)
        .style(utility_action_style);
    if let Some(msg) = msg {
        action = action.on_press(msg);
    }
    fab_tooltip(action.into(), label)
}

pub(crate) fn wizard_error_utility_action<'a>(
    icon: iced::widget::Text<'static, Theme, iced::Renderer>,
    label: String,
    msg: Option<Message>,
) -> Element<'a, Message> {
    let mut action = button(fab_icon_content(icon))
        .width(Length::Fixed(48.0))
        .height(Length::Fixed(48.0))
        .padding(0)
        .style(utility_error_action_style);
    if let Some(msg) = msg {
        action = action.on_press(msg);
    }
    fab_tooltip(action.into(), label)
}

pub(crate) fn wizard_utility_toolbar<'a>(
    actions: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(actions)
        .padding(4)
        .height(Length::Fixed(WIZARD_FAB_SIZE))
        .style(|t: &Theme| {
            let p = pal_of(t);
            container::Style {
                background: Some(p.surface_container_high.into()),
                border: iced::Border {
                    radius: theme::shape::FULL.into(),
                    ..Default::default()
                },
                // This is a low-emphasis utility island, not a FAB. The
                // labeled and circular FAB shapes both use level 3/4 via
                // `fab_style`; this toolbar deliberately stays at level 1.
                shadow: theme::elevation(1, theme::is_dark(t)),
                ..Default::default()
            }
        })
        .into()
}

pub(crate) fn wizard_primary_extended_fab<'a>(
    icon: iced::widget::Text<'static, Theme, iced::Renderer>,
    label: String,
    msg: Option<Message>,
    disabled_hint: Option<String>,
) -> Element<'a, Message> {
    let mut action = button(
        container(
            row![
                icon.size(20),
                text(label)
                    .size(theme::text_size::LABEL_LARGE)
                    .font(theme::emphasis::bold())
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .height(Length::Fill)
        .center_y(Length::Fill),
    )
    .height(Length::Fixed(WIZARD_FAB_SIZE))
    .padding([0, 20])
    .style(extended_fab_primary_style);
    let enabled = msg.is_some();
    if let Some(msg) = msg {
        action = action.on_press(msg);
    }
    let action = action.into();
    if !enabled && let Some(hint) = disabled_hint {
        return fab_tooltip(action, hint);
    }
    action
}

pub(crate) fn wizard_fab_footer<'a>(
    leading: impl Into<Element<'a, Message>>,
    trailing: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let leading = leading.into();
    let trailing = trailing.into();

    container(
        row![leading, Space::new().width(Length::Fill), trailing]
            .spacing(WIZARD_FAB_SPACING)
            .align_y(iced::Alignment::Center)
            .height(Length::Fixed(WIZARD_FAB_SIZE))
            .width(Length::Fill),
    )
    .padding(iced::Padding {
        top: 8.0,
        right: 24.0,
        bottom: 24.0,
        left: 24.0,
    })
    .width(Length::Fill)
    .height(Length::Fixed(WIZARD_FAB_NAV_HEIGHT))
    .into()
}

pub(crate) fn empty_wizard_nav<'a>() -> Element<'a, Message> {
    Space::new().height(0).into()
}

/// M3 common-button height. Dialog and popup actions were built ad hoc
/// from `padding([6, 18])` at `size(12)`, which lands around 28 px —
/// well under the 40 dp M3 gives a labeled button, and small enough to
/// be a real pointing chore on the copy/download chips. Every action
/// button now goes through [`m3_filled_button`] / [`m3_text_button`] so
/// the size lives in one place.
pub(crate) const M3_BUTTON_HEIGHT: f32 = 40.0;

/// Interior padding for text fields and pick lists. At the default the
/// dropdown options came out around 27 px tall; this puts a 13 px option
/// at ~41 px, in reach of the 48 dp M3 asks of a menu item without
/// making the settings rows tower over their labels.
pub(crate) const M3_FIELD_PADDING: iced::Padding = iced::Padding {
    top: 12.0,
    right: 16.0,
    bottom: 12.0,
    left: 16.0,
};

fn m3_button<'a>(
    label: String,
    style: fn(&Theme, button::Status) -> button::Style,
) -> button::Button<'a, Message> {
    button(
        container(
            text(label)
                .size(theme::text_size::LABEL_LARGE)
                // A localized label must never shred into a per-glyph
                // column when the parent row is tight; let it overflow.
                .wrapping(iced::widget::text::Wrapping::None),
        )
        .height(Length::Fill)
        .center_y(Length::Fill),
    )
    // Vertical padding stays 0 — the fixed height plus the centering
    // container own the vertical metrics.
    .padding(iced::Padding {
        top: 0.0,
        right: M3_BUTTON_H_PADDING,
        bottom: 0.0,
        left: M3_BUTTON_H_PADDING,
    })
    .height(Length::Fixed(M3_BUTTON_HEIGHT))
    .style(style)
}

/// Shared width of the Root wizard's KPM browse card and the picked-file
/// list beneath it, so the two stack as a single centred column.
pub(crate) const KPM_COLUMN_WIDTH: f32 = 280.0;

/// Interior padding of the Dashboard's hero device card. Sized against
/// its `XL_INCREASED` corner via M3's `outer radius - padding = inner
/// radius` rule.
pub(crate) const DEVICE_CARD_PADDING: f32 = 24.0;
/// Inner height of the dashboard device card at the minimum window. Pinned so
/// the empty-state and populated cards are the same size.
pub(crate) const DEVICE_CARD_HEIGHT: f32 = 160.0;

/// Horizontal padding for a common button. M3 Expressive retired the
/// 24 dp small-button padding in favour of 16 dp, which it notes matches
/// the padding of the new size range.
pub(crate) const M3_BUTTON_H_PADDING: f32 = 16.0;

/// Icon-button size. M3 requires extra-small and small icon buttons to
/// carry a pointer target of at least 48x48 even when the painted
/// container is smaller.
///
/// iced paints a button's style across the button's own rect, so a 32 px
/// disc inside a 48 px target would leave the state layer on the
/// invisible box instead of the visible shape. Rather than lose hover
/// feedback, the container *is* the target: 48 px is a size M3 lists for
/// icon buttons, so this stays on the scale instead of inventing one.
pub(crate) const M3_ICON_BUTTON_SIZE: f32 = 48.0;

/// Icon button at the M3 target size, with the glyph centred inside.
/// Hand-built versions of this at two call sites came out at 32 px.
pub(crate) fn m3_icon_button(
    glyph: iced::widget::Text<'static, Theme, iced::Renderer>,
    glyph_size: f32,
    style: fn(&Theme, button::Status) -> button::Style,
) -> button::Button<'static, Message> {
    button(
        container(glyph.size(glyph_size))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fixed(M3_ICON_BUTTON_SIZE))
    .height(Length::Fixed(M3_ICON_BUTTON_SIZE))
    .padding(0)
    .style(style)
}

/// M3 filled button at the common height. Returns the `Button` so the
/// caller still owns `on_press` (several sites gate it on state).
pub(crate) fn m3_filled_button<'a>(label: String) -> button::Button<'a, Message> {
    m3_button(label, md_filled_btn_style)
}

/// M3 text button at the common height — the low-emphasis half of a
/// dialog's action pair.
pub(crate) fn m3_text_button<'a>(label: String) -> button::Button<'a, Message> {
    m3_button(label, md_text_btn_style)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WizardLeadingAction {
    None,
    Back,
    Cancel,
}

fn wizard_nav_fabs<'a>(
    leading_action: WizardLeadingAction,
    next_label: &str,
    can_next: bool,
    disabled_next_hint: Option<String>,
    back_label: &str,
    back_msg: Message,
    next_msg: Message,
) -> Element<'a, Message> {
    let layout = wizard_nav_layout(next_label);
    let mut leading = row![]
        .spacing(WIZARD_FAB_SPACING)
        .align_y(iced::Alignment::Center)
        .height(Length::Fill);

    if layout.grouped_leading {
        let mut utility_actions = row![].spacing(0).align_y(iced::Alignment::Center);
        if leading_action != WizardLeadingAction::None {
            utility_actions = match leading_action {
                WizardLeadingAction::None => utility_actions,
                WizardLeadingAction::Cancel => utility_actions.push(wizard_error_utility_action(
                    icon::fab_cancel(),
                    back_label.to_string(),
                    Some(back_msg),
                )),
                WizardLeadingAction::Back => utility_actions.push(wizard_utility_action(
                    icon::fab_back(),
                    back_label.to_string(),
                    Some(back_msg),
                )),
            };
        }
        if leading_action == WizardLeadingAction::Back {
            utility_actions = utility_actions.push(wizard_error_utility_action(
                icon::fab_cancel(),
                ltbox_core::i18n::tr("btn_cancel").to_string(),
                Some(Message::StartOver),
            ));
        }
        leading = leading.push(wizard_utility_toolbar(utility_actions));
    } else if leading_action != WizardLeadingAction::None {
        leading = match leading_action {
            WizardLeadingAction::None => leading,
            WizardLeadingAction::Cancel => {
                leading.push(wizard_utility_toolbar(wizard_error_utility_action(
                    icon::fab_cancel(),
                    back_label.to_string(),
                    Some(back_msg),
                )))
            }
            WizardLeadingAction::Back => leading.push(wizard_fab(
                icon::fab_back(),
                back_label.to_string(),
                Some(back_msg),
                fab_surface_style,
                None,
            )),
        };
    }

    let mut trailing = row![]
        .spacing(WIZARD_FAB_SPACING)
        .align_y(iced::Alignment::Center)
        .height(Length::Fill);

    if layout.extended_primary {
        trailing = trailing.push(wizard_primary_extended_fab(
            fab_next_icon(next_label),
            next_label.to_string(),
            can_next.then_some(next_msg),
            disabled_next_hint,
        ));
    } else {
        trailing = trailing.push(wizard_fab(
            fab_next_icon(next_label),
            next_label.to_string(),
            can_next.then_some(next_msg),
            fab_primary_style,
            disabled_next_hint,
        ));
    }

    wizard_fab_footer(leading, trailing)
}

pub(crate) fn wizard_nav<'a>(
    can_back: bool,
    next_label: &str,
    can_next: bool,
    back_label: &str,
) -> Element<'a, Message> {
    wizard_nav_fabs(
        if can_back {
            WizardLeadingAction::Back
        } else {
            WizardLeadingAction::None
        },
        next_label,
        can_next,
        None,
        back_label,
        Message::Root(RootMsg::RootBack),
        Message::Root(RootMsg::RootNext),
    )
}

// =========================================================================
// Reusable widgets
// =========================================================================

/// Section header. Renders the label when `expanded` is `true`,
/// otherwise an invisible spacer at the same fixed height — keeps
/// the nav column from re-flowing vertically as the sidebar tween
/// crosses its midpoint.
pub(crate) const SEC_HDR_HEIGHT: f32 = 36.0;

/// Cubic ease-out curve `f(t) = 1 - (1 - t)^3`, mapped to `[0, 1]`.
/// Used by the sidebar tween so labels fade in faster early and
/// settle smoothly near the spring's resting point.
pub(crate) fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Pinned nav button height — matches the expanded label form so
/// the sidebar tween's mid-frame swap between icon-only and
/// label content doesn't push every row vertically.
pub(crate) const NAV_BTN_HEIGHT: f32 = 38.0;

/// Collapsed sidebar rail width (icon-only). The main row reserves
/// exactly this much space so the content area never reflows when the
/// sidebar tweens — the expanded form floats over content via a
/// `Stack` overlay.
pub(crate) const SIDEBAR_RAIL_WIDTH: f32 = 64.0;
pub(crate) const SIDEBAR_EXPANDED_WIDTH: f32 = 210.0;

pub(crate) fn nav_btn<'a>(
    view: View,
    label: &str,
    active: bool,
    enabled: bool,
    label_alpha: f32,
) -> Element<'a, Message> {
    // M3 active indicator: the whole row becomes a `secondary_container`
    // pill (see the button style below), so the icon only has to carry
    // the matching on-color. The previous 32x28 chip wrapped the icon
    // alone, which left the label outside the indicator in the expanded
    // form — M3's navigation drawer puts icon *and* label inside one pill.
    let icon = lucide_icon(view.nav_icon(), 18.0, move |t: &Theme| {
        let p = pal_of(t);
        if !enabled {
            with_alpha(p.on_surface, 0.38)
        } else if active {
            p.on_secondary_container
        } else {
            p.on_surface_variant
        }
    });
    let icon_pill: Element<'a, Message> = container(icon)
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(28.0))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .into();

    // Single base layout in both modes: icon left-anchored + optional
    // label. Keeping the icon's horizontal position constant across
    // modes means it does not jump from "centered in 64 px shell"
    // to "left-padded next to label" the moment the label mounts.
    // Outer padding shrinks from 22 → 15 (= 22 - (32-18)/2) so the
    // pill's geometric center sits at the same x as the bare icon
    // did before, avoiding a horizontal shift the moment a row
    // becomes active.
    let mut inner = iced::widget::row![icon_pill]
        .spacing(8)
        .align_y(iced::Alignment::Center);
    if label_alpha > 0.0 {
        // Resolve the base text color (hover / disabled apply via the
        // button style below; here we just fade the label in along
        // the spring), then re-apply alpha so the glyph fades in step
        // with the sidebar width tween. The active row sits on a
        // `secondary_container` pill, so its label takes the matching
        // on-color; inactive rows sit on the bare panel.
        let alpha = label_alpha;
        let base_label_color = move |t: &Theme| -> iced::Color {
            let p = pal_of(t);
            if !enabled {
                with_alpha(p.on_surface, 0.38)
            } else if active {
                p.on_secondary_container
            } else {
                p.on_surface
            }
        };
        let mut label_text = text(label.to_string())
            .size(theme::text_size::LABEL_LARGE)
            .height(Length::Fill)
            .align_y(iced::alignment::Vertical::Center);
        if active && enabled {
            label_text = label_text.font(theme::emphasis::medium());
        }
        inner = inner.push(
            label_text
                // Forbid wrapping: during the sidebar spring there is
                // a brief window where the panel is wide enough to
                // mount the label but too narrow for long glyphs to
                // fit on one line. Wrapping into 2 rows mid-tween then
                // collapsing back to 1 row reads as a jank flicker.
                // No-wrap lets the text overflow under the panel's
                // clip rect instead — invisible until width settles.
                .wrapping(iced::widget::text::Wrapping::None)
                .style(move |t: &Theme| iced::widget::text::Style {
                    color: Some(with_alpha(base_label_color(t), alpha)),
                }),
        );
    }
    let content: Element<'a, Message> = container(inner)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::Alignment::Center)
        .into();

    // The button paints both the active indicator and the state layer, so
    // it carries a FULL radius: every fill it draws is a pill, never the
    // edge-to-edge square the flat-background version produced. The
    // 12 px side inset lives on the wrapper below; the remaining 3 px of
    // button padding keeps the 32-wide icon slot at the same on-screen x
    // as before (12 + 3 = the previous 15).
    let btn = button(content)
        .padding([0, 3])
        .width(Length::Fill)
        .height(Length::Fixed(NAV_BTN_HEIGHT))
        .style(move |t: &Theme, status| {
            let p = pal_of(t);
            let pill = iced::Border {
                radius: theme::shape::FULL.into(),
                ..Default::default()
            };
            if !enabled {
                return button::Style {
                    background: None,
                    text_color: with_alpha(p.on_surface, 0.38),
                    border: pill,
                    ..Default::default()
                };
            }
            // Active rows carry an opaque `secondary_container` pill;
            // inactive rows are transparent until a state layer lands on
            // them (hover 8%, pressed 12%). Stacking the layer on the
            // active fill keeps an already-selected row responsive to the
            // pointer instead of looking inert.
            let background = if active {
                Some(
                    theme::mix_color(
                        p.secondary_container,
                        p.on_secondary_container,
                        theme::state_alpha(status),
                    )
                    .into(),
                )
            } else {
                theme::state_layer_bg(status, p.on_surface).map(|c| c.into())
            };
            button::Style {
                background,
                text_color: if active {
                    p.on_secondary_container
                } else {
                    p.on_surface_variant
                },
                border: pill,
                ..Default::default()
            }
        });
    let btn: Element<'a, Message> = if enabled {
        btn.on_press(Message::Navigate(view)).into()
    } else {
        btn.into()
    };
    // M3 insets drawer/rail items from the panel edge so the indicator
    // reads as a discrete pill rather than a full-bleed band.
    container(btn).padding([0, 12]).into()
}

// Device portrait handles — built once, cloned each render.
// Unknown models fall through to `GENERIC_TABLET_SVG_HANDLE`.
static LAVIETAB9QHD1_HANDLE: std::sync::LazyLock<iced::widget::image::Handle> =
    std::sync::LazyLock::new(|| {
        iced::widget::image::Handle::from_bytes(
            include_bytes!("../assets/devices/9qhd1.png").as_slice(),
        )
    });
static TB320FC_HANDLE: std::sync::LazyLock<iced::widget::image::Handle> =
    std::sync::LazyLock::new(|| {
        iced::widget::image::Handle::from_bytes(
            include_bytes!("../assets/devices/tb320fc.png").as_slice(),
        )
    });
static TB321FU_HANDLE: std::sync::LazyLock<iced::widget::image::Handle> =
    std::sync::LazyLock::new(|| {
        iced::widget::image::Handle::from_bytes(
            include_bytes!("../assets/devices/tb321fu.png").as_slice(),
        )
    });
static TB322FC_HANDLE: std::sync::LazyLock<iced::widget::image::Handle> =
    std::sync::LazyLock::new(|| {
        iced::widget::image::Handle::from_bytes(
            include_bytes!("../assets/devices/tb322fc.png").as_slice(),
        )
    });
static TB323FU_HANDLE: std::sync::LazyLock<iced::widget::image::Handle> =
    std::sync::LazyLock::new(|| {
        iced::widget::image::Handle::from_bytes(
            include_bytes!("../assets/devices/tb323fu.png").as_slice(),
        )
    });
static TB520FU_HANDLE: std::sync::LazyLock<iced::widget::image::Handle> =
    std::sync::LazyLock::new(|| {
        iced::widget::image::Handle::from_bytes(
            include_bytes!("../assets/devices/tb520fu.png").as_slice(),
        )
    });
static TB710FU_HANDLE: std::sync::LazyLock<iced::widget::image::Handle> =
    std::sync::LazyLock::new(|| {
        iced::widget::image::Handle::from_bytes(
            include_bytes!("../assets/devices/tb710fu.png").as_slice(),
        )
    });
static GENERIC_TABLET_SVG_HANDLE: std::sync::LazyLock<iced::widget::svg::Handle> =
    std::sync::LazyLock::new(|| {
        iced::widget::svg::Handle::from_memory(
            include_bytes!("../assets/devices/generic_tablet.svg").as_slice(),
        )
    });

/// Asset for the Dashboard portrait slot.
pub(crate) enum DevicePortrait {
    Png(iced::widget::image::Handle),
    Svg(iced::widget::svg::Handle),
}

pub(crate) fn device_portrait(model: &str) -> DevicePortrait {
    match model.to_uppercase().as_str() {
        "LAVIETAB9QHD1" => DevicePortrait::Png(LAVIETAB9QHD1_HANDLE.clone()),
        "TB320FC" => DevicePortrait::Png(TB320FC_HANDLE.clone()),
        "TB321FU" => DevicePortrait::Png(TB321FU_HANDLE.clone()),
        "TB322FC" => DevicePortrait::Png(TB322FC_HANDLE.clone()),
        "TB323FU" => DevicePortrait::Png(TB323FU_HANDLE.clone()),
        "TB520FU" => DevicePortrait::Png(TB520FU_HANDLE.clone()),
        "TB710FU" => DevicePortrait::Png(TB710FU_HANDLE.clone()),
        _ => DevicePortrait::Svg(GENERIC_TABLET_SVG_HANDLE.clone()),
    }
}

pub(crate) const WIZARD_CARD_HEIGHT: f32 = 180.0;

/// Side length for the square (1:1) option cards used by single-row wizard
/// steps. Sized so a 3-up row still fits within the minimum window width.
pub(crate) const WIZARD_CARD_SQUARE: f32 = 200.0;
pub(crate) const WIZARD_CARD_ICON: f32 = 57.6;
pub(crate) const WIZARD_CARD_ICON_MAX: f32 = 86.4;
/// Title and description sizes the square cards grow toward, on the same
/// curve as the card itself.
pub(crate) const WIZARD_CARD_TITLE_MAX: f32 = 20.0;
pub(crate) const WIZARD_CARD_DESC_MAX: f32 = 14.0;
/// Larger square-card side used once the window has enough horizontal room.
pub(crate) const WIZARD_CARD_SQUARE_MAX: f32 = 300.0;
/// Content width at which cards are still at their minimum size, and the one
/// where they reach `WIZARD_CARD_SQUARE_MAX`. 756 is the content area of the
/// 820 px minimum window.
pub(crate) const WIZARD_CARD_GROW_FROM_CONTENT: f32 = 756.0;
pub(crate) const WIZARD_CARD_GROW_TO_CONTENT: f32 = 1600.0;
#[derive(Debug, Clone, Copy)]
pub(crate) struct ListRowMetrics {
    pub(crate) height: f32,
    pub(crate) label_size: f32,
    pub(crate) desc_size: f32,
}

pub(crate) const WIZARD_LIST_CARD_HEIGHT: f32 = 72.0;
/// Row height the single-column lists grow to, keeping the enlarged icon inside
/// its padding.
pub(crate) const WIZARD_LIST_CARD_HEIGHT_MAX: f32 = 96.0;
/// Single-column lists widen by this much across the growth range; icons in
/// them grow on the same 1.5x curve the square-card icon uses.
pub(crate) const WIZARD_LIST_GROWTH: f32 = 1.32;
pub(crate) const WIZARD_ICON_GROWTH: f32 = 1.5;
/// Growth ratios for [`Density`], one per element class.
///
/// They are deliberately unequal. Text carries a reading burden, so scaling it
/// with the window as hard as an image would just read as a zoomed screenshot;
/// images and icons carry none, and are what leave a wide window looking empty
/// when they stay at the size that suited the minimum one.
pub(crate) const TEXT_GROWTH: f32 = 1.25;
pub(crate) const IMAGE_GROWTH: f32 = 1.5;
pub(crate) const SPACE_GROWTH: f32 = 1.4;
pub(crate) const SIZE_GROWTH: f32 = 1.3;
pub(crate) const WIDTH_GROWTH: f32 = 1.32;
pub(crate) const WIZARD_LIST_MAX_WIDTH: f32 = 620.0;
pub(crate) const WIZARD_CONFIRM_MAX_WIDTH: f32 = 660.0;
pub(crate) const WIZARD_TOP_APP_BAR_HEIGHT: f32 = 132.0;
pub(crate) const WIZARD_TOP_APP_BAR_MAX_WIDTH: f32 = 1040.0;
pub(crate) const WIZARD_FAB_SIZE: f32 = 56.0;
pub(crate) const WIZARD_FAB_SPACING: f32 = 12.0;
pub(crate) const WIZARD_FAB_NAV_HEIGHT: f32 = 88.0;
pub(crate) const ADVANCED_GRID_MAX_WIDTH: f32 = 860.0;
pub(crate) const SETTINGS_PANEL_MAX_WIDTH: f32 = 620.0;

/// Fixed sub-row height (~2 lines at size 11) so cards line up across
/// translations.
pub(crate) const SUB_ROW_HEIGHT: f32 = 32.0;

/// Taller sub-row for the narrower square cards so longer localized
/// descriptions wrap without clipping. The widest bundled string reaches
/// three lines at `BODY_SMALL` inside the narrowest card, needing ~49 px.
pub(crate) const WIZARD_CARD_SQUARE_SUB_HEIGHT: f32 = 60.0;

pub(crate) const FLASH_PARTS_MARKER_CELL_WIDTH: f32 = 32.0;
pub(crate) const FLASH_PARTS_MARKER_CELL_HEIGHT: f32 = 20.0;
pub(crate) const FLASH_PARTS_MARKER_SIZE: f32 = 16.0;
pub(crate) const FLASH_PARTS_ERASE_DASH_WIDTH: f32 = 9.0;
pub(crate) const FLASH_PARTS_ERASE_DASH_HEIGHT: f32 = 2.0;

pub(crate) fn centered_max_width<'a>(
    content: impl Into<Element<'a, Message>>,
    max_width: f32,
) -> Element<'a, Message> {
    container(
        container(content.into())
            .width(Length::Fill)
            .max_width(max_width),
    )
    .width(Length::Fill)
    .center_x(Length::Fill)
    .into()
}

pub(crate) fn centered_step<'a>(
    content: impl Into<Element<'a, Message>>,
    max_width: f32,
) -> Element<'a, Message> {
    container(
        container(content.into())
            .width(Length::Fill)
            .max_width(max_width),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

/// The one adaptive-sizing rule for the whole app.
///
/// Every view that wants to answer "how big should this be on a large window"
/// asks a `Density` instead of inventing its own breakpoint — that drift is
/// what left the wizard growing while the dashboard, settings and about panels
/// stayed frozen at their minimum-window sizes.
///
/// `t` runs 0.0 at the content width of the 820 px minimum window and reaches
/// 1.0 at [`WIZARD_CARD_GROW_TO_CONTENT`], past which more window only buys
/// margin. It keys on width alone: a size that changed when the user dragged
/// only the height would be far more surprising than one that did not.
///
/// Each method applies the growth ratio its element class is allowed, so a
/// caller picks *what kind of thing* it is sizing rather than a magic number.
/// Dialogs are the deliberate exception — they are centred and fixed-width, so
/// they never look emptier on a big window and stay at [`Density::MIN`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct Density {
    t: f32,
}

impl Density {
    /// Minimum-window density. For dialogs and for tests.
    pub(crate) const MIN: Self = Self { t: 0.0 };

    /// Text sizes.
    pub(crate) fn text(self, base: f32) -> f32 {
        self.by(base, TEXT_GROWTH)
    }

    /// Icons, images and device portraits.
    pub(crate) fn image(self, base: f32) -> f32 {
        self.by(base, IMAGE_GROWTH)
    }

    /// Padding and gaps, so grown content does not crowd a card edge.
    pub(crate) fn space(self, base: f32) -> f32 {
        self.by(base, SPACE_GROWTH)
    }

    /// Fixed card and row heights, and other hit-target boxes.
    pub(crate) fn size(self, base: f32) -> f32 {
        self.by(base, SIZE_GROWTH)
    }

    /// Width cap of a centred panel or list.
    pub(crate) fn width(self, base: f32) -> f32 {
        self.by(base, WIDTH_GROWTH)
    }

    /// Symmetric padding, both axes scaled as spacing.
    pub(crate) fn padding(self, vertical: f32, horizontal: f32) -> iced::Padding {
        iced::Padding::default()
            .top(self.space(vertical))
            .bottom(self.space(vertical))
            .left(self.space(horizontal))
            .right(self.space(horizontal))
    }

    /// Scale an existing [`iced::Padding`] constant as spacing.
    pub(crate) fn scale_padding(self, base: iced::Padding) -> iced::Padding {
        iced::Padding {
            top: self.space(base.top),
            right: self.space(base.right),
            bottom: self.space(base.bottom),
            left: self.space(base.left),
        }
    }

    /// Interpolate between an explicit pair when a dimension has a chosen
    /// maximum rather than a ratio.
    pub(crate) fn between(self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.t
    }

    fn by(self, base: f32, growth: f32) -> f32 {
        self.between(base, base * growth)
    }
}

/// [`Density`] for a given content width — the width of the area left of the
/// sidebar rail.
pub(crate) fn density_for_content_width(content_width: f32) -> Density {
    let span = WIZARD_CARD_GROW_TO_CONTENT - WIZARD_CARD_GROW_FROM_CONTENT;
    Density {
        t: ((content_width - WIZARD_CARD_GROW_FROM_CONTENT) / span).clamp(0.0, 1.0),
    }
}

impl App {
    /// The current [`Density`]. Resolve it once per view and pass it down;
    /// recomputing it per widget is what lets two halves of one screen
    /// disagree.
    pub(crate) fn density(&self) -> Density {
        density_for_content_width(self.window_size.0 - SIDEBAR_RAIL_WIDTH)
    }

    /// Width cap for the single-column wizard lists (root families, reboot
    /// targets), on the same curve as the square cards.
    pub(crate) fn wizard_list_max_width(&self, base: f32) -> f32 {
        self.density().by(base, WIZARD_LIST_GROWTH)
    }

    /// The adaptive dimensions of one single-column list row, resolved together
    /// so a caller cannot mix a scaled height with unscaled text.
    pub(crate) fn wizard_list_metrics(&self, label_base: f32, desc_base: f32) -> ListRowMetrics {
        let (label_size, desc_size) = self.wizard_list_text(label_base, desc_base);
        ListRowMetrics {
            height: self.wizard_list_row_height(),
            label_size,
            desc_size,
        }
    }

    pub(crate) fn wizard_list_row_height(&self) -> f32 {
        self.density()
            .between(WIZARD_LIST_CARD_HEIGHT, WIZARD_LIST_CARD_HEIGHT_MAX)
    }

    /// Icon size for a single-column list row, scaled from its own base so the
    /// root list (44) and the reboot list (32) keep their relative weights.
    pub(crate) fn wizard_list_icon(&self, base: f32) -> f32 {
        self.density().by(base, WIZARD_ICON_GROWTH)
    }

    /// Label and description sizes for a list row, from that row's own bases.
    pub(crate) fn wizard_list_text(&self, label_base: f32, desc_base: f32) -> (f32, f32) {
        let d = self.density();
        (
            d.between(label_base, label_base.max(WIZARD_CARD_TITLE_MAX)),
            d.between(desc_base, desc_base.max(WIZARD_CARD_DESC_MAX)),
        )
    }

    pub(crate) fn wizard_square_side(&self) -> f32 {
        // Grow with the window rather than stepping once and then staying flat.
        // At the minimum window the cards are already sized to look right, and
        // past `WIZARD_CARD_GROW_TO_CONTENT` further growth would only pad the
        // icon and label they contain.
        self.density()
            .between(WIZARD_CARD_SQUARE, WIZARD_CARD_SQUARE_MAX)
    }

    pub(crate) fn wizard_square_icon(&self) -> f32 {
        self.density()
            .between(WIZARD_CARD_ICON, WIZARD_CARD_ICON_MAX)
    }

    pub(crate) fn square_step_max_width(&self, columns: usize) -> f32 {
        let columns = columns.max(1) as f32;
        let gaps = (columns - 1.0) * 12.0;
        // The column that owns card rows uses 28 px horizontal padding.
        columns * self.wizard_square_side() + gaps + 56.0
    }
}

pub(crate) fn wizard_nav_generic<'a>(
    can_back: bool,
    next_label: &str,
    can_next: bool,
    back_label: &str,
    back_msg: Message,
    next_msg: Message,
) -> Element<'a, Message> {
    wizard_nav_generic_with_disabled_next_tooltip(
        can_back, next_label, can_next, None, back_label, back_msg, next_msg,
    )
}

pub(crate) fn wizard_nav_generic_with_leading_action<'a>(
    leading_action: WizardLeadingAction,
    next_label: &str,
    can_next: bool,
    leading_label: &str,
    leading_msg: Message,
    next_msg: Message,
) -> Element<'a, Message> {
    wizard_nav_fabs(
        leading_action,
        next_label,
        can_next,
        None,
        leading_label,
        leading_msg,
        next_msg,
    )
}

pub(crate) fn wizard_nav_generic_with_disabled_next_tooltip<'a>(
    can_back: bool,
    next_label: &str,
    can_next: bool,
    disabled_next_hint: Option<String>,
    back_label: &str,
    back_msg: Message,
    next_msg: Message,
) -> Element<'a, Message> {
    wizard_nav_fabs(
        if can_back {
            WizardLeadingAction::Back
        } else {
            WizardLeadingAction::None
        },
        next_label,
        can_next,
        disabled_next_hint,
        back_label,
        back_msg,
        next_msg,
    )
}

pub(crate) fn wizard_nav_cancel_generic_with_disabled_next_tooltip<'a>(
    next_label: &str,
    can_next: bool,
    disabled_next_hint: Option<String>,
    cancel_label: &str,
    cancel_msg: Message,
    next_msg: Message,
) -> Element<'a, Message> {
    wizard_nav_fabs(
        WizardLeadingAction::Cancel,
        next_label,
        can_next,
        disabled_next_hint,
        cancel_label,
        cancel_msg,
        next_msg,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        App, Density, IMAGE_GROWTH, MaterialProgressSize, WIZARD_CARD_GROW_FROM_CONTENT,
        WIZARD_CARD_GROW_TO_CONTENT, WIZARD_CARD_ICON_MAX, WIZARD_CARD_SQUARE,
        WIZARD_CARD_SQUARE_MAX, density_for_content_width, extended_fab_primary_style,
        fab_elevation_level, fab_primary_style, fab_surface_style, material_progress_arc,
        material_progress_gap_angle, material_progress_metrics, wizard_nav_layout,
    };
    use iced::widget::button;

    #[test]
    fn wizard_square_contents_track_card_growth_endpoints() {
        let app = App::default();

        assert_eq!(app.wizard_square_side(), WIZARD_CARD_SQUARE);
        assert_eq!(app.wizard_square_icon(), 57.6);

        let mut maximized = App::default();
        maximized.window_size.0 = WIZARD_CARD_GROW_TO_CONTENT + crate::SIDEBAR_RAIL_WIDTH;
        assert_eq!(maximized.wizard_square_side(), WIZARD_CARD_SQUARE_MAX);
        assert_eq!(maximized.wizard_square_icon(), WIZARD_CARD_ICON_MAX);
    }

    #[test]
    fn material_progress_metrics_match_m3_tokens() {
        let standard = material_progress_metrics(MaterialProgressSize::Standard);
        assert_eq!(standard.diameter, 40.0);
        assert_eq!(standard.stroke_width, 4.0);
        assert_eq!(standard.track_gap, 4.0);

        let hero = material_progress_metrics(MaterialProgressSize::Hero);
        assert_eq!(hero.diameter, 52.0);
        assert_eq!(hero.stroke_width, 8.0);
        assert_eq!(hero.track_gap, 4.0);
    }

    #[test]
    fn material_progress_arc_wraps_phase_and_bounds_sweep() {
        let start = material_progress_arc(0.0);
        let wrapped = material_progress_arc(1.0);
        assert!((start.start_angle - wrapped.start_angle).abs() < f32::EPSILON);
        assert!((start.sweep_angle - wrapped.sweep_angle).abs() < f32::EPSILON);

        for phase in [0.0, 0.125, 0.25, 0.5, 0.75, 0.999] {
            let arc = material_progress_arc(phase);
            assert!(arc.sweep_angle >= std::f32::consts::TAU * 0.12);
            assert!(arc.sweep_angle <= std::f32::consts::TAU * 0.67);
        }
    }

    #[test]
    fn material_progress_gap_accounts_for_round_caps() {
        for size in [MaterialProgressSize::Standard, MaterialProgressSize::Hero] {
            let metrics = material_progress_metrics(size);
            let radius = (metrics.diameter - metrics.stroke_width) / 2.0;
            let centerline_gap = material_progress_gap_angle(metrics, radius) * radius;
            assert_eq!(centerline_gap, metrics.track_gap + metrics.stroke_width);
        }
    }

    #[test]
    fn all_fab_shapes_share_the_same_elevation_policy() {
        assert_eq!(fab_elevation_level(button::Status::Active), 3);
        assert_eq!(fab_elevation_level(button::Status::Hovered), 4);
        assert_eq!(fab_elevation_level(button::Status::Pressed), 3);
        assert_eq!(fab_elevation_level(button::Status::Disabled), 0);

        let theme = iced::Theme::custom(
            "test",
            crate::theme::iced_palette(crate::theme::ThemeSeed::Indigo, false),
        );
        for status in [
            button::Status::Active,
            button::Status::Hovered,
            button::Status::Pressed,
        ] {
            let circular_shadow = fab_primary_style(&theme, status).shadow;
            assert_eq!(
                extended_fab_primary_style(&theme, status).shadow,
                circular_shadow
            );
            assert_eq!(fab_surface_style(&theme, status).shadow, circular_shadow);
        }
    }

    #[test]
    fn no_fab_shape_draws_an_outline_in_any_state() {
        // M3 gives the FAB no `outline-*` token and no disabled state at
        // all, so an outline has nothing to derive its width or colour
        // from. Disabled is separated by elevation 0 and the 38% content
        // alpha instead.
        let theme = iced::Theme::custom(
            "test",
            crate::theme::iced_palette(crate::theme::ThemeSeed::Indigo, false),
        );
        for status in [
            button::Status::Active,
            button::Status::Hovered,
            button::Status::Pressed,
            button::Status::Disabled,
        ] {
            for style in [
                fab_primary_style(&theme, status),
                fab_surface_style(&theme, status),
                extended_fab_primary_style(&theme, status),
            ] {
                assert_eq!(style.border.width, 0.0);
            }
        }
    }

    #[test]
    fn wizard_nav_layout_groups_confirmation_actions() {
        for key in ["btn_start", "btn_dump"] {
            let label = ltbox_core::i18n::tr(key);
            let layout = wizard_nav_layout(label.as_str());
            assert!(layout.grouped_leading);
            assert!(layout.extended_primary);
        }

        let next_label = ltbox_core::i18n::tr("btn_next");
        let next = wizard_nav_layout(next_label.as_str());
        assert!(!next.grouped_leading);
        assert!(next.extended_primary);
    }

    #[test]
    fn density_leaves_the_minimum_window_exactly_as_authored() {
        // Views declare the sizes that were tuned at the 820 px minimum, so a
        // minimum-width window must hand every one of them straight back.
        let min = density_for_content_width(WIZARD_CARD_GROW_FROM_CONTENT);
        for base in [11.0, 14.0, 40.0, 160.0, 620.0] {
            assert_eq!(min.text(base), base);
            assert_eq!(min.image(base), base);
            assert_eq!(min.space(base), base);
            assert_eq!(min.size(base), base);
            assert_eq!(min.width(base), base);
        }
        assert_eq!(Density::MIN.text(14.0), 14.0);
        // Narrower than the minimum cannot shrink anything.
        assert_eq!(density_for_content_width(300.0).text(14.0), 14.0);
    }

    #[test]
    fn density_grows_element_classes_in_their_intended_order() {
        let full = density_for_content_width(WIZARD_CARD_GROW_TO_CONTENT);
        let base = 100.0;
        // Text carries a reading burden, so it grows least; images carry none
        // and are what leave a wide window looking empty, so they grow most.
        assert!(full.text(base) < full.size(base));
        assert!(full.size(base) < full.width(base));
        assert!(full.width(base) < full.space(base));
        assert!(full.space(base) < full.image(base));
        assert_eq!(full.image(base), base * IMAGE_GROWTH);
        // Past the top of the range growth stops rather than running away.
        assert_eq!(
            density_for_content_width(4000.0).image(base),
            base * IMAGE_GROWTH
        );
    }
}
