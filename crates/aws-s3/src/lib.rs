//! S3: buckets, objects, versions, multipart uploads, tags and the bucket configurations, as a state machine behind a lazy actor per
//! region. Object bytes are blobs on disk written crash-safely and streamed in and out; metadata is in one redb file.
//!
//! Buckets are kept apart by region like every other service here (a bucket belongs to the region its creating request was signed
//! for; the home region is `us-east-1`), so the same bucket name can exist in two regions, which AWS's global namespace forbids.
//!
//! Operations that have no meaning on one machine (ACLs, archive storage classes and restores, S3 Select, directory buckets, metadata
//! tables, access points) are refused by name.

mod attrs;
mod authz;
mod blob;
mod buckets;
mod checksum;
mod complete;
mod config;
mod copy;
mod core;
mod delete;
mod download;
mod errors;
mod eventbridge;
mod events;
mod get;
mod idconfig;
mod janitor;
mod lifecycle;
mod lifecycle_check;
mod list;
mod listv;
mod model;
mod mplist;
mod multipart;
mod notify;
mod put;
mod receive;
mod req;
mod store;
mod tagging;
mod timeutil;
mod validate;
mod versioning;

use std::path::PathBuf;
use std::sync::Arc;

use aws_model_s3::Operation;
use aws_model_s3::meta::META;
use aws_model_s3::tables::TABLES;
use aws_schema::json::Violation;
use aws_wire::ApiError;
use aws_wire::rest::{RestReply, RestService, Upload};
use aws_wire::service::{BoxFuture, Call, Reply, Service};
use doze_fs::fs::RealFs;
use doze_runtime::{Clock, Ctx, Keyed, Tasks};

use crate::blob::Blobs;
use crate::checksum::Algo;
use crate::core::Core;
use crate::janitor::Janitor;
use crate::receive::{Declared, receive};
use crate::req::{Answer, Ask};

/// The region whose files are kept in the service's directory itself.
const HOME_REGION: &str = "us-east-1";

/// The S3 service. Nothing runs until the first request, except that a region left with lifecycle rules arms the janitor.
#[derive(Debug)]
pub struct S3 {
    inner: Arc<Inner>,
}

#[derive(Debug)]
pub(crate) struct Inner {
    actors: Keyed<Core>,
    dir: PathBuf,
    pub(crate) janitor: Janitor,
    tasks: Tasks,
    clock: Arc<dyn Clock>,
}

/// What a lifecycle sweep did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Swept {
    /// How many objects, versions and uploads the rules acted on.
    pub actions: usize,
    /// Whether any rule is left in the region (the janitor keeps visiting while one is).
    pub armed: bool,
}

impl S3 {
    /// Registers S3. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        let inner = Arc::new(Inner {
            dir: ctx.dir.clone(),
            tasks: ctx.tasks.clone(),
            clock: Arc::clone(&ctx.clock),
            janitor: Janitor::default(),
            actors: Keyed::new("s3", ctx, HOME_REGION),
        });
        for region in janitor::armed_regions(&inner.dir) {
            inner.arm(&region);
        }
        Self { inner }
    }

    /// Applies the lifecycle rules of `region` now, as the janitor does at midnight. For tests, and for a developer who does not
    /// want to wait a day.
    ///
    /// # Errors
    /// The region's service could not be reached.
    pub async fn sweep_now(&self, region: &str) -> Result<Swept, ApiError> {
        self.inner.sweep_now(region).await
    }
}

impl Inner {
    /// Arms the janitor for `region`, starting its task if it is not running.
    fn arm(self: &Arc<Self>, region: &str) {
        if self.janitor.arm(region) {
            if tokio::runtime::Handle::try_current().is_err() {
                self.janitor.disarm_all();
                return;
            }
            let (me, tasks, clock) = (
                Arc::clone(self),
                self.tasks.clone(),
                Arc::clone(&self.clock),
            );
            self.tasks.spawn(janitor::run(me, tasks, clock));
        }
    }

    pub(crate) async fn sweep_now(&self, region: &str) -> Result<Swept, ApiError> {
        let out = self.ask(region, Ask::Sweep).await?;
        Ok(Swept {
            actions: out
                .json
                .get("Actions")
                .and_then(serde_json::Value::as_u64)
                .map_or(0, |n| usize::try_from(n).unwrap_or(0)),
            armed: out
                .json
                .get("Armed")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        })
    }

