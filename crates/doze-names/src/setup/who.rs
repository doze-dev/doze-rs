//! Who setup is for: the person running it, also when they ran it under sudo (root's HOME is not theirs, and a daemon
//! pointed at /root/.doze answers the zone from an empty registry: found in the Lima VM, Go commit after 74b4a84).

use crate::Error;
use std::path::PathBuf;

/// The uid and gid the daemon serves: the person running setup, also under sudo (`SUDO_UID`, `SUDO_GID`).
pub fn setup_user() -> Result<(u32, u32), Error> {
    let env = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<u32>().ok());
    who(doze_os::privilege::ids(), env("SUDO_UID"), env("SUDO_GID"))
}

fn who(
    ids: Option<(u32, u32)>,
    sudo_uid: Option<u32>,
    sudo_gid: Option<u32>,
) -> Result<(u32, u32), Error> {
    let (uid, gid) = match ids {
        Some((0, _)) => (sudo_uid.unwrap_or(0), sudo_gid.unwrap_or(0)),
        Some(ids) => ids,
        None => return Err(Error::Setup("no numeric user on this system".into())),
    };
    if uid == 0 {
        return Err(Error::Setup(
            "setup has to know whose services to serve: run it as yourself, not as root".into(),
        ));
    }
    Ok((uid, gid))
}

/// The names folder of the person running setup: `DOZE_HOME` when set, else their own home's `.doze` (under sudo, the
/// home of `SUDO_UID` from the user database), else the shared home.
pub fn setup_home() -> PathBuf {
    if std::env::var_os(crate::ENV_HOME).is_some_and(|h| !h.is_empty()) {
        return crate::home();
    }
    if let Some((0, _)) = doze_os::privilege::ids()
        && let Some(uid) = std::env::var("SUDO_UID").ok().and_then(|v| v.parse().ok())
        && let Some(home) = doze_os::privilege::home_of(uid)
    {
        return home.join(format!(".{}", crate::SUFFIX));
    }
    crate::home()
}

/// Whether the job at `path` starts a daemon older than this build's: it is there, and does not carry `stamp`, the
/// platform's spelling of this build's version. The daemon is the copy setup made, so the job is the version that runs.
pub(crate) fn job_outdated(path: &std::path::Path, stamp: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|job| !job.contains(stamp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_sudo_setup_is_for_the_person_who_asked() {
        assert_eq!(who(Some((501, 20)), None, None).unwrap(), (501, 20));
        assert_eq!(
            who(Some((0, 0)), Some(1000), Some(1000)).unwrap(),
            (1000, 1000)
        );
        assert!(
            who(Some((0, 0)), None, None).is_err(),
            "root with no SUDO_UID is refused"
        );
    }
}
