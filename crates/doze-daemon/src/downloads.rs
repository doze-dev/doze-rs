//! A service's `download` (D2; Go's `runtime.fetchDownload`): fetched by a `__fetch` child of this program (the daemon
//! keeps no HTTPS client), checked against its pin in doze.lock at every start, and pinned there the first time.

use doze_fetch::child::ChildDownloads;
use doze_fetch::lock::Lock;
use std::path::PathBuf;

/// What fetches the stack's downloads: the child, the project's doze.lock, and the doze home (whose lock on the file
/// every writer takes).
#[derive(Debug, Clone)]
pub struct ServiceDownloads {
    pub child: ChildDownloads,
    pub lock: PathBuf,
    pub home: PathBuf,
}

impl doze_engine::Fetch for ServiceDownloads {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
    ) -> doze_engine::stack::BoxFuture<'a, Result<PathBuf, String>> {
        Box::pin(async move {
            let pinned = Lock::load(&self.lock)?
                .downloads
                .get(url)
                .filter(|d| !d.is_empty())
                .cloned();
            let (dir, digest) = self
                .child
                .download(url, pinned.as_deref())
                .await
                .map_err(|e| e.message)?;
            if pinned.is_none() {
                doze_fetch::lock::update(&self.home, &self.lock, None, |l| {
                    l.record_download(url, &digest);
                    Ok(())
                })?;
            }
            Ok(dir)
        })
    }
}
