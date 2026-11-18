use scan_store::SavedScan;

use super::ReviewKind;

pub(super) fn notice(kind: ReviewKind, scan: &SavedScan) -> Option<String> {
    let mut parts = Vec::new();
    if scan.cancelled {
        parts.push("Scan cancelled; findings cover only the indexed portion.".to_string());
    }
    match kind {
        ReviewKind::ExactDuplicates => {
            if !scan.duplicates_analyzed {
                parts.push(
                    "Duplicate analysis is incomplete; only completed groups are shown.".into(),
                );
            }
            if scan.duplicate_error_count > 0 {
                parts.push(format!(
                    "{} duplicate checks failed.",
                    scan.duplicate_error_count
                ));
            }
        }
        ReviewKind::SimilarImages => {
            if !scan.similar_images_analyzed {
                parts.push("Image analysis is incomplete; only completed groups are shown.".into());
            }
            if scan.image_error_count > 0 {
                parts.push(format!(
                    "{} images could not be analyzed.",
                    scan.image_error_count
                ));
            }
        }
        ReviewKind::CleanupCandidates | ReviewKind::EmptyFolders => {}
    }
    if scan.progress.skipped > 0 || scan.progress.errors > 0 {
        parts.push(format!(
            "Traversal skipped {} entries and reported {} errors.",
            scan.progress.skipped, scan.progress.errors
        ));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

pub(super) fn empty_message(kind: ReviewKind, scan: &SavedScan) -> &'static str {
    if matches!(kind, ReviewKind::ExactDuplicates) && !scan.duplicates_analyzed
        || matches!(kind, ReviewKind::SimilarImages) && !scan.similar_images_analyzed
    {
        return "No completed groups are available for this check. Run a new scan to finish analysis.";
    }
    if scan.cancelled {
        return "No findings in the indexed portion of this cancelled scan.";
    }
    match kind {
        ReviewKind::ExactDuplicates => "No exact duplicate groups found among the files checked.",
        ReviewKind::SimilarImages => "No similar-image groups found among the images checked.",
        ReviewKind::CleanupCandidates => "No cleanup candidates matched the indexed metadata.",
        ReviewKind::EmptyFolders => {
            "No empty folders were observed. Older scans may need rescanning."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scan_engine::ScanSummary;
    use scan_store::ScanStore;

    #[test]
    fn incomplete_analysis_stays_visible_even_with_saved_groups()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = tempfile::tempdir()?;
        let mut store = ScanStore::open(&fixture.path().join("scans.sqlite3"))?;
        let mut scan = store
            .begin_scan(fixture.path(), 100)?
            .finish(&ScanSummary::default(), 0)?;
        scan.cancelled = true;
        scan.duplicate_group_count = 2;
        scan.duplicate_error_count = 3;
        let text = notice(ReviewKind::ExactDuplicates, &scan).unwrap();
        assert!(text.contains("cancelled"));
        assert!(text.contains("incomplete"));
        assert!(text.contains("3 duplicate checks failed"));
        scan.duplicate_group_count = 0;
        assert!(empty_message(ReviewKind::ExactDuplicates, &scan).contains("finish analysis"));
        assert!(empty_message(ReviewKind::CleanupCandidates, &scan).contains("indexed portion"));
        Ok(())
    }

    #[test]
    fn completed_checks_keep_skips_distinct_from_empty_results()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = tempfile::tempdir()?;
        let mut store = ScanStore::open(&fixture.path().join("scans.sqlite3"))?;
        let mut scan = store
            .begin_scan(fixture.path(), 100)?
            .finish(&ScanSummary::default(), 0)?;
        scan.duplicates_analyzed = true;
        scan.similar_images_analyzed = true;
        assert!(notice(ReviewKind::SimilarImages, &scan).is_none());
        assert!(empty_message(ReviewKind::SimilarImages, &scan).contains("images checked"));
        scan.image_error_count = 4;
        scan.progress.skipped = 5;
        scan.progress.errors = 2;
        let text = notice(ReviewKind::SimilarImages, &scan).unwrap();
        assert!(text.contains("4 images"));
        assert!(text.contains("5 entries"));
        assert!(text.contains("2 errors"));
        assert!(!text.contains("incomplete"));
        assert!(
            !notice(ReviewKind::ExactDuplicates, &scan)
                .unwrap()
                .contains("4 images")
        );
        assert!(empty_message(ReviewKind::EmptyFolders, &scan).contains("Older scans"));
        Ok(())
    }
}
