//! A server whose clock the test moves, for the tests that need time to pass (Object Lock retention, lifecycle).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use aws_gateway::{Config, Gateway, Running};
use aws_s3::S3;
use doze_runtime::{Ctx, ManualClock, Tasks};

use super::{Reply, request};

pub(crate) const DAY_MS: i64 = 86_400_000;
/// 2023-11-14T22:13:20Z.
pub(crate) const START_MS: i64 = 1_700_000_000_000;
/// About five days after the start.
pub(crate) const SOON: &str = "2023-11-20T00:00:00Z";
/// About ten days after the start.
pub(crate) const LATER: &str = "2023-11-25T00:00:00Z";
pub(crate) const BUCKET_LOCK: (&str, &str) = ("x-amz-bucket-object-lock-enabled", "true");
pub(crate) const BYPASS: (&str, &str) = ("x-amz-bypass-governance-retention", "true");

pub(crate) struct Lab {
    pub(crate) addr: SocketAddr,
    pub(crate) clock: Arc<ManualClock>,
    pub(crate) s3: Arc<S3>,
    running: Running,
    tasks: Tasks,
}

pub(crate) async fn lab(name: &str) -> Lab {
    let dir = std::env::temp_dir().join(format!("aws-s3-lock-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let clock = Arc::new(ManualClock::at(START_MS));
    let tasks = Tasks::new();
    let ctx = Arc::new(Ctx {
        dir,
        idle: Duration::from_secs(300),
        tasks: tasks.clone(),
        clock: clock.clone(),
        ext: None,
    });
    let s3 = Arc::new(S3::new(ctx));
    let running = Gateway::new(Config::new("127.0.0.1:0".parse().unwrap()))
        .with(s3.clone())
        .bind()
        .await
        .unwrap();
    Lab {
        addr: running.local_addr(),
        clock,
        s3,
        running,
        tasks,
    }
}

impl Lab {
    pub(crate) async fn call(
        &self,
        method: &str,
        path: &str,
        extra: &[(&str, &str)],
        body: &str,
    ) -> Reply {
        request(self.addr, method, "127.0.0.1", path, extra, body.as_bytes()).await
    }

    /// Puts an object and returns its version id.
    pub(crate) async fn put(&self, path: &str, extra: &[(&str, &str)]) -> String {
        let r = self.call("PUT", path, extra, "data").await;
        assert_eq!(r.status, 200, "{}", r.text());
        r.header("x-amz-version-id").unwrap_or_default().to_owned()
    }

    pub(crate) async fn lock_bucket(&self, name: &str) {
        let r = self
            .call("PUT", &format!("/{name}"), &[BUCKET_LOCK], "")
            .await;
        assert_eq!(r.status, 200, "{}", r.text());
    }

    /// Sets the retention of `path` (which may already carry `?versionId=`).
    pub(crate) async fn retain(
        &self,
        path: &str,
        extra: &[(&str, &str)],
        mode: &str,
        until: &str,
    ) -> Reply {
        let body = format!(
            "<Retention><Mode>{mode}</Mode><RetainUntilDate>{until}</RetainUntilDate></Retention>"
        );
        let sep = if path.contains('?') { '&' } else { '?' };
        self.call("PUT", &format!("{path}{sep}retention"), extra, &body)
            .await
    }

    pub(crate) async fn stop(self) {
        self.running.shutdown().await;
        self.tasks.shutdown().await;
    }
}

/// An `AccessDenied` answer.
pub(crate) fn denied(r: &Reply) -> bool {
    r.status == 403 && r.text().contains("<Code>AccessDenied</Code>")
}
