use std::ffi::OsStr;
use std::path::{Component, Path, Prefix};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Platform {
    Macos,
    Windows,
}

/// Narrow, lexical evidence for default browser cache locations, never deletion authority.
/// Custom homes, relocated profiles, UNC paths, other channels and other cache types
/// deliberately remain unclassified. See docs/cache-policies.md for upstream evidence.
pub(crate) fn app_cache_origin(path: &Path) -> Option<&'static str> {
    if !path.is_absolute() {
        return None;
    }
    let platform = if cfg!(target_os = "macos") {
        Platform::Macos
    } else if cfg!(windows) {
        Platform::Windows
    } else {
        return None;
    };
    let mut names = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => names.push(name),
            Component::RootDir => {}
            Component::Prefix(prefix)
                if platform == Platform::Windows
                    && matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) => {}
            _ => return None,
        }
    }
    cache_origin(&names, platform)
}

fn cache_origin(names: &[&OsStr], platform: Platform) -> Option<&'static str> {
    let names = strip_names(names, &["Users"], platform)?;
    // Keep the account/profile/file components native; only policy literals need UTF-8.
    let names = names.get(1..)?;
    let names = match platform {
        Platform::Macos => strip_names(names, &["Library", "Caches"], platform)?,
        Platform::Windows => strip_names(names, &["AppData", "Local"], platform)?,
    };
    let (browser, names) = if let Some(names) = strip_names(names, &["Google", "Chrome"], platform)
    {
        ("Google Chrome", names)
    } else {
        ("Chromium", strip_names(names, &["Chromium"], platform)?)
    };
    let names = match platform {
        Platform::Macos => names,
        Platform::Windows => strip_names(names, &["User Data"], platform)?,
    };
    // One profile component, followed specifically by the HTTP cache directory.
    // Bookmarks, Cookies, Login Data, Sessions, Service Worker and Local Storage
    // are siblings, not part of this policy.
    let names = strip_names(names.get(1..)?, &["Cache"], platform)?;
    (!names.is_empty()).then_some(browser)
}

fn strip_names<'a>(
    names: &'a [&OsStr],
    prefix: &[&str],
    platform: Platform,
) -> Option<&'a [&'a OsStr]> {
    if names.len() < prefix.len()
        || !names.iter().zip(prefix).all(|(name, expected)| {
            name.to_str().is_some_and(|name| match platform {
                Platform::Macos => name == *expected,
                Platform::Windows => name.eq_ignore_ascii_case(expected),
            })
        })
    {
        return None;
    }
    Some(&names[prefix.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(path: &str, platform: Platform) -> Option<&'static str> {
        let names: Vec<_> = path.split('/').map(OsStr::new).collect();
        cache_origin(&names, platform)
    }

    #[test]
    fn recognizes_only_default_http_cache_layouts_on_each_platform() {
        for (platform, root) in [
            (Platform::Macos, "Users/test/Library/Caches"),
            (Platform::Windows, "Users/test/AppData/Local"),
        ] {
            for (directory, browser) in
                [("Google/Chrome", "Google Chrome"), ("Chromium", "Chromium")]
            {
                let user_data = if platform == Platform::Windows {
                    "/User Data"
                } else {
                    ""
                };
                for profile in ["Default", "Profile 2", "ผู้ใช้"] {
                    let base = format!("{root}/{directory}{user_data}/{profile}");
                    for file in ["Cache/data_0", "Cache/Cache_Data/sqldb0"] {
                        assert_eq!(classify(&format!("{base}/{file}"), platform), Some(browser));
                    }
                    for sibling in [
                        "Bookmarks",
                        "Cookies",
                        "Login Data",
                        "Sessions/session",
                        "Cache",
                        "Service Worker/CacheStorage/data",
                        "Local Storage/data",
                        "Cache-old/data",
                    ] {
                        assert_eq!(classify(&format!("{base}/{sibling}"), platform), None);
                    }
                }
            }
        }
        assert_eq!(
            classify(
                "users/test/appdata/local/google/chrome/user data/default/cache/data",
                Platform::Windows
            ),
            Some("Google Chrome")
        );
        assert_eq!(
            classify(
                "Users/test/Library/Caches/Google/Chrome Beta/Default/Cache/data",
                Platform::Macos
            ),
            None
        );
        assert_eq!(
            classify(
                "Users/test/Library/Application Support/Google/Chrome/Default/Cache/data",
                Platform::Macos
            ),
            None
        );
        assert_eq!(
            classify(
                "backup/Users/test/Library/Caches/Google/Chrome/Default/Cache/data",
                Platform::Macos
            ),
            None
        );
    }

    #[test]
    fn native_paths_require_absolute_location_without_parent_traversal() {
        #[cfg(target_os = "macos")]
        let base = Path::new("/Users/test/Library/Caches/Google/Chrome/Default/Cache");
        #[cfg(windows)]
        let base = Path::new(r"C:\Users\test\AppData\Local\Google\Chrome\User Data\Default\Cache");
        #[cfg(any(target_os = "macos", windows))]
        {
            assert_eq!(app_cache_origin(&base.join("data")), Some("Google Chrome"));
            assert_eq!(app_cache_origin(&base.join("../Bookmarks")), None);
            assert_eq!(app_cache_origin(base), None);
        }
        assert_eq!(
            app_cache_origin(Path::new(
                "Users/test/Library/Caches/Google/Chrome/Default/Cache/data"
            )),
            None
        );
        #[cfg(windows)]
        assert_eq!(
            app_cache_origin(Path::new(
                r"\\server\Users\test\AppData\Local\Google\Chrome\User Data\Default\Cache\data"
            )),
            None
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn non_utf8_account_profile_and_file_names_keep_native_identity() {
        use std::os::unix::ffi::OsStrExt;
        let name = OsStr::from_bytes(b"native-\xff");
        let path = Path::new("/Users")
            .join(name)
            .join("Library/Caches/Chromium")
            .join(name)
            .join("Cache")
            .join(name);
        assert_eq!(app_cache_origin(&path), Some("Chromium"));
        let mut file = crate::FileRecord {
            path,
            logical_bytes: 10,
            created: None,
            modified: None,
            read_only: false,
        };
        assert_eq!(
            crate::cleanup_candidate(&file, std::time::UNIX_EPOCH)
                .unwrap()
                .kind,
            crate::CleanupKind::AppCache
        );
        file.logical_bytes = 0;
        assert_eq!(
            crate::cleanup_candidate(&file, std::time::UNIX_EPOCH)
                .unwrap()
                .kind,
            crate::CleanupKind::EmptyFile
        );
        assert_eq!(
            crate::assess_risk(&file.path, false).tier,
            crate::RiskTier::Protected
        );
    }
}
