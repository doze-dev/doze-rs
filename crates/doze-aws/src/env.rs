//! `doze-aws env`: the lines that point this shell's AWS SDKs and the AWS CLI at the running doze-aws. Besides the endpoint, region
//! and keys, it clears what would quietly send calls elsewhere: a per-service `AWS_ENDPOINT_URL_<SERVICE>` (it beats
//! `AWS_ENDPOINT_URL`), `AWS_IGNORE_CONFIGURED_ENDPOINT_URLS` (it makes the SDKs ignore both), and a leftover `AWS_SESSION_TOKEN`
//! from a real AWS session (sent with the local keys, `--auth verify` refuses it).

use crate::cli::EnvArgs;
use crate::config::Config;
use crate::instance::{Instance, endpoint_of};

/// The shell syntax to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shell {
    Sh,
    Fish,
    Pwsh,
}

impl Shell {
    /// The shell `--shell` names, else the one `$SHELL` names (Pwsh on Windows).
    pub(crate) fn pick(flag: Option<&str>) -> Self {
        let name = flag.map(str::to_owned).unwrap_or_else(|| {
            let shell = std::env::var("SHELL").unwrap_or_default();
            if shell.is_empty() && cfg!(windows) {
                return "powershell".to_owned();
            }
            shell
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or_default()
                .to_owned()
        });
        match name.as_str() {
            "fish" => Self::Fish,
            "powershell" | "pwsh" | "powershell.exe" | "pwsh.exe" => Self::Pwsh,
            _ => Self::Sh,
        }
    }

    /// How to load `doze-aws env` into this shell.
    pub(crate) const fn hint(self) -> &'static str {
        match self {
            Self::Sh => "eval \"$(doze-aws env)\"",
            Self::Fish => "doze-aws env | source",
            Self::Pwsh => "doze-aws env | Invoke-Expression",
        }
    }

    fn set(self, k: &str, v: &str) -> String {
        match self {
            Self::Sh => format!("export {k}={}", sh_quote(v)),
            Self::Fish => format!("set -gx {k} {}", sh_quote(v)),
            Self::Pwsh => format!("$Env:{k} = '{}'", v.replace('\'', "''")),
        }
    }

    fn unset(self, k: &str) -> String {
        match self {
            Self::Sh => format!("unset {k}"),
            Self::Fish => format!("set -e {k}"),
            Self::Pwsh => format!("Remove-Item Env:{k} -ErrorAction SilentlyContinue"),
        }
    }
}

fn sh_quote(v: &str) -> String {
    if !v.is_empty()
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./:@=+,".contains(&b))
    {
        return v.to_owned();
    }
    format!("'{}'", v.replace('\'', "'\\''"))
}

/// Variables that would send calls past `AWS_ENDPOINT_URL`, among those set in `env`.
pub(crate) fn interfering(env: &[(String, String)]) -> Vec<String> {
    let mut names: Vec<String> = env
        .iter()
        .map(|(k, _)| k)
        .filter(|k| {
            k.starts_with("AWS_ENDPOINT_URL_")
                || *k == "AWS_IGNORE_CONFIGURED_ENDPOINT_URLS"
                || *k == "AWS_SESSION_TOKEN"
        })
        .cloned()
        .collect();
    names.sort();
    names
}

/// The lines for `instance` (`unset`: the lines that undo them), clearing what `env` has that would interfere.
pub(crate) fn lines(
    instance: &Instance,
    shell: Shell,
    unset: bool,
    env: &[(String, String)],
) -> Vec<String> {
    let (access, secret) = instance.keys();
    let set = [
        ("AWS_ENDPOINT_URL", instance.endpoint.as_str()),
        ("AWS_REGION", instance.region.as_str()),
        ("AWS_DEFAULT_REGION", instance.region.as_str()),
        ("AWS_ACCESS_KEY_ID", access.as_str()),
        ("AWS_SECRET_ACCESS_KEY", secret.as_str()),
    ];
    if unset {
        return set.iter().map(|(k, _)| shell.unset(k)).collect();
    }
    let mut out: Vec<String> = set.iter().map(|(k, v)| shell.set(k, v)).collect();
    out.extend(interfering(env).iter().map(|k| shell.unset(k)));
    out
}

/// Runs the command; the exit code.
pub(crate) fn run(args: &EnvArgs, cfg: &Config) -> i32 {
    let shell = Shell::pick(args.shell.as_deref());
    let instance = crate::locate::find(cfg).or_else(|| {
        // Nothing running yet: the configured address, so `env` in a shell profile works before the server starts.
        let listen: std::net::SocketAddr = cfg.text("listen").parse().ok()?;
        if listen.port() == 0 {
            return None;
        }
        eprintln!(
            "doze-aws env: doze-aws is not running; these point at {listen}, where it will listen."
        );
        let verify = cfg.text("auth") == "verify";
        Some(Instance {
            endpoint: endpoint_of(listen),
            region: cfg.text("region"),
            auth: cfg.text("auth"),
            access_key: verify.then(|| cfg.text("access-key")),
            secret_key: verify.then(|| cfg.text("secret-key")),
            ..Instance::default()
        })
    });
    let Some(instance) = instance else {
        eprintln!("{}", crate::locate::not_running(cfg));
        return 1;
    };
    let env: Vec<(String, String)> = std::env::vars().collect();
    for line in lines(&instance, shell, args.unset, &env) {
        println!("{line}");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> Instance {
        Instance {
            endpoint: "http://127.0.0.1:4566".into(),
            region: "eu-west-1".into(),
            auth: "trust".into(),
            ..Instance::default()
        }
    }

    #[test]
    fn sh_lines_set_the_endpoint_region_and_keys_and_clear_what_interferes() {
        let env = vec![
            (
                "AWS_ENDPOINT_URL_SQS".to_owned(),
                "https://elsewhere".to_owned(),
            ),
            ("AWS_SESSION_TOKEN".to_owned(), "x".to_owned()),
            ("HOME".to_owned(), "/h".to_owned()),
        ];
        assert_eq!(
            lines(&local(), Shell::Sh, false, &env),
            [
                "export AWS_ENDPOINT_URL=http://127.0.0.1:4566",
                "export AWS_REGION=eu-west-1",
                "export AWS_DEFAULT_REGION=eu-west-1",
                "export AWS_ACCESS_KEY_ID=test",
                "export AWS_SECRET_ACCESS_KEY=test",
                "unset AWS_ENDPOINT_URL_SQS",
                "unset AWS_SESSION_TOKEN",
            ]
        );
    }

    #[test]
    fn verify_keys_fish_powershell_and_unset_are_written_in_their_syntax() {
        let mut i = local();
        i.auth = "verify".into();
        i.access_key = Some("AKID".into());
        i.secret_key = Some("it's".into());
        assert_eq!(
            lines(&i, Shell::Fish, false, &[])[4],
            "set -gx AWS_SECRET_ACCESS_KEY 'it'\\''s'"
        );
        assert_eq!(
            lines(&i, Shell::Pwsh, false, &[])[3],
            "$Env:AWS_ACCESS_KEY_ID = 'AKID'"
        );
        assert_eq!(lines(&i, Shell::Sh, true, &[])[0], "unset AWS_ENDPOINT_URL");
        assert_eq!(Shell::pick(Some("pwsh")), Shell::Pwsh);
    }
}
