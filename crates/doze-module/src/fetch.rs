//! The host's download service, for `resolve`: the host owns the mirror, the cache, checksums and the lock; a module
//! only names what it needs.

use crate::module::Result;
use doze_module_proto::method::Method;
use doze_module_proto::msg::run::{Ensure, Ensured, ResolveMajor};
use doze_module_proto::peer::{CallOpts, Peer};
use std::time::Duration;

/// Downloads can be large; the host reports progress and enforces its own limits.
const FETCH_DEADLINE: Duration = Duration::from_secs(30 * 60);

/// Bound to the `resolve` call it was made for: the host refuses it once that call has ended.
#[derive(Debug, Clone)]
pub struct Fetch {
    pub(crate) peer: Peer,
    pub(crate) for_call: u32,
}

impl Fetch {
    fn opts(&self) -> CallOpts {
        CallOpts {
            deadline: Some(FETCH_DEADLINE),
            for_call: Some(self.for_call),
            handle: None,
        }
    }

    /// The full version the host's mirror maps a major to.
    pub async fn resolve_major(&self, engine: &str, major: &str) -> Result<String> {
        let args = ResolveMajor {
            engine: engine.to_string(),
            major: major.to_string(),
        };
        self.peer
            .call(Method::FetchResolveMajor, &args, self.opts())
            .await
    }

    /// Makes an exact build present and verified; returns its bin directory and digest.
    pub async fn ensure(
        &self,
        engine: &str,
        full: &str,
        target: &str,
        expected_sha: Option<&str>,
    ) -> Result<Ensured> {
        let args = Ensure {
            engine: engine.to_string(),
            full: full.to_string(),
            target: target.to_string(),
            expected_sha: expected_sha.map(str::to_string),
        };
        self.peer
            .call(Method::FetchEnsure, &args, self.opts())
            .await
    }
}
