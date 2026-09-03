//! Shared dimensions for localized copy rendered in constrained slots.
//!
//! `tests/locale_guards.rs` imports this module directly so the regression
//! guard measures against the same budgets as the production widgets.

pub(crate) const WIZARD_CARD_SQUARE: f32 = 240.0;
pub(crate) const WIZARD_CARD_ICON: f32 = 57.6;
pub(crate) const WIZARD_CARD_ICON_MAX: f32 = 86.4;
pub(crate) const WIZARD_CARD_TITLE_SIZE: f32 = 16.0;
pub(crate) const WIZARD_CARD_DESC_SIZE: f32 = 12.0;
pub(crate) const WIZARD_CARD_TITLE_MAX: f32 = 20.0;
pub(crate) const WIZARD_CARD_DESC_MAX: f32 = 14.0;
pub(crate) const WIZARD_CARD_SQUARE_MAX: f32 = 300.0;
pub(crate) const WIZARD_CARD_HORIZONTAL_PADDING: f32 = 16.0;
pub(crate) const WIZARD_CARD_VERTICAL_PADDING: f32 = 20.0;
pub(crate) const WIZARD_CARD_ICON_TITLE_GAP: f32 = 14.0;
pub(crate) const WIZARD_CARD_TITLE_DESC_GAP: f32 = 4.0;
pub(crate) const WIZARD_CARD_SQUARE_SUB_HEIGHT: f32 = 60.0;

pub(crate) const WIZARD_LIST_CARD_HEIGHT: f32 = 72.0;
pub(crate) const WIZARD_LIST_MAX_WIDTH: f32 = 620.0;
pub(crate) const WIZARD_LIST_ICON_SIZE: f32 = 44.0;
pub(crate) const WIZARD_LIST_LABEL_SIZE: f32 = 16.0;
pub(crate) const WIZARD_LIST_DESC_SIZE: f32 = 12.0;
pub(crate) const WIZARD_LIST_VERTICAL_PADDING: f32 = 10.0;
pub(crate) const WIZARD_LIST_HORIZONTAL_PADDING: f32 = 16.0;
pub(crate) const WIZARD_LIST_TEXT_GAP: f32 = 3.0;
pub(crate) const WIZARD_LIST_ICON_GAP: f32 = 16.0;
pub(crate) const WIZARD_STEP_HORIZONTAL_PADDING: f32 = 28.0;

pub(crate) const SETTINGS_PICK_LIST_WIDTH: f32 = 168.0;
pub(crate) const SETTINGS_PICK_LIST_TEXT_SIZE: f32 = 13.0;

pub(crate) const M3_FIELD_PADDING: iced::Padding = iced::Padding {
    top: 12.0,
    right: 16.0,
    bottom: 12.0,
    left: 16.0,
};

pub(crate) const M3_BUTTON_H_PADDING: f32 = 16.0;

pub(crate) const REGION_TARGET_POPUP_WIDTH: f32 = 400.0;
pub(crate) const REGION_TARGET_POPUP_PADDING: f32 = 20.0;
pub(crate) const REGION_TARGET_POPUP_TITLE_SIZE: f32 = 16.0;
pub(crate) const REGION_TARGET_POPUP_ACTION_SIZE: f32 = 14.0;
