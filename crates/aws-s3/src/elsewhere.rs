//! A bucket the request's Region does not have, though another Region of this stack does (doze only: here the same bucket name can live
//! in two Regions, where AWS's namespace is global). The error answers as before, with a sentence added that says where the bucket is, so
//! a client whose Region differs from the one the bucket was declared in is told so.

use aws_wire::ApiError;

use crate::Inner;
use crate::req::Ask;

impl Inner {
    /// `error`, a `NoSuchBucket` for the Region `region`, with the other Regions that hold the bucket named in it. The lookup runs on
    /// this path alone, and only in Regions that already have data.
    pub(crate) async fn hint_regions(&self, region: &str, mut error: ApiError) -> ApiError {
        let Some(name) = error
            .extra
            .iter()
            .find(|(k, _)| k == "BucketName")
            .map(|(_, v)| v.clone())
        else {
            return error;
        };
        let mut found = Vec::new();
        for key in self.actors.keys_on_disk() {
            if key != region && self.ask(&key, Ask::Configs(name.clone())).await.is_ok() {
                found.push(key);
            }
        }
        if !found.is_empty() {
            error.note_other_regions("bucket", &name, &found, region);
        }
        error
    }
}
