//! Whole-file helpers that do not tear.

use std::io;
use std::path::{Path, PathBuf};

use super::{Fs, FsFile};

/// The bytes of a file, or `None` if there is no such file.
///
/// # Errors
/// Any failure other than a missing file.
pub fn read_file<F: Fs>(fs: &F, path: &Path) -> io::Result<Option<Vec<u8>>> {
    let file = match fs.open_existing(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let len = usize::try_from(file.size()?).map_err(|_| io::Error::other("file too large"))?;
    let mut out = vec![0; len];
    let mut filled = 0;
    while filled < len {
        let got = file.read_at(out.get_mut(filled..).unwrap_or_default(), filled as u64)?;
        if got == 0 {
            break;
        }
        filled += got;
    }
    out.truncate(filled);
    Ok(Some(out))
}

/// Replaces `path` with `data` so that a crash at any point leaves the old contents or the new, never a torn
/// or empty file: the bytes go to a temporary file that is flushed, the temporary file is renamed over
/// `path`, and the directory is flushed so the rename is on disk too.
///
/// # Errors
/// Any failure; the temporary file is removed.
pub fn write_durably<F: Fs>(fs: &F, path: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = temporary_name(path);
    let written = write_and_sync(fs, &tmp, data).and_then(|()| fs.rename(&tmp, path));
    if let Err(e) = written {
        let _ = fs.remove_file(&tmp);
        return Err(e);
    }
    match path.parent() {
        Some(dir) => fs.sync_dir(dir),
        None => Ok(()),
    }
}

/// Where [`write_durably`] stages `path`.
#[must_use]
pub(super) fn temporary_name(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

fn write_and_sync<F: Fs>(fs: &F, tmp: &Path, data: &[u8]) -> io::Result<()> {
    let (file, _) = fs.open_or_create(tmp)?;
    file.set_len(0)?;
    file.write_all_at(data, 0)?;
    file.sync_data()
}
