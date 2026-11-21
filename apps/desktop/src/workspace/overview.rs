use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    tooltip::Tooltip,
};
use std::sync::atomic::Ordering;

use gpui_kit::{
    Context, Div, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px, relative,
};

mod saved;
pub(super) use saved::SavedOverview;

use super::{
    Page, Workspace, exclusions::path_label, preferences::max_workers, scan::format_bytes,
    theme::type_color,
};

pub(super) fn render(workspace: &Workspace, cx: &Context<Workspace>) -> impl IntoElement {
    let tiles = div()
        .flex()
        .flex_wrap()
        .gap_4()
        .child(wide(hero_card(workspace, cx)))
        .child(narrow(disk_card(workspace, cx)))
        .child(wide(analysis_card(workspace, cx)))
        .child(narrow(types_card(workspace, cx)))
        .child(narrow(findings_card(workspace, cx)))
        .child(narrow(caches_card(workspace, cx)))
        .child(narrow(scope_card(workspace, cx)))
        .child(narrow(exclusions_card(workspace, cx)));

    div()
        .id("overview")
        .size_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_4()
        .p_6()
        .child(div().text_xl().font_semibold().child("Overview"))
        .child(tiles)
        .child(saved::render(workspace, cx))
}

/// A bento tile two columns wide; tiles wrap onto new rows as the window narrows.
fn wide(card: Div) -> Div {
    card.flex_grow(2.0).flex_basis(px(560.0)).min_w(px(320.0))
}

/// A bento tile one column wide.
fn narrow(card: Div) -> Div {
    card.flex_grow(1.0).flex_basis(px(280.0)).min_w(px(260.0))
}

fn card(title: &'static str, cx: &Context<Workspace>) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.secondary)
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(theme.muted_foreground)
                .child(title.to_uppercase()),
        )
}

/// A horizontal bar filled to `fraction`.
fn meter(fraction: f32, color: Hsla, height: f32, cx: &Context<Workspace>) -> Div {
    div()
        .w_full()
        .h(px(height))
        .rounded(px(height / 2.0))
        // Card backgrounds already use `muted`, so the empty track uses the border colour.
        .bg(cx.theme().border)
        .child(
            div()
                .h_full()
                .w(relative(fraction.clamp(0.0, 1.0)))
                .rounded(px(height / 2.0))
                .bg(color),
        )
}

/// A big number over a small label.
fn stat(value: String, label: &'static str, cx: &Context<Workspace>) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(90.0))
        .child(div().text_2xl().font_semibold().child(value))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
}

fn hero_card(workspace: &Workspace, cx: &Context<Workspace>) -> Div {
    let scan = &workspace.scan;
    let theme = cx.theme();
    let (heading, description) = scan_status(workspace);
    let progress = match (&scan.active, &scan.report) {
        (Some(active), _) => Some(active.progress),
        (None, Some(report)) => Some(report.summary.progress),
        (None, None) => None,
    };
    let scan_button = match &scan.active {
        Some(active) => Button::new("cancel-scan")
            .label("Cancel scan")
            .disabled(active.cancelling)
            .on_click(cx.listener(|workspace, _, _, cx| workspace.cancel_scan(cx))),
        None => Button::new("choose-folder")
            .primary()
            .label("Choose folder")
            .disabled(!workspace.can_start_scan())
            .on_click(cx.listener(|workspace, _, _, cx| workspace.choose_scan_folder(cx))),
    };
    let results_button = Button::new("open-results")
        .ghost()
        .label(if scan.report.is_some() {
            "View last scan results"
        } else if workspace.results.is_saved() {
            "View open results"
        } else {
            "View sample results"
        })
        .on_click(cx.listener(|workspace, _, _, cx| {
            if let Some(report) = &workspace.scan.report {
                workspace.results.show_scan(report, cx);
            }
            workspace.show_page(Page::Results);
            cx.notify();
        }));

    let mut hero = card("Scan", cx)
        .child(div().text_lg().font_semibold().child(heading))
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .text_ellipsis()
                .child(description),
        );
    if let Some(progress) = progress {
        hero = hero.child(
            div()
                .flex()
                .flex_wrap()
                .gap_4()
                .child(stat(
                    format_bytes(progress.logical_bytes),
                    "logical size",
                    cx,
                ))
                .child(stat(progress.files.to_string(), "files", cx))
                .child(stat(progress.directories.to_string(), "folders", cx))
                .child(stat(
                    (progress.skipped + progress.errors).to_string(),
                    "skipped or unreadable",
                    cx,
                )),
        );
    }
    if scan.active.is_some() {
        hero = hero.child(div().h(px(4.0)).w_full().bg(theme.primary).opacity(0.6));
    }
    hero.children(scan.error.as_ref().map(|error| {
        div()
            .text_sm()
            .text_color(theme.danger)
            .child(error.clone())
    }))
    .child(
        div()
            .flex()
            .gap_2()
            .child(scan_button)
            .child(results_button),
    )
}

