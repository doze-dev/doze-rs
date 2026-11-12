//! The state a region's S3 owns, the dispatch from an operation to the code that does it, and what happens around every call:
//! authorize, run, commit what changed, answer only once it is on disk.

use std::collections::HashSet;
use std::sync::Arc;

use aws_model_s3::Operation;
use aws_peers::{Mode, Peers};
use aws_wire::ApiError;
use doze_fs::fs::{Fs, RealFs};
use doze_runtime::{Clock, Ctx};
use doze_store::records::{Changes, Records};
use doze_store::{Db, StoreError};

use crate::blob::Blobs;
use crate::model::{Bucket, PartRec, Upload};
use crate::req::{Answer, Ask, Out, Req};

pub(crate) const FILE: &str = "s3.redb";
pub(crate) const BUCKETS: &str = "buckets";
pub(crate) const UPLOADS: &str = "uploads";
pub(crate) const PARTS: &str = "parts";
pub(crate) const VERSIONS: &str = "versions";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
pub(crate) const VERSION: u8 = 1;
/// The region whose files are kept in the service's directory itself.
pub(crate) const HOME: &str = "us-east-1";
/// Present while the service is open: finding it at start means the last run did not close.
const RUNNING: &str = "running";

/// The rows of the versions table a call changed.
#[derive(Debug, Default)]
pub(crate) struct Batch {
    pub(crate) puts: Vec<(String, Vec<u8>)>,
    pub(crate) deletes: Vec<String>,
}

/// All of one region's S3. Owned by one task, so no operation needs a lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) buckets: Records<Bucket>,
    pub(crate) uploads: Records<Upload>,
    pub(crate) parts: Records<PartRec>,
    pub(crate) batch: Batch,
    pub(crate) db: Db,
    pub(crate) blobs: Blobs<RealFs>,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) dir: std::path::PathBuf,
    /// Blobs to remove once the rows that referred to them are committed.
    pub(crate) doomed: Vec<String>,
}

/// The operations this service runs. The others are refused by name (501, `X-Doze-Not-Built`) before their request is read: ACLs
/// (a local account has one owner), archive storage classes and restores, S3 Select, torrents, directory buckets and Express,
/// metadata tables and annotations, access points and the rest of what has no local meaning.
pub(crate) fn built(op: Operation) -> bool {
    use Operation as O;
    matches!(
        op,
        O::CreateBucket
            | O::DeleteBucket
            | O::HeadBucket
            | O::ListBuckets
            | O::GetBucketLocation
            | O::PutObject
            | O::GetObject
            | O::HeadObject
            | O::DeleteObject
            | O::DeleteObjects
            | O::CopyObject
            | O::ListObjects
            | O::ListObjectsV2
            | O::ListObjectVersions
            | O::GetObjectAttributes
            | O::GetObjectTagging
            | O::PutObjectTagging
            | O::DeleteObjectTagging
            | O::GetBucketTagging
            | O::PutBucketTagging
            | O::DeleteBucketTagging
            | O::CreateMultipartUpload
            | O::UploadPart
            | O::UploadPartCopy
            | O::CompleteMultipartUpload
            | O::AbortMultipartUpload
            | O::ListParts
            | O::ListMultipartUploads
            | O::GetBucketVersioning
            | O::PutBucketVersioning
    ) || crate::config::kind_of(op).is_some()
}

impl doze_runtime::Service for Core {
    type Request = Ask;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        Self::open_in(
            &ctx.dir,
            Arc::clone(&ctx.clock),
            ctx.ext
                .as_ref()
                .and_then(|e| Arc::clone(e).downcast::<Peers>().ok()),
        )
    }

    async fn handle(&mut self, ask: Ask) -> Answer {
        let (call, staged, gate) = match ask {
            Ask::Gate(call) => (call, None, true),
            Ask::Call(call, staged) => (call, staged, false),
        };
        let req = Req::new(&call, staged);
        self.authorize(&req).await?;
        if gate {
            return self.gate(&req);
        }
        let result = self.dispatch(req).await;
        self.commit(result).await
    }

    async fn close(self) {
        self.blobs.fs().remove_file(&self.dir.join(RUNNING)).ok();
        self.db.close();
    }
}

impl Core {
    pub(crate) fn open_in(
        dir: &std::path::Path,
        clock: Arc<dyn Clock>,
        peers: Option<Arc<Peers>>,
    ) -> Result<Self, StoreError> {
        let db = Db::open(dir, FILE, &[BUCKETS, UPLOADS, PARTS, VERSIONS])?;
        let read = db.read()?;
        let buckets = Records::load(BUCKETS, "bucket", VERSION, &read)?;
        let uploads = Records::load(UPLOADS, "upload", VERSION, &read)?;
        let parts = Records::load(PARTS, "part", VERSION, &read)?;
        drop(read);
        let blobs = Blobs::new(RealFs, dir);
        let core = Self {
            buckets,
            uploads,
            parts,
            batch: Batch::default(),
            db,
            blobs,
            peers,
            clock,
            dir: dir.to_path_buf(),
            doomed: Vec::new(),
        };
        core.start_up();
        Ok(core)
    }

