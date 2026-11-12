//! The pieces of the plan that need care: the password file valkey reads first, and where it listens.

use doze_module::proto::msg::instance::Address;
use doze_module::proto::msg::run::Ready;
use doze_module::{Result, help};
use std::path::Path;

/// The config file that holds the password, in the data directory.
const AUTH_FILE: &str = "doze-auth.conf";

/// With a password, the config file holding it (valkey reads a config file only as its first argument); without
/// one, nothing, and an old file is removed so a dropped password does not linger on disk.
pub(crate) fn auth_file(data: &Path, password: Option<&str>) -> Result<Vec<String>> {
    let path = data.join(AUTH_FILE);
    let Some(p) = password else {
        help::clear_stale(&path)?;
        return Ok(vec![]);
    };
    doze_os::fs::private_dir(data).map_err(help::io_error("creating", data))?;
    doze_os::fs::write_private(&path, auth_conf(p).as_bytes())
        .map_err(help::io_error("writing", &path))?;
    Ok(vec![path.to_string_lossy().into_owned()])
}

/// `requirepass "…"`, quoted the way valkey's config parser reads a quoted argument.
pub(crate) fn auth_conf(password: &str) -> String {
    let quoted = password.replace('\\', "\\\\").replace('"', "\\\"");
    format!("# Written by doze on every boot.\nrequirepass \"{quoted}\"\n")
}

/// The listening arguments and the readiness check for a granted backend.
pub(crate) fn listen(addr: Address) -> Result<(Vec<String>, Ready)> {
    match addr {
        Address::Unix(p) => Ok((
            vec![
                "--port".into(),
                "0".into(),
                "--unixsocket".into(),
                p.clone(),
            ],
            Ready::Socket(p),
        )),
        Address::Tcp(a) => {
            let (host, port) = help::host_port(&a)?;
            let args = vec!["--port".into(), port.into(), "--bind".into(), host.into()];
            Ok((args, Ready::Tcp(a.clone())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_password_line_survives_quotes_and_backslashes() {
        assert_eq!(
            auth_conf(r#"a"b\c d"#),
            "# Written by doze on every boot.\nrequirepass \"a\\\"b\\\\c d\"\n"
        );
    }
}
