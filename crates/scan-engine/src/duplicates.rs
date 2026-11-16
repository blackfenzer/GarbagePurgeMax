use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::{Digest, Sha256};

use crate::FileRecord;

use crate::file_version::FileVersion;
use crate::hash_cache::{HashCache, HashStage, NoHashCache};

const SAMPLE_BYTES: usize = 8 * 1024;
const BUFFER_BYTES: usize = 64 * 1024;
// SQLite cache traffic costs more than hashing small files or the 16 KiB sample.
const MIN_CACHED_FILE_BYTES: u64 = 64 * 1024;
type Fingerprint = [u8; 32];

#[derive(Debug, Default)]
pub struct DuplicateAnalysis {
    pub groups: Vec<DuplicateGroup>,
    pub errors: u64,
    pub first_error: Option<String>,
    pub cancelled: bool,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DuplicateGroup {
    /// One path for each distinct file, ordered by native path.
    pub paths: Vec<PathBuf>,
    /// SHA-256 of the entire file.
    pub hash: [u8; 32],
}

/// Verifies indexed files with equal sizes. If sizes are mixed, each size is
/// analyzed independently. Cancellation discards the groups from this call.
pub fn verify_duplicate_group(files: &[FileRecord], cancelled: &AtomicBool) -> DuplicateAnalysis {
    verify_duplicate_group_cached(files, cancelled, &mut NoHashCache)
}

/// Reuses only hashes with a matching native file version. Every resulting match is
/// still compared byte for byte; the cache never stores or grants verified status.
pub fn verify_duplicate_group_cached(
    files: &[FileRecord],
    cancelled: &AtomicBool,
    cache: &mut dyn HashCache,
) -> DuplicateAnalysis {
    let mut verifier = Verifier {
        cancelled,
        cache,
        analysis: DuplicateAnalysis::default(),
    };
    verifier.verify(files);
    if verifier.is_cancelled() {
        verifier.analysis.cancelled = true;
        verifier.analysis.groups.clear();
    }
    verifier.analysis
}

struct Verifier<'a> {
    cancelled: &'a AtomicBool,
    cache: &'a mut dyn HashCache,
    analysis: DuplicateAnalysis,
}

