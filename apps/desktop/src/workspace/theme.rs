//! The two selectable app themes and their file-type colours.

use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{App, Div, Hsla, ParentElement as _, Styled as _, div, px, rgb};

use super::storage;

const THEMES: &str = include_str!("../../themes/storage-clinic.json");

/// Whether Amber Deck's palette is active. File-type colours are drawn from many views
/// that only see the GPUI theme, so the applied theme records its palette here.
static AMBER_PALETTE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum AppTheme {
    #[default]
    Workbench,
    AmberDeck,
}

impl AppTheme {
    pub(super) const ALL: [Self; 2] = [Self::Workbench, Self::AmberDeck];

    /// Also the theme's name in `themes/storage-clinic.json` and the saved choice.
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Workbench => "Workbench",
            Self::AmberDeck => "Amber Deck",
        }
    }

    /// A round swatch of the theme: its background with an accent dot, for the picker.
    pub(super) fn swatch(self, cx: &App) -> Div {
        let (background, accent) = match self {
            Self::Workbench => (0x171b26, 0xb9abff),
            Self::AmberDeck => (0x0d0d0c, 0xf5b301),
        };
        div()
            .flex()
            .items_center()
            .justify_center()
            .size(px(18.0))
            .rounded_full()
            .border_1()
            .border_color(cx.theme().border)
            .bg(rgb(background))
            .child(div().size(px(8.0)).rounded_full().bg(rgb(accent)))
    }

    /// The saved choice, or Workbench when none is saved or it cannot be read.
    pub(super) fn saved() -> Self {
        let saved = storage::database_path()
            .and_then(|database| fs::read_to_string(database.with_file_name("theme")));
        saved
            .ok()
            .and_then(|name| {
                Self::ALL
                    .into_iter()
                    .find(|theme| theme.label() == name.trim())
            })
            .unwrap_or_default()
    }

    /// Applies the theme to every window, then saves it as the next launch's choice.
    pub(super) fn apply(self, cx: &mut App) {
        if ThemeRegistry::global(cx)
            .themes()
            .get(self.label())
            .is_none()
            && let Err(error) = ThemeRegistry::global_mut(cx).load_themes_from_str(THEMES)
        {
            log::error!("Could not load the app themes: {error}");
            return;
        }
        let Some(config) = ThemeRegistry::global(cx)
            .themes()
            .get(self.label())
            .cloned()
        else {
            log::error!("The {} theme is missing", self.label());
            return;
        };
        Theme::global_mut(cx).apply_config(&config);
        Theme::change(ThemeMode::Dark, None, cx);
        AMBER_PALETTE.store(self == Self::AmberDeck, Ordering::Relaxed);

        let saved = storage::database_path().and_then(|database| {
            fs::create_dir_all(
                database
                    .parent()
                    .expect("database has an app data directory"),
            )?;
            fs::write(database.with_file_name("theme"), self.label())
        });
        if let Err(error) = saved {
            log::warn!("Could not save the theme choice: {error}");
        }
    }
}

/// Treemap and legend colour for a `scan_engine::FILE_TYPES` name. Amber Deck uses warm
/// earth tones ("Ember") that sit with its amber accent; Workbench uses deeper, desaturated
/// jewel tones ("Slate jewel") that suit its slate and lavender.
pub(super) fn type_color(file_type: &str) -> Hsla {
    let amber = AMBER_PALETTE.load(Ordering::Relaxed);
    let (ember, slate) = match file_type {
        "Video" => (0xd9824a, 0x4f7fb8),
        "Images" => (0xe2b04a, 0xc9803f),
        "Audio" => (0xc9677a, 0xb0617f),
        "Documents" => (0xb8ad98, 0x6b7a90),
        "Archives & disk images" => (0xb39a5c, 0xa8873a),
        "Apps & installers" => (0xb07fa8, 0x7d68b5),
        "Code & data" => (0x7fae9a, 0x3f9a8a),
        _ => (0x6e6a60, 0x4d5463),
    };
    rgb(if amber { ember } else { slate }).into()
}

/// Text drawn on a `type_color` block: dark ink on Ember, light ink on Slate jewel.
pub(super) fn on_type_color() -> Hsla {
    rgb(if AMBER_PALETTE.load(Ordering::Relaxed) {
        0x1a1612
    } else {
        0xf4f1ea
    })
    .into()
}