    /// Sweeps what an earlier run left: staging files always, and after an unclean stop the blobs nothing refers to.
    fn start_up(&self) {
        let marker = self.dir.join(RUNNING);
        let unclean = RealFs.open_existing(&marker).is_ok();
        let live = unclean.then(|| self.live_blobs());
        self.blobs.sweep(live.as_ref());
        // WHY: without the marker every start would sweep everything; a marker that cannot be made only costs a sweep next time.
        RealFs.open_or_create(&marker).ok();
    }

    /// Every blob a version, an upload part refers to.
    fn live_blobs(&self) -> HashSet<String> {
        let mut live: HashSet<String> = self.parts.iter().map(|(_, p)| p.blob.clone()).collect();
        let scanned = self.db.scan(VERSIONS, "", |_, bytes| {
            if let Ok(v) = doze_store::record::decode::<crate::model::Version>("version", VERSION, bytes) {
                live.extend(v.segs.into_iter().map(|s| s.blob));
            }
            true
        });
        if scanned.is_err() {
            // WHY: an unreadable table must not lead to every blob being swept; a full set is every name on disk.
            return HashSet::new();
        }
        live
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Asks the authorizer, when there is one and it is switched on.
    async fn authorize(&self, req: &Req<'_>) -> Result<(), ApiError> {
        let Some(peers) = self.peers.as_ref().filter(|p| p.mode() != Mode::Off) else {
            return Ok(());
        };
        peers.authorize(&req.call.caller, self.needs(req)).await
    }

    /// Writes what the call changed and waits until it is on disk; a call that failed changes nothing.
    async fn commit(&mut self, result: Answer) -> Answer {
        let doomed = std::mem::take(&mut self.doomed);
        if result.is_err() {
            self.rollback();
            return result;
        }
        let ack = self.flush().map_err(|e| ApiError::internal(format!("could not save: {e}")))?;
        if let Some(ack) = ack {
            ack.await.map_err(|e| ApiError::internal(format!("could not save: {e}")))?;
        }
        for blob in doomed {
            self.blobs.remove(&blob);
        }
        result
    }

    /// Forgets the changes of a call that failed.
    fn rollback(&mut self) {
        self.batch = Batch::default();
        self.doomed.clear();
        // WHY: the records noted their changes as they were made; staging them and dropping the rows is how they forget. A record
        // changed in memory by a call that then failed is rewritten from disk at the next start; failing calls change nothing
        // before their last step by construction (they check, then change).
        self.buckets.stage().ok();
        self.uploads.stage().ok();
        self.parts.stage().ok();
    }

    fn flush(&mut self) -> Result<Option<doze_store::Ack>, StoreError> {
        let batch = std::mem::take(&mut self.batch);
        let changes = vec![
            self.buckets.stage()?,
            self.uploads.stage()?,
            self.parts.stage()?,
            Changes::of_rows(VERSIONS, batch.puts, batch.deletes),
        ];
        Ok(self.db.submit(changes))
    }

    /// Every operation of the model is either run here or refused by name.
    async fn dispatch(&mut self, req: Req<'_>) -> Answer {
        use Operation as O;
        let Some(op) = Operation::from_index(req.call.op).filter(|op| built(*op)) else {
            return Err(ApiError::new(501, "NotImplemented", "This operation is not built in doze-aws."));
        };
        match op {
            O::CreateBucket => self.create_bucket(&req),
            O::DeleteBucket => self.delete_bucket(&req),
            O::HeadBucket => self.head_bucket(&req),
            O::ListBuckets => self.list_buckets(&req),
            O::GetBucketLocation => self.get_bucket_location(&req),
            O::PutObject => self.put_object(req).await,
            O::GetObject | O::HeadObject => self.get_object(&req),
            O::DeleteObject => self.delete_object(&req),
            O::DeleteObjects => self.delete_objects(&req),
            O::CopyObject => self.copy_object(&req).await,
            O::ListObjects | O::ListObjectsV2 => self.list_objects(&req, op == O::ListObjectsV2),
            O::ListObjectVersions => self.list_object_versions(&req),
            O::GetObjectAttributes => self.get_object_attributes(&req),
            O::GetObjectTagging | O::GetBucketTagging => self.get_tagging(&req, op == O::GetBucketTagging),
            O::PutObjectTagging | O::PutBucketTagging => self.put_tagging(&req, op == O::PutBucketTagging),
            O::DeleteObjectTagging | O::DeleteBucketTagging => {
                self.delete_tagging(&req, op == O::DeleteBucketTagging)
            }
            O::GetBucketVersioning => self.get_versioning(&req),
            O::PutBucketVersioning => self.put_versioning(&req),
            O::CreateMultipartUpload => self.create_upload(&req),
            O::UploadPart => self.upload_part(req),
            O::UploadPartCopy => self.upload_part_copy(&req).await,
            O::CompleteMultipartUpload => self.complete_upload(&req),
            O::AbortMultipartUpload => self.abort_upload(&req),
            O::ListParts => self.list_parts(&req),
            O::ListMultipartUploads => self.list_uploads(&req),
            other => self.configuration(&req, other).await,
        }
    }

    /// The checks that come before a body is read: the bucket is there.
    fn gate(&self, req: &Req<'_>) -> Answer {
        self.bucket(req.bucket())?;
        Out::empty()
    }

    pub(crate) fn bucket(&self, name: &str) -> Result<&Bucket, ApiError> {
        self.buckets
            .get(name)
            .ok_or_else(|| crate::errors::no_such_bucket(name))
    }
}
