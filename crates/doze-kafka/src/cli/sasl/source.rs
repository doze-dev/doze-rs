//! Reading the users that are not on the command line: a file, and an environment variable. Both keep the password
//! out of `ps`. Every message names a position (a line, an entry) and never the text found there.

use std::path::Path;

use kafka_auth::credentials::validate;

/// A user and a password, as written.
pub(super) type Pair = (String, String);

const FLAG: &str = "--sasl-users-file";
const ENV: &str = "DOZE_KAFKA_SASL_USERS";

/// The users in the text of a users file: one `user:password` a line, split at the first colon. A blank line, a line
/// whose first non-blank character is `#`, and the byte order mark at the start of the file are skipped; a CRLF line
/// ending is not part of the password. Nothing else is trimmed.
pub(super) fn parse_file(text: &str) -> Result<Vec<Pair>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let n = n + 1;
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        out.push(entry(line, &format!("{FLAG}: line {n}"))?);
    }
    Ok(out)
}

/// The users in the value of `DOZE_KAFKA_SASL_USERS`: `user:password` entries separated by commas, with the space around
/// an entry ignored and an empty entry skipped. A password with a comma in it cannot be written here.
pub(super) fn parse_env(text: &str) -> Result<Vec<Pair>, String> {
    let mut out = Vec::new();
    for (n, part) in text.split(',').enumerate() {
        let part = part.trim();
        if !part.is_empty() {
            out.push(entry(part, &format!("{ENV}: entry {}", n + 1))?);
        }
    }
    Ok(out)
}

fn entry(text: &str, whence: &str) -> Result<Pair, String> {
    let (user, password) = text
        .split_once(':')
        .ok_or_else(|| format!("{whence}: not a valid entry (expected user:password)"))?;
    validate(user, password).map_err(|e| format!("{whence}: {e}"))?;
    Ok((user.to_owned(), password.to_owned()))
}

/// The users file's text, or why it cannot be had.
pub(super) fn read_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path)
        .map_err(|e| format!("{FLAG} {}: cannot be read: {e}", path.display()))?;
    String::from_utf8(bytes)
        .map_err(|_| format!("{FLAG} {}: is not valid UTF-8 text", path.display()))
}

/// What to say if `path` can be read by anyone but its owner: a file of passwords is as private as a key.
#[cfg(unix)]
pub(crate) fn permission_warning(path: &Path) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).ok()?.permissions().mode();
    (mode & 0o077 != 0).then(|| {
        format!(
            "{FLAG} {} is readable by group or others (mode {:03o}); run chmod 600 on it",
            path.display(),
            mode & 0o777
        )
    })
}

/// Permissions are not looked at where there is no such thing as a mode.
#[cfg(not(unix))]
pub(crate) fn permission_warning(_path: &Path) -> Option<String> {
    None
}
