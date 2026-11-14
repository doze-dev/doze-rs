//! Downloads for tests: the module's engine from one local build directory (`DOZE_<ENGINE>_BINDIR`), and any other
//! engine it fetches (postgres's extension bundles) from local folders, each named by the line that resolves to it.

use doze_module_host::{BoxFuture, Downloads};
use doze_module_proto::msg::error::{Error, ErrorKind};
use doze_module_proto::msg::run::{Ensure, Ensured};
use std::path::PathBuf;

/// One build of another engine: `line` resolves to `full` (the newest given wins), and `full` is the folder `root`,
/// whose `bin/` is what `ensure` answers (as the core's cache does).
#[derive(Debug, Clone)]
pub struct LocalEngine {
    pub engine: String,
    pub line: String,
    pub full: String,
    pub root: PathBuf,
}

pub(crate) struct LocalBuild {
    pub(crate) engine: String,
    pub(crate) bin_dir: PathBuf,
    pub(crate) full: String,
    pub(crate) extra: Vec<LocalEngine>,
}

fn missing(what: String) -> Error {
    Error::new(ErrorKind::NotFound, what)
}

impl LocalBuild {
    /// Whether `engine` is answered with the module's own build: its engine, or any engine when the test gave no others
    /// (ferret fetches documentdb from the same folder). Given others, an engine not among them is not found, so a
    /// missing build cannot pass for the module's own.
    fn serves_main(&self, engine: &str) -> bool {
        engine == self.engine || self.extra.is_empty()
    }
}

impl Downloads for LocalBuild {
    fn resolve_major<'a>(&'a self, e: &'a str, m: &'a str) -> BoxFuture<'a, Result<String, Error>> {
        Box::pin(async move {
            if self.serves_main(e) {
                return Ok(self.full.clone());
            }
            self.extra
                .iter()
                .rev()
                .find(|x| x.engine == e && x.line == m)
                .map(|x| x.full.clone())
                .ok_or_else(|| missing(format!("mirror has no {e} version for major \"{m}\"")))
        })
    }

    fn ensure<'a>(&'a self, r: &'a Ensure) -> BoxFuture<'a, Result<Ensured, Error>> {
        Box::pin(async move {
            if self.serves_main(&r.engine) {
                return Ok(Ensured {
                    bin_dir: self.bin_dir.to_string_lossy().into_owned(),
                    digest: "sha256:local".into(),
                });
            }
            let x = self
                .extra
                .iter()
                .find(|x| x.engine == r.engine && x.full == r.full)
                .ok_or_else(|| {
                    missing(format!("mirror has no {} {} artifact", r.engine, r.full))
                })?;
            let digest = format!("sha256:local-{}-{}", x.engine, x.full);
            if r.expected_sha.as_ref().is_some_and(|want| *want != digest) {
                return Err(Error::new(
                    ErrorKind::Invalid,
                    format!("{} {}: the lock pins another archive", x.engine, x.full),
                ));
            }
            Ok(Ensured {
                bin_dir: x.root.join("bin").to_string_lossy().into_owned(),
                digest,
            })
        })
    }
}