impl Verifier<'_> {
    fn verify(&mut self, files: &[FileRecord]) {
        let mut by_size = BTreeMap::<_, Vec<_>>::new();
        for record in files {
            if self.is_cancelled() {
                return;
            }
            by_size
                .entry(record.logical_bytes)
                .or_default()
                .push(record);
        }

        for records in by_size.values_mut().filter(|records| records.len() > 1) {
            records.sort_unstable_by(|a, b| a.path.cmp(&b.path));
            let mut identities = HashSet::new();
            let mut by_sample = BTreeMap::<_, Vec<_>>::new();
            for record in records {
                if self.is_cancelled() {
                    return;
                }
                let opened = FileVersion::open(&record.path).and_then(|(file, version)| {
                    version.check_record(record)?;
                    Ok((file, version))
                });
                let Some((mut file, version)) = self.observe(&record.path, opened) else {
                    continue;
                };
                if identities.contains(&version.identity) {
                    continue;
                }
                let partial = self.hash(&record.path, &mut file, version, HashStage::Sample);
                if let Some(hash) = self.observe(&record.path, partial) {
                    identities.insert(version.identity);
                    by_sample
                        .entry(hash)
                        .or_default()
                        .push(Candidate { record, version });
                }
            }

            for candidates in by_sample.into_values().filter(|files| files.len() > 1) {
                let mut by_hash = BTreeMap::<_, VecDeque<_>>::new();
                for candidate in candidates {
                    if self.is_cancelled() {
                        return;
                    }
                    let full = candidate.open().and_then(|mut file| {
                        self.hash(
                            candidate.path(),
                            &mut file,
                            candidate.version,
                            HashStage::Full,
                        )
                    });
                    if let Some(hash) = self.observe(candidate.path(), full) {
                        by_hash.entry(hash).or_default().push_back(candidate);
                    }
                }

                for (hash, candidates) in by_hash {
                    if self.is_cancelled() {
                        return;
                    }
                    self.verify_hash_bucket(hash, candidates);
                }
            }
        }
        self.analysis
            .groups
            .sort_unstable_by(|a, b| a.paths.cmp(&b.paths));
    }

    fn verify_hash_bucket<'a>(
        &mut self,
        hash: Fingerprint,
        mut candidates: VecDeque<Candidate<'a>>,
    ) {
        while !self.is_cancelled() && candidates.len() > 1 {
            let leader = candidates.pop_front().unwrap();
            let mut matched = Vec::new();
            let mut leader_valid = true;
            for _ in 0..candidates.len() {
                let candidate = candidates.pop_front().unwrap();
                match compare_contents(leader, candidate, self.cancelled) {
                    Ok(true) => matched.push(candidate),
                    Ok(false) => candidates.push_back(candidate),
                    Err((path, error)) => {
                        self.observe::<()>(path, Err(error));
                        if self.is_cancelled() {
                            return;
                        }
                        if path == leader.path() {
                            // The remaining candidates may match one another.
                            candidates.push_back(candidate);
                            candidates.extend(std::mem::take(&mut matched));
                            leader_valid = false;
                            break;
                        }
                    }
                }
            }

            if leader_valid && !matched.is_empty() {
                matched.push(leader);
                let mut paths = Vec::new();
                for candidate in matched {
                    if self.is_cancelled() {
                        return;
                    }
                    let current = candidate.version.check_path(candidate.path());
                    if self.observe(candidate.path(), current).is_some() {
                        paths.push(candidate.path().to_path_buf());
                    }
                }
                if paths.len() > 1 {
                    paths.sort_unstable();
                    self.analysis.groups.push(DuplicateGroup { paths, hash });
                }
            }
        }
    }

    fn hash(
        &mut self,
        path: &Path,
        file: &mut File,
        version: FileVersion,
        stage: HashStage,
    ) -> io::Result<Fingerprint> {
        check_cancelled(self.cancelled)?;
        let key = (stage == HashStage::Full && version.len >= MIN_CACHED_FILE_BYTES)
            .then(|| version.cache_key());
        if let Some(key) = &key {
            if let Some(hash) = self.cache.load(path, key, stage)? {
                version.check(path, file)?;
                self.analysis.cache_hits += 1;
                return Ok(hash);
            }
            self.analysis.cache_misses += 1;
        }
        let hash = match stage {
            HashStage::Sample => hash_sample(file, version.len, self.cancelled)?,
            HashStage::Full => hash_full(file, version.len, self.cancelled)?,
        };
        version.check(path, file)?;
        if let Some(key) = key {
            self.cache.store(path, &key, stage, &hash)?;
        }
        Ok(hash)
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    fn observe<T>(&mut self, path: &Path, result: io::Result<T>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                if !self.is_cancelled() {
                    self.analysis.errors += 1;
                    self.analysis
                        .first_error
                        .get_or_insert_with(|| format!("{}: {error}", path.display()));
                }
                None
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Candidate<'a> {
    record: &'a FileRecord,
    version: FileVersion,
}

impl<'a> Candidate<'a> {
    fn path(self) -> &'a Path {
        &self.record.path
    }

    fn open(self) -> io::Result<File> {
        let (file, version) = FileVersion::open(self.path())?;
        if version == self.version {
            Ok(file)
        } else {
            Err(io::Error::other("file changed since it was read"))
        }
    }
}

fn hash_sample(file: &mut File, len: u64, cancelled: &AtomicBool) -> io::Result<Fingerprint> {
    let mut hasher = Sha256::new();
    let mut sample = [0; SAMPLE_BYTES];
    let head = len.min(SAMPLE_BYTES as u64) as usize;
    check_cancelled(cancelled)?;
    file.read_exact(&mut sample[..head])?;
    hasher.update(&sample[..head]);

    let tail = (len - head as u64).min(SAMPLE_BYTES as u64) as usize;
    if tail > 0 {
        check_cancelled(cancelled)?;
        file.seek(SeekFrom::Start(len - tail as u64))?;
        file.read_exact(&mut sample[..tail])?;
        hasher.update(&sample[..tail]);
    }
    Ok(hasher.finalize().into())
}

fn hash_full(
    file: &mut impl Read,
    mut remaining: u64,
    cancelled: &AtomicBool,
) -> io::Result<Fingerprint> {
    let mut hasher = Sha256::new();
    let mut buffer = [0; BUFFER_BYTES];
    while remaining > 0 {
        check_cancelled(cancelled)?;
        let size = remaining.min(BUFFER_BYTES as u64) as usize;
        file.read_exact(&mut buffer[..size])?;
        hasher.update(&buffer[..size]);
        remaining -= size as u64;
    }
    Ok(hasher.finalize().into())
}

