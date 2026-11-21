//! Off-screen UI smoke run: scans a synthetic folder, opens every page in both themes,
//! checks Space Map, and saves screenshots for review. It renders into a Metal texture,
//! so it needs no screen recording permission.
//!
//! Built only with the `ui-smoke` feature and started by `tests/ui_smoke.rs`, because the
//! macOS platform must be created on the main thread, which libtest does not provide.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{AnyWindowHandle, AppContext as _, VisualTestAppContext, WindowHandle, px, size};

use super::{Page, Workspace, theme::AppTheme};

const PAGES: [(Page, &str); 7] = [
    (Page::Overview, "overview"),
    (Page::SpaceMap, "space-map"),
    (Page::Results, "results"),
    (Page::Review, "review"),
    (Page::History, "history"),
    (Page::Cleanup, "cleanup-queue"),
    (Page::Recovery, "recovery"),
];

/// Runs the smoke check and writes `<theme>-<page>.png` files into `shots`.
/// Refuses to run unless `STORAGE_CLINIC_DATA_DIR` points away from the real app data.
pub(crate) fn run(shots: &Path) -> Result<(), Box<dyn Error>> {
    if std::env::var_os("STORAGE_CLINIC_DATA_DIR").is_none_or(|data| data.is_empty()) {
        return Err(
            "set STORAGE_CLINIC_DATA_DIR so the smoke run cannot touch real app data".into(),
        );
    }
    let fixture = tempfile::tempdir()?;
    let root = fixture.path().join("you");
    write_fixture(&root)?;
    fs::create_dir_all(shots)?;

    let platform = gpui_kit::platform::current_platform(false);
    let mut cx =
        VisualTestAppContext::with_asset_source(platform, Arc::new(gpui_kit::assets::Assets));
    cx.update(gpui_kit::init);
    let window: WindowHandle<Workspace> = cx
        .open_offscreen_window(size(px(1280.0), px(820.0)), |window, cx| {
            cx.new(|cx| Workspace::new(window, cx))
        })?;
    settle(&mut cx);

    window.update(&mut cx, |workspace, _, cx| {
        workspace.scan_again(root.clone(), cx)
    })?;
    let mut finished = false;
    for _ in 0..600 {
        settle(&mut cx);
        if window.update(&mut cx, |workspace, _, _| workspace.scan.active.is_none())? {
            finished = true;
            break;
        }
    }
    ensure(finished, "the scan finished")?;
    let (scan_id, error) = window.update(&mut cx, |workspace, _, _| {
        (
            workspace.results.saved_scan().map(|scan| scan.id),
            workspace.scan.error.clone(),
        )
    })?;
    ensure(error.is_none(), &format!("the scan succeeded: {error:?}"))?;
    ensure(scan_id.is_some(), "the scan was saved")?;

    for theme in AppTheme::ALL {
        window.update(&mut cx, |workspace, _, cx| {
            workspace.theme = theme;
            theme.apply(cx);
        })?;
        for (page, name) in PAGES {
            window.update(&mut cx, |workspace, _, cx| {
                workspace.page = page;
                workspace.deck_left = Page::SpaceMap;
                workspace.load_page(page, cx);
            })?;
            settle(&mut cx);
            let file = shots.join(format!("{}-{name}.png", slug(theme.label())));
            capture(&mut cx, window.into(), &file)?;
        }
    }

    // Space Map totals the scan, lists the largest folder first and opens it on request.
    let (map_scan, first, folder) = window.update(&mut cx, |workspace, _, cx| {
        let map = &mut workspace.space_map;
        let map_scan = map.scan_id();
        let Some(summary) = &map.summary else {
            return (map_scan, None, None);
        };
        let first = summary.entries.first().map(|entry| entry.name.clone());
        if let Some(movies) = summary
            .entries
            .iter()
            .position(|entry| entry.name == "Movies")
        {
            map.choose(movies, cx);
        }
        (map_scan, first, Some(map.folder.clone()))
    })?;
    ensure(map_scan == scan_id, "Space Map shows the saved scan")?;
    ensure(
        first.as_deref() == Some("Movies".as_ref()),
        &format!("Movies is the largest entry, found {first:?}"),
    )?;
    ensure(
        folder == Some(root.join("Movies")),
        "choosing Movies opens it",
    )?;
    settle(&mut cx);
    let (opened, years) = window.update(&mut cx, |workspace, _, _| {
        let map = &workspace.space_map;
        (
            map.summary.as_ref().map(|summary| summary.entries.len()),
            map.years.len(),
        )
    })?;
    ensure(
        opened == Some(2),
        &format!("Movies holds two entries, found {opened:?}"),
    )?;
    ensure(years > 0, "the footer's year chart has data")?;
    capture(&mut cx, window.into(), &shots.join("amber-deck-movies.png"))?;
    Ok(())
}

fn ensure(condition: bool, what: &str) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(format!("UI smoke check failed: {what}").into())
    }
}

/// Lets background loads finish and scan progress polling run.
fn settle(cx: &mut VisualTestAppContext) {
    for _ in 0..5 {
        cx.run_until_parked();
        cx.advance_clock(Duration::from_millis(100));
    }
    cx.run_until_parked();
}

fn capture(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    file: &Path,
) -> Result<(), Box<dyn Error>> {
    let image = cx.capture_screenshot(window)?;
    let distinct: std::collections::HashSet<_> = image.pixels().step_by(97).copied().collect();
    ensure(
        distinct.len() > 8,
        &format!("{} rendered more than a blank fill", file.display()),
    )?;
    image.save(file)?;
    Ok(())
}

fn slug(label: &str) -> String {
    label.to_ascii_lowercase().replace(' ', "-")
}

/// A small synthetic home folder with duplicates, similar images and cleanup candidates.
fn write_fixture(root: &Path) -> std::io::Result<()> {
    let file = |path: &str, bytes: Vec<u8>| -> std::io::Result<PathBuf> {
        let path = root.join(path);
        fs::create_dir_all(path.parent().expect("fixture files have parents"))?;
        fs::write(&path, bytes)?;
        Ok(path)
    };
    file("Movies/trip.mov", vec![7; 900_000])?;
    file("Movies/Old/trip copy.mov", vec![7; 900_000])?;
    file("Downloads/Tool.dmg", vec![3; 400_000])?;
    file("Downloads/movie.mp4.crdownload", vec![4; 120_000])?;
    file("Documents/notes.txt", b"synthetic notes".to_vec())?;
    file("Documents/empty.txt", Vec::new())?;
    file(".DS_Store", vec![0; 6])?;
    for (name, shade) in [("sunrise.png", 0u8), ("sunrise-copy.png", 2)] {
        let image = image::RgbImage::from_fn(64, 48, |x, _| {
            let value = (x as u8).saturating_mul(4).saturating_add(shade);
            image::Rgb([value, value / 2, 200])
        });
        let path = root.join("Pictures").join(name);
        fs::create_dir_all(path.parent().expect("pictures folder"))?;
        image.save(&path).map_err(std::io::Error::other)?;
    }
    Ok(())
}