    /// Asks the actor of the region the call was signed for.
    async fn ask(&self, region: &str, ask: Ask) -> Answer {
        let actor = self
            .actors
            .actor(region)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        actor
            .call(ask)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    }

    /// The blobs of a region, for writing a body before the actor is asked to keep it.
    fn blobs(&self, region: &str) -> Blobs<RealFs> {
        let dir = if region == HOME_REGION {
            self.dir.clone()
        } else {
            self.dir.join("regions").join(region)
        };
        Blobs::new(RealFs, &dir)
    }

    async fn run(
        self: &Arc<Self>,
        call: Call,
        upload: Option<Box<dyn Upload>>,
    ) -> Result<RestReply, ApiError> {
        let region = call.caller.region.clone();
        let op = Operation::from_index(call.op);
        let answer = match (op, upload) {
            (Some(Operation::PutObject | Operation::UploadPart), Some(mut upload)) => {
                self.stream_in(&region, call, upload.as_mut()).await?
            }
            (_, _) => self.ask(&region, Ask::Call(call, None)).await?,
        };
        if op == Some(Operation::PutBucketLifecycleConfiguration) {
            self.arm(&region);
        }
        Ok(RestReply {
            json: answer.json,
            status: answer.status,
            body: answer.body.map(download::stream),
        })
    }

    /// An operation that takes a body: the actor first checks the caller and the bucket, then the body is written and summed
    /// without the actor, then the actor keeps it.
    async fn stream_in(&self, region: &str, call: Call, upload: &mut dyn Upload) -> Answer {
        self.ask(region, Ask::Gate(call.duplicate())).await?;
        let v: serde_json::Value = serde_json::from_str(&call.body).unwrap_or_default();
        let mut declared = Declared::of(&v, upload.sha256().as_deref());
        // A part's CRC64NVME gives the whole object its checksum when the upload is completed.
        if Operation::from_index(call.op) == Some(Operation::UploadPart)
            && !declared.want.contains(&Algo::Crc64Nvme)
        {
            declared.want.push(Algo::Crc64Nvme);
        }
        let staged = receive(&self.blobs(region), upload, &declared).await?;
        let blobs = self.blobs(region);
        let kept = self
            .ask(
                region,
                Ask::Call(
                    call,
                    Some(req::Staged {
                        staging: staged.staging,
                        done: staged.done,
                    }),
                ),
            )
            .await;
        if kept.is_err() {
            // WHY: a staging file the actor did not take is swept at the next start; nothing refers to it.
            blobs.sweep(None);
        }
        kept
    }
}

impl Service for S3 {
    fn meta(&self) -> &'static aws_schema::meta::ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static aws_schema::tables::Tables {
        &TABLES
    }

    fn serves(&self, op: u32) -> bool {
        Operation::from_index(op).is_some_and(core::built)
    }

    /// A call from a peer (no stream): the same operations without a body.
    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(async move {
            Err(ApiError::new(
                501,
                "NotImplemented",
                format!("{} is served over HTTP only.", call.id),
            ))
        })
    }

    fn rest(&self) -> Option<&dyn RestService> {
        Some(self)
    }

    /// S3 words a request that breaks the model's rules as `InvalidArgument`.
    fn invalid(&self, violation: &Violation) -> ApiError {
        let member = |ix: u32| TABLES.member(ix).map_or("", |m| m.name);
        match violation {
            Violation::Missing { member: m, .. } => ApiError::new(
                400,
                "InvalidRequest",
                format!("Missing required parameter {}.", member(*m)),
            ),
            Violation::Syntax { .. } | Violation::TooDeep => errors::malformed_xml(),
            other => ApiError::new(
                400,
                "InvalidArgument",
                match other {
                    Violation::Length { member: m, .. }
                    | Violation::Range { member: m, .. }
                    | Violation::Pattern { member: m, .. }
                    | Violation::Enum { member: m, .. }
                    | Violation::Type { member: m, .. } => {
                        format!("Invalid value for {}.", member(*m))
                    }
                    _ => "The request is not valid.".to_owned(),
                },
            ),
        }
    }
}

impl RestService for S3 {
    fn call_rest(
        &self,
        call: Call,
        upload: Option<Box<dyn Upload>>,
    ) -> BoxFuture<'_, Result<RestReply, ApiError>> {
        Box::pin(self.inner.run(call, upload))
    }
}

#[cfg(test)]
mod tests;