fn compare_contents<'a>(
    left: Candidate<'a>,
    right: Candidate<'a>,
    cancelled: &AtomicBool,
) -> Result<bool, (&'a Path, io::Error)> {
    let left_error = |error| (left.path(), error);
    let right_error = |error| (right.path(), error);
    check_cancelled(cancelled).map_err(left_error)?;
    let mut a = left.open().map_err(left_error)?;
    let mut b = right.open().map_err(right_error)?;
    let mut a_buffer = [0; BUFFER_BYTES];
    let mut b_buffer = [0; BUFFER_BYTES];
    let mut remaining = left.version.len;
    let mut equal = true;
    while remaining > 0 {
        check_cancelled(cancelled).map_err(left_error)?;
        let size = remaining.min(BUFFER_BYTES as u64) as usize;
        a.read_exact(&mut a_buffer[..size]).map_err(left_error)?;
        b.read_exact(&mut b_buffer[..size]).map_err(right_error)?;
        if a_buffer[..size] != b_buffer[..size] {
            equal = false;
            break;
        }
        remaining -= size as u64;
    }
    left.version.check(left.path(), &a).map_err(left_error)?;
    right.version.check(right.path(), &b).map_err(right_error)?;
    Ok(equal)
}

fn check_cancelled(cancelled: &AtomicBool) -> io::Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "analysis cancelled",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn checks_cancellation_between_chunks() {
        struct CancelOnRead<'a>(&'a AtomicBool);
        impl Read for CancelOnRead<'_> {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                assert!(!self.0.swap(true, Ordering::Relaxed));
                Ok(buffer.len())
            }
        }

        let cancelled = AtomicBool::new(false);
        let error = hash_full(
            &mut CancelOnRead(&cancelled),
            (BUFFER_BYTES * 2) as u64,
            &cancelled,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    }

    #[test]
    fn detects_replacement_of_a_previously_opened_path() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("original");
        let replacement = directory.path().join("replacement");
        fs::write(&path, b"same")?;
        fs::write(&replacement, b"same")?;
        let (open_file, original) = FileVersion::open(&path)?;
        let (_, separate) = FileVersion::open(&replacement)?;
        assert_ne!(original.identity, separate.identity);

        fs::rename(&path, directory.path().join("moved"))?;
        fs::rename(replacement, &path)?;
        assert!(original.check(&path, &open_file).is_err());
        assert!(original.check_path(&path).is_err());
        Ok(())
    }

    #[test]
    fn separates_distinct_contents_in_one_hash_bucket() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let mut records = Vec::new();
        let mut versions = Vec::new();
        for (name, contents) in [("a", b"one"), ("b", b"two"), ("c", b"two")] {
            let path = directory.path().join(name);
            fs::write(&path, contents)?;
            let (_, version) = FileVersion::open(&path)?;
            records.push(FileRecord {
                path,
                logical_bytes: version.len,
                created: None,
                modified: None,
                read_only: false,
            });
            versions.push(version);
        }

        let candidates = records
            .iter()
            .zip(versions)
            .map(|(record, version)| Candidate { record, version })
            .collect();
        let cancelled = AtomicBool::new(false);
        let mut verifier = Verifier {
            cancelled: &cancelled,
            cache: &mut NoHashCache,
            analysis: DuplicateAnalysis::default(),
        };
        verifier.verify_hash_bucket([0; 32], candidates);
        assert_eq!(verifier.analysis.errors, 0);
        assert_eq!(verifier.analysis.groups.len(), 1);
        assert_eq!(
            verifier.analysis.groups[0].paths,
            [records[1].path.clone(), records[2].path.clone()]
        );
        Ok(())
    }
    #[test]
    fn a_cached_hash_never_substitutes_for_byte_comparison() -> io::Result<()> {
        struct CollidingCache;
        impl HashCache for CollidingCache {
            fn load(
                &mut self,
                _: &Path,
                _: &[u8],
                _: HashStage,
            ) -> io::Result<Option<Fingerprint>> {
                Ok(Some([0; 32]))
            }
            fn store(
                &mut self,
                _: &Path,
                _: &[u8],
                _: HashStage,
                _: &Fingerprint,
            ) -> io::Result<()> {
                Ok(())
            }
        }
        let root = tempfile::tempdir()?;
        let mut files = Vec::new();
        for (name, middle) in [("a", b'a'), ("b", b'b'), ("c", b'b')] {
            let mut bytes = vec![0; 128 * 1024];
            bytes[64 * 1024] = middle;
            let path = root.path().join(name);
            fs::write(&path, bytes)?;
            files.push(FileRecord {
                path,
                logical_bytes: 128 * 1024,
                created: None,
                modified: None,
                read_only: false,
            });
        }
        let analysis =
            verify_duplicate_group_cached(&files, &AtomicBool::new(false), &mut CollidingCache);
        assert_eq!(analysis.cache_hits, 3);
        assert_eq!(analysis.groups.len(), 1);
        assert_eq!(
            analysis.groups[0].paths,
            [root.path().join("b"), root.path().join("c")]
        );
        Ok(())
    }
}