fn disk_card(workspace: &Workspace, cx: &Context<Workspace>) -> Div {
    let theme = cx.theme();
    let tile = card("Disk", cx);
    let Some(report) = workspace
        .scan
        .report
        .as_ref()
        .filter(|_| workspace.scan.active.is_none())
    else {
        return tile.child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("Capacity appears after a scan in this session."),
        );
    };
    match &report.volume_space {
        Some(Ok(space)) => {
            let used = space.capacity_bytes.saturating_sub(space.available_bytes);
            let used_fraction = fraction(used, space.capacity_bytes);
            let scanned_fraction =
                fraction(report.summary.progress.logical_bytes, space.capacity_bytes);
            tile.child(
                div()
                    .flex()
                    .gap_4()
                    .child(stat(format_bytes(space.available_bytes), "available", cx))
                    .child(stat(format_bytes(space.capacity_bytes), "capacity", cx)),
            )
            .child(
                // Used space, with the scanned folder's share drawn inside it.
                div().relative().w_full().h(px(14.0)).rounded(px(7.0)).bg(theme.border).child(
                    div()
                        .h_full()
                        .w(relative(used_fraction))
                        .rounded(px(7.0))
                        .bg(theme.primary)
                        .opacity(0.45)
                        .child(
                            div()
                                .h_full()
                                .w(relative(fraction_of(scanned_fraction, used_fraction)))
                                .rounded(px(7.0))
                                .bg(theme.primary),
                        ),
                ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!(
                        "{:.0}% used · bright part is the scanned folder · OS-reported when the scan finished",
                        used_fraction * 100.0
                    )),
            )
        }
        Some(Err(error)) => tile.child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(format!("Volume space unavailable: {error}")),
        ),
        None => tile.child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("Not sampled because the scan was cancelled."),
        ),
    }
}

