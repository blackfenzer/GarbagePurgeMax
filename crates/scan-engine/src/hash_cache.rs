use std::io;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashStage {
    Sample,
    Full,
}

impl HashStage {
    pub fn key(self) -> &'static str {
        match self {
            Self::Sample => "sha256-head-tail-8192-v1",
            Self::Full => "sha256-full-v1",
        }
    }
}

/// Storage adapter owned by the caller. Keys encode native identity, size, modification,
/// creation, and change times. An unavailable cache must never imply a matching file.
pub trait HashCache {
    fn load(
        &mut self,
        path: &Path,
        version: &[u8],
        stage: HashStage,
    ) -> io::Result<Option<[u8; 32]>>;
    fn store(
        &mut self,
        path: &Path,
        version: &[u8],
        stage: HashStage,
        hash: &[u8; 32],
    ) -> io::Result<()>;
}

pub(crate) struct NoHashCache;

impl HashCache for NoHashCache {
    fn load(&mut self, _: &Path, _: &[u8], _: HashStage) -> io::Result<Option<[u8; 32]>> {
        Ok(None)
    }
    fn store(&mut self, _: &Path, _: &[u8], _: HashStage, _: &[u8; 32]) -> io::Result<()> {
        Ok(())
    }
}
