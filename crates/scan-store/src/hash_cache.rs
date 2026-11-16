use std::io;
use std::path::Path;

use rusqlite::{OptionalExtension, params};
use scan_engine::{HashCache, HashStage};

use super::{ScanWriter, encode_path, sqlite_integer};

impl HashCache for ScanWriter<'_> {
    fn load(
        &mut self,
        path: &Path,
        version: &[u8],
        stage: HashStage,
    ) -> io::Result<Option<[u8; 32]>> {
        let key = encode_path(path);
        let hash = self.transaction.prepare_cached(
            "SELECT hash FROM hash_cache WHERE path_key = ?1 AND algorithm = ?2 AND version = ?3"
        ).and_then(|mut statement| statement.query_row(params![key, stage.key(), version], |row| row.get(0)).optional())
            .map_err(io::Error::other)?;
        if hash.is_some() {
            self.transaction.prepare_cached(
                "UPDATE hash_cache SET last_scan_id = ?1 WHERE path_key = ?2 AND algorithm = ?3"
            ).and_then(|mut statement| statement.execute(params![self.scan.id, key, stage.key()]))
                .map_err(io::Error::other)?;
        }
        Ok(hash)
    }

    fn store(
        &mut self,
        path: &Path,
        version: &[u8],
        stage: HashStage,
        hash: &[u8; 32],
    ) -> io::Result<()> {
        self.transaction.prepare_cached(
            "INSERT INTO hash_cache (path_key, algorithm, version, hash, last_scan_id) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (path_key, algorithm) DO UPDATE SET version = excluded.version, hash = excluded.hash, last_scan_id = excluded.last_scan_id"
        ).and_then(|mut statement| statement.execute(params![encode_path(path), stage.key(), version, hash.as_slice(), self.scan.id]))
            .map_err(io::Error::other)?;
        Ok(())
    }
}

impl ScanWriter<'_> {
    pub fn record_cache_usage(&mut self, hits: u64, misses: u64) -> rusqlite::Result<()> {
        if hits == 0 && misses == 0 {
            return Ok(());
        }
        self.scan.hash_cache_hits += hits;
        self.scan.hash_cache_misses += misses;
        self.transaction.execute(
            "UPDATE scans SET hash_cache_hits = ?1, hash_cache_misses = ?2 WHERE id = ?3",
            params![
                sqlite_integer(self.scan.hash_cache_hits)?,
                sqlite_integer(self.scan.hash_cache_misses)?,
                self.scan.id
            ],
        )?;
        Ok(())
    }
}