fn analysis_card(workspace: &Workspace, cx: &Context<Workspace>) -> Div {
    let theme = cx.theme();
    let scan = &workspace.scan;
    let preferences = workspace.preferences;
    let saved = workspace.results.saved_scan().cloned();
    let mut tile = card("Duplicate and similar-image analysis", cx);

    let step = |label: &'static str, detail: String, done: f32, active: bool| {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .flex_1()
            .min_w(px(150.0))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_sm()
                    .child(div().font_semibold().child(label))
                    .child(div().text_color(theme.muted_foreground).child(detail)),
            )
            .child(meter(
                done,
                if active {
                    theme.primary
                } else {
                    theme.muted_foreground
                },
                8.0,
                cx,
            ))
    };

    match &scan.analysis {
        Some(analysis) => {
            let progress = &analysis.progress;
            let checked = progress.candidates_checked.load(Ordering::Relaxed);
            let total = progress.candidates_total.load(Ordering::Relaxed);
            let images = progress.comparing_images.load(Ordering::Relaxed);
            let images_done = progress.images_done.load(Ordering::Relaxed);
            let images_total = progress.images_total.load(Ordering::Relaxed);
            tile = tile
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .text_ellipsis()
                        .child(format!(
                            "{} · {} image workers. Results stay browsable while this runs.",
                            analysis.root.display(),
                            analysis.workers
                        )),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_4()
                        .child(step("Index", "saved".into(), 1.0, false))
                        .child(step(
                            "Exact duplicates",
                            format!("{checked} / {total} candidates"),
                            if images {
                                1.0
                            } else {
                                fraction(checked, total)
                            },
                            !images,
                        ))
                        .child(step(
                            "Similar images",
                            format!("{images_done} / {images_total}"),
                            fraction(images_done as u64, images_total as u64),
                            images,
                        )),
                )
                .child(
                    div()
                        .flex()
                        .gap_4()
                        .child(stat(
                            progress
                                .duplicate_groups
                                .load(Ordering::Relaxed)
                                .to_string(),
                            "duplicate groups so far",
                            cx,
                        ))
                        .child(stat(
                            progress.exempt_files.load(Ordering::Relaxed).to_string(),
                            "skipped: games, caches, backups, cloud-only",
                            cx,
                        )),
                )
                .child(
                    Button::new("cancel-analysis")
                        .label(if analysis.cancelling {
                            "Cancelling…"
                        } else {
                            "Cancel analysis"
                        })
                        .disabled(analysis.cancelling)
                        .on_click(cx.listener(|workspace, _, _, cx| workspace.cancel_analysis(cx))),
                );
        }
        None => {
            let (duplicates, images) = saved.as_ref().map_or((0.0, 0.0), |scan| {
                (
                    f32::from(u8::from(scan.duplicates_analyzed)),
                    f32::from(u8::from(scan.similar_images_analyzed)),
                )
            });
            tile = tile
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_4()
                        .child(step(
                            "Index",
                            if saved.is_some() {
                                "saved"
                            } else {
                                "no scan open"
                            }
                            .into(),
                            f32::from(u8::from(saved.is_some())),
                            false,
                        ))
                        .child(step(
                            "Exact duplicates",
                            if duplicates > 0.0 { "done" } else { "not run" }.into(),
                            duplicates,
                            false,
                        ))
                        .child(step(
                            "Similar images",
                            if images > 0.0 { "done" } else { "not run" }.into(),
                            images,
                            false,
                        )),
                )
                .children(scan.analysis_note.as_ref().map(|note| {
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(note.clone())
                }))
                .child(
                    Button::new("run-analysis")
                        .primary()
                        .label(if duplicates > 0.0 {
                            "Run analysis again"
                        } else {
                            "Run analysis"
                        })
                        .disabled(saved.is_none() || !workspace.can_start_scan())
                        .on_click(cx.listener(|workspace, _, _, cx| {
                            if let Some(scan) = workspace.results.saved_scan().cloned() {
                                workspace.start_analysis(scan, cx);
                            }
                        })),
                );
        }
    }

    let editable = scan.analysis.is_none();
    tile.child(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .pt_2()
            .border_t_1()
            .border_color(theme.border)
            .child(div().text_sm().child("Image workers"))
            .child(
                Button::new("fewer-workers")
                    .small()
                    .label("−")
                    .disabled(!editable || preferences.analysis_workers <= 1)
                    .on_click(cx.listener(|workspace, _, _, cx| {
                        workspace.change_preferences(cx, |preferences| {
                            preferences.analysis_workers =
                                preferences.analysis_workers.saturating_sub(1).max(1);
                        });
                    })),
            )
            .child(
                div()
                    .w(px(28.0))
                    .text_center()
                    .font_semibold()
                    .child(preferences.analysis_workers.to_string()),
            )
            .child(
                Button::new("more-workers")
                    .small()
                    .label("+")
                    .disabled(!editable || preferences.analysis_workers >= max_workers())
                    .on_click(cx.listener(|workspace, _, _, cx| {
                        workspace.change_preferences(cx, |preferences| {
                            preferences.analysis_workers =
                                (preferences.analysis_workers + 1).min(max_workers());
                        });
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("of {} cores", max_workers())),
            )
            .child(
                Button::new("auto-analyze")
                    .small()
                    .label("Analyze after each scan")
                    .selected(preferences.auto_analyze)
                    .on_click(cx.listener(|workspace, _, _, cx| {
                        workspace.change_preferences(cx, |preferences| {
                            preferences.auto_analyze = !preferences.auto_analyze;
                        });
                    })),
            ),
    )
}

fn types_card(workspace: &Workspace, cx: &Context<Workspace>) -> Div {
    let theme = cx.theme();
    let tile = card("Storage by type · latest saved scan", cx);
    let Some(types) = workspace
        .overview
        .data
        .as_ref()
        .map(|data| &data.file_types)
        .filter(|types| !types.is_empty())
    else {
        return tile.child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("File types appear after a saved scan."),
        );
    };
    let total: u64 = types.iter().map(|total| total.logical_bytes).sum();
    let mut stacked = div()
        .flex()
        .w_full()
        .h(px(16.0))
        .rounded(px(4.0))
        .overflow_hidden();
    for total_of_type in types {
        stacked = stacked.child(
            div()
                .h_full()
                .w(relative(fraction(total_of_type.logical_bytes, total)))
                .bg(type_color(total_of_type.file_type)),
        );
    }
    let mut legend = div().flex().flex_col().gap_1();
    for total_of_type in types.iter().take(6) {
        legend = legend.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .child(
                    div()
                        .size(px(10.0))
                        .rounded(px(2.0))
                        .bg(type_color(total_of_type.file_type)),
                )
                .child(div().flex_1().child(total_of_type.file_type))
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(format_bytes(total_of_type.logical_bytes)),
                ),
        );
    }
    tile.child(stacked).child(legend)
}

