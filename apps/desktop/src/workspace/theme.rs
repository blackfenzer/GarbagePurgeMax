//! The two selectable app themes and the file-type colours they share.

use std::fs;

use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{App, Hsla, rgb};

use super::storage;

const THEMES: &str = include_str!("../../themes/storage-clinic.json");

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

/// Treemap and legend colour for a `scan_engine::FILE_TYPES` name. Both themes are dark,
/// so one light palette with dark labels keeps every type readable in either.
pub(super) fn type_color(file_type: &str) -> Hsla {
    rgb(match file_type {
        "Video" => 0x8fb3ff,
        "Images" => 0xf2b880,
        "Audio" => 0xf29fb8,
        "Documents" => 0xc3cddd,
        "Archives & disk images" => 0xe8d48a,
        "Apps & installers" => 0xc9a7ff,
        "Code & data" => 0x9ed8ce,
        _ => 0x7d8799,
    })
    .into()
}

/// Dark text drawn on a `type_color` block.
pub(super) fn on_type_color() -> Hsla {
    rgb(0x121620).into()
}
