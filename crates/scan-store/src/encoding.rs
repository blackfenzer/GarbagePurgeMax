use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Error, Row, types::Type};

const NANOS_PER_SECOND: i128 = 1_000_000_000;

#[cfg(unix)]
pub(super) fn encode_path(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(windows)]
pub(super) fn encode_path(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

pub(super) fn decode_path(row: &Row<'_>, column: usize) -> rusqlite::Result<PathBuf> {
    let bytes: Vec<u8> = row.get(column)?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(std::ffi::OsString::from_vec(bytes).into())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let (words, remainder) = bytes.as_chunks::<2>();
        if !remainder.is_empty() {
            return Err(Error::FromSqlConversionFailure(
                column,
                Type::Blob,
                io::Error::new(io::ErrorKind::InvalidData, "invalid Windows path").into(),
            ));
        }
        let words: Vec<_> = words.iter().copied().map(u16::from_le_bytes).collect();
        Ok(std::ffi::OsString::from_wide(&words).into())
    }
}

fn unix_nanos(time: SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(value) => value.as_nanos() as i128,
        Err(error) => -(error.duration().as_nanos() as i128),
    }
}

pub(super) fn unix_millis(time: SystemTime) -> rusqlite::Result<i64> {
    i64::try_from(unix_nanos(time).div_euclid(1_000_000))
        .map_err(|error| Error::ToSqlConversionFailure(error.into()))
}

pub(super) fn time_parts(time: Option<SystemTime>) -> rusqlite::Result<(Option<i64>, Option<u32>)> {
    let Some(time) = time else {
        return Ok((None, None));
    };
    let nanos = unix_nanos(time);
    let seconds = i64::try_from(nanos.div_euclid(NANOS_PER_SECOND))
        .map_err(|error| Error::ToSqlConversionFailure(error.into()))?;
    Ok((
        Some(seconds),
        Some(nanos.rem_euclid(NANOS_PER_SECOND) as u32),
    ))
}

pub(super) fn datetime(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<SystemTime>> {
    let Some(seconds) = row.get::<_, Option<i64>>(column)? else {
        return Ok(None);
    };
    let subsecond: u32 = row.get(column + 1)?;
    let nanos = i128::from(seconds) * NANOS_PER_SECOND + i128::from(subsecond);
    let absolute_nanos = nanos.abs();
    let duration = Duration::new(
        (absolute_nanos / NANOS_PER_SECOND) as u64,
        (absolute_nanos % NANOS_PER_SECOND) as u32,
    );
    let time = if nanos < 0 {
        UNIX_EPOCH.checked_sub(duration)
    } else {
        UNIX_EPOCH.checked_add(duration)
    };
    time.map(Some).ok_or_else(|| {
        Error::FromSqlConversionFailure(
            column,
            Type::Integer,
            io::Error::new(io::ErrorKind::InvalidData, "timestamp out of range").into(),
        )
    })
}