fn findings_card(workspace: &Workspace, cx: &Context<Workspace>) -> Div {
    let tile = card("Findings", cx);
    let Some(scan) = workspace.results.saved_scan() else {
        return tile.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Open or run a scan to see what it found."),
        );
    };
    tile.child(
        div()
            .flex()
            .flex_wrap()
            .gap_4()
            .child(stat(
                scan.duplicate_group_count.to_string(),
                "duplicate groups",
                cx,
            ))
            .child(stat(
                scan.similar_group_count.to_string(),
                "similar-image groups",
                cx,
            )),
    )
    .child(
        div()
            .flex()
            .flex_wrap()
            .gap_4()
            .child(stat(
                scan.large_file_count.to_string(),
                "files over 1 GiB",
                cx,
            ))
            .child(stat(
                (scan.duplicate_error_count + scan.image_error_count).to_string(),
                "unreadable during analysis",
                cx,
            )),
    )
    .child(
        Button::new("open-review")
            .small()
            .label("Open Review")
            .on_click(cx.listener(|workspace, _, _, cx| {
                workspace.show_page(Page::Review);
                workspace.load_page(Page::Review, cx);
            })),
    )
}

fn caches_card(workspace: &Workspace, cx: &Context<Workspace>) -> Div {
    let theme = cx.theme();
    let tile = card("Regenerable developer caches", cx);
    let Some(caches) = workspace
        .overview
        .data
        .as_ref()
        .map(|data| &data.caches)
        .filter(|caches| !caches.is_empty())
    else {
        return tile.child(div().text_sm().text_color(theme.muted_foreground).child(
            "None in the latest scan. Scan your home folder to find npm, Cargo, Gradle, Xcode \
             and similar caches.",
        ));
    };
    let total: u64 = caches.iter().map(|cache| cache.logical_bytes).sum();
    let largest = caches[0].logical_bytes;
    let mut tile = tile
        .child(stat(format_bytes(total), "rebuildable by their tools", cx))
        .child(div().text_xs().text_color(theme.muted_foreground).child(
            "Each tool can rebuild these: for example npm cache clean --force, \
             go clean -modcache, or deleting Xcode DerivedData. Nothing here moves files.",
        ));
    for cache in caches.iter().take(6) {
        tile = tile.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .text_sm()
                        .child(cache.label)
                        .child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(format_bytes(cache.logical_bytes)),
                        ),
                )
                .child(meter(
                    fraction(cache.logical_bytes, largest),
                    theme.primary,
                    6.0,
                    cx,
                )),
        );
    }
    tile
}

