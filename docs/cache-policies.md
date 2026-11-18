# Read-only app cache policies

The first curated policies identify files whose indexed paths match the default Google Chrome or Chromium HTTP cache layout. They add **App caches** evidence to Cleanup candidates, File Inspector, and CSV. They do not inspect cache contents, find running browsers, delete files, or estimate reclaimable space. A matching path is evidence about location, not proof that a browser created the file or no longer needs it.

## Recognized layouts

Only native absolute paths on the current OS are eligible. `user` and `profile` each represent one native path component; the cache must have at least one descendant file component.

| OS | Application | Recognized file location |
| --- | --- | --- |
| macOS | Google Chrome stable | `/Users/<user>/Library/Caches/Google/Chrome/<profile>/Cache/<file>` |
| macOS | Chromium | `/Users/<user>/Library/Caches/Chromium/<profile>/Cache/<file>` |
| Windows | Google Chrome stable | `<drive>:\Users\<user>\AppData\Local\Google\Chrome\User Data\<profile>\Cache\<file>` |
| Windows | Chromium | `<drive>:\Users\<user>\AppData\Local\Chromium\User Data\<profile>\Cache\<file>` |

Nested HTTP cache formats such as `Cache/Cache_Data` are included without depending on an internal filename format. Windows policy literals compare ASCII case-insensitively; macOS uses exact spelling. Account names, profile names, and cache filenames retain native path identity, including non-UTF-8 components on macOS. Parent traversal and UNC paths are rejected. Existing scanner exclusions, link/reparse skipping, and read-only traversal still apply.

The rules intentionally omit relocated home/app-data directories, custom cache/profile roots, other browsers and release channels, and Linux. They do not classify sibling bookmarks, passwords, cookies, sessions, Local Storage, or Service Worker storage as HTTP caches. Generic existing metadata rules may still identify a sibling empty or temporary file; that is separate evidence. Empty files keep their existing first-priority classification. A recognized cache location is always **Protected · review only**, even if its file is empty or read-only.

## Evidence and maintenance

Reviewed November 17, 2026 against Chromium's primary sources:

- [User data and cache directory conventions](https://chromium.googlesource.com/chromium/src/+/HEAD/docs/user_data_dir.md) distinguish Windows profile-based cache roots from macOS `Library/Caches` roots, and document custom-root overrides.
- [Chrome path constants](https://chromium.googlesource.com/chromium/src/+/HEAD/chrome/common/chrome_constants.h) define the `Cache` directory separately from profile data files.
- [Profile network context](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/chrome/browser/net/profile_network_context_service.cc) appends that directory to the base cache path.

The engine owns these rules in `app_cache.rs`; storage and desktop callers use the existing shared candidate/risk APIs. App caches is appended to the kind list so earlier Review group identifiers stay stable. Rules are evaluated against saved metadata, so older saved scans gain this classification without a database migration or reading the source files again.

Tests cover both platform layouts and rejected siblings/channels, native absolute/relative paths, parent traversal, native filenames, and reopening synthetic SQLite records for consistent Review/CSV evidence. Windows layout logic runs in the shared tests; Windows native path execution and actual browser installations still require Windows CI/runtime testing. New applications need authoritative path evidence, negative fixtures for personal/profile data, and explicit limits before inclusion. No policy authorizes future cleanup without separately validated mutation and recovery rules.
