//! Scan and analysis preferences, saved as `key=value` lines beside the scan database.

use std::fs;
use std::thread;

use scan_engine::{DEFAULT_IMAGE_WORKERS, ScanScope};

use super::storage;

const FILE_NAME: &str = "preferences";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Preferences {
    /// Threads that decode images for the similar-image check.
    pub analysis_workers: usize,
    /// Skip project dependencies and build output in broad scans.
    pub smart_scope: bool,
    /// Start duplicate and similar-image analysis as soon as indexing is saved.
    pub auto_analyze: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            analysis_workers: DEFAULT_IMAGE_WORKERS,
            smart_scope: true,
            auto_analyze: true,
        }
    }
}

impl Preferences {
    /// The saved preferences, with defaults for anything missing or unreadable.
    pub(super) fn load() -> Self {
        let mut preferences = Self::default();
        let Ok(text) = storage::database_path()
            .and_then(|database| fs::read_to_string(database.with_file_name(FILE_NAME)))
        else {
            return preferences;
        };
        for (key, value) in text.lines().filter_map(|line| line.split_once('=')) {
            match (key.trim(), value.trim()) {
                ("analysis_workers", workers) => {
                    if let Ok(workers) = workers.parse::<usize>() {
                        preferences.analysis_workers = workers.clamp(1, max_workers());
                    }
                }
                ("smart_scope", flag) => preferences.smart_scope = flag != "0",
                ("auto_analyze", flag) => preferences.auto_analyze = flag != "0",
                _ => {}
            }
        }
        preferences
    }

    pub(super) fn save(&self) {
        let text = format!(
            "analysis_workers={}\nsmart_scope={}\nauto_analyze={}\n",
            self.analysis_workers,
            u8::from(self.smart_scope),
            u8::from(self.auto_analyze)
        );
        let saved = storage::database_path().and_then(|database| {
            fs::create_dir_all(
                database
                    .parent()
                    .expect("database has an app data directory"),
            )?;
            fs::write(database.with_file_name(FILE_NAME), text)
        });
        if let Err(error) = saved {
            log::warn!("Could not save preferences: {error}");
        }
    }

    pub(super) fn scope(&self) -> ScanScope {
        if self.smart_scope {
            ScanScope::Smart
        } else {
            ScanScope::Everything
        }
    }
}

/// The most analysis workers worth offering: one per CPU core.
pub(super) fn max_workers() -> usize {
    thread::available_parallelism().map_or(1, usize::from)
}