fn scope_card(workspace: &Workspace, cx: &Context<Workspace>) -> Div {
    let theme = cx.theme();
    let smart = workspace.preferences.smart_scope;
    let mut tile = card("Smart scope", cx)
        .child(div().text_sm().text_color(theme.muted_foreground).child(
            "Broad scans skip dependency and build folders inside projects (node_modules, \
             target, .venv, .git, Unity Library, Unreal Intermediate…). Choose a project folder \
             to scan it in full. Games, app bundles, system folders, disk images, device \
             backups, developer caches and online-only cloud files are sized but never read \
             for duplicates.",
        ))
        .child(
            Button::new("smart-scope")
                .small()
                .label(if smart {
                    "Smart scope on"
                } else {
                    "Smart scope off: index everything"
                })
                .selected(smart)
                .disabled(!workspace.can_start_scan())
                .on_click(cx.listener(|workspace, _, _, cx| {
                    workspace.change_preferences(cx, |preferences| {
                        preferences.smart_scope = !preferences.smart_scope;
                    });
                })),
        );
    if let Some(report) = workspace
        .scan
        .report
        .as_ref()
        .filter(|report| report.scoped_out_count > 0)
    {
        let mut reasons: Vec<(&'static str, usize)> = Vec::new();
        for (_, reason) in &report.scoped_out {
            match reasons.iter_mut().find(|(known, _)| known == reason) {
                Some((_, count)) => *count += 1,
                None => reasons.push((reason, 1)),
            }
        }
        reasons.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        tile = tile.child(stat(
            report.scoped_out_count.to_string(),
            "folders skipped in the last scan",
            cx,
        ));
        for (reason, count) in reasons.into_iter().take(5) {
            tile = tile.child(
                div()
                    .flex()
                    .justify_between()
                    .text_sm()
                    .child(reason)
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(count.to_string()),
                    ),
            );
        }
    }
    tile
}

fn fraction(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64) as f32
    }
}

/// `part` as a share of `whole`, both already fractions of the same total.
fn fraction_of(part: f32, whole: f32) -> f32 {
    if whole <= 0.0 {
        0.0
    } else {
        (part / whole).min(1.0)
    }
}

fn exclusions_card(workspace: &Workspace, cx: &Context<Workspace>) -> Div {
    let exclusions = &workspace.exclusions;
    let disabled = workspace.exclusion_controls_disabled();
    let rows = exclusions.paths.iter().enumerate().map(|(index, path)| {
        let path_to_remove = path.clone();
        let label = path_label(path);
        let tooltip = label.clone();
        div()
            .flex()
            .items_center()
            .gap_3()
            .py_1()
            .child(
                div()
                    .id(("excluded-path", index))
                    .flex_1()
                    .min_w(px(0.0))
                    .text_sm()
                    .text_ellipsis()
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                    .child(label),
            )
            .child(
                Button::new(("remove-exclusion", index))
                    .ghost()
                    .small()
                    .label("Remove")
                    .disabled(disabled)
                    .on_click(cx.listener(move |workspace, _, _, cx| {
                        workspace.remove_exclusion(path_to_remove.clone(), cx);
                    })),
            )
    });

    card("Scan exclusions", cx)
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("These files and folders are skipped in future scans."),
        )
        .children(exclusions.error.as_ref().map(|error| {
            div()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(error.clone())
        }))
        .children(exclusions.busy.then(|| {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Updating scan exclusions…")
        }))
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    Button::new("exclude-folder")
                        .small()
                        .label("Add folder")
                        .disabled(disabled)
                        .on_click(cx.listener(|workspace, _, _, cx| {
                            workspace.choose_exclusion(true, cx);
                        })),
                )
                .child(
                    Button::new("exclude-file")
                        .small()
                        .label("Add file")
                        .disabled(disabled)
                        .on_click(cx.listener(|workspace, _, _, cx| {
                            workspace.choose_exclusion(false, cx);
                        })),
                ),
        )
        .children(rows)
}

/// The current or last scan as a heading and a one-line description.
pub(super) fn scan_status(workspace: &Workspace) -> (&'static str, String) {
    let scan = &workspace.scan;
    match (&scan.active, &scan.report) {
        (Some(active), _) => (
            if active.cancelling {
                "Cancelling scan"
            } else if active.analyzing {
                "Checking duplicates and similar images"
            } else {
                "Scanning folder"
            },
            format!(
                "{} · {} files · {} folders · {} logical size · {} skipped · {} errors",
                active.root.display(),
                active.progress.files,
                active.progress.directories,
                format_bytes(active.progress.logical_bytes),
                active.progress.skipped,
                active.progress.errors,
            ),
        ),
        (None, Some(report)) => (
            if report.summary.cancelled {
                "Last scan cancelled"
            } else {
                "Last scan"
            },
            format!(
                "{} · {} files · {} logical size",
                report.root.display(),
                report.summary.progress.files,
                format_bytes(report.summary.progress.logical_bytes),
            ),
        ),
        (None, None) => (
            "Ready to scan",
            "Choose a folder to inspect its files. Scans are read-only.".into(),
        ),
    }
}
