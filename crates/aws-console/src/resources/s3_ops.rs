//! What a bucket's pages (objects, properties, permissions, management) may ask of S3: one list of operations a tab, each bound
//! to this bucket (`{"Bucket":"{id}"}`), so a call acts on this bucket and on no other. A section's edit is the `Put…` of its
//! setting (the page reads with the `Get…` first and sends the whole configuration, as S3 takes it); `Delete…` is the reset.

use super::{Deep, Op, op};

/// What every call of a bucket page names: this bucket.
pub(super) const BUCKET: &str = r#"{"Bucket":"{id}"}"#;

const OBJECTS: &[Op] = &[
    op("ListObjectsV2", BUCKET),
    op("ListObjectVersions", BUCKET),
    op("HeadObject", BUCKET),
    op("GetObjectTagging", BUCKET),
    op("DeleteObject", BUCKET),
    op("DeleteObjects", BUCKET),
];

const PROPERTIES: &[Op] = &[
    op("GetBucketVersioning", BUCKET),
    op("PutBucketVersioning", BUCKET),
    op("GetBucketEncryption", BUCKET),
    op("PutBucketEncryption", BUCKET),
    op("GetBucketNotificationConfiguration", BUCKET),
    op("PutBucketNotificationConfiguration", BUCKET),
    op("GetObjectLockConfiguration", BUCKET),
    op("GetBucketWebsite", BUCKET),
    op("PutBucketWebsite", BUCKET),
    op("DeleteBucketWebsite", BUCKET),
];

const PERMISSIONS: &[Op] = &[
    op("GetPublicAccessBlock", BUCKET),
    op("PutPublicAccessBlock", BUCKET),
    op("GetBucketPolicy", BUCKET),
    op("PutBucketPolicy", BUCKET),
    op("DeleteBucketPolicy", BUCKET),
    op("GetBucketOwnershipControls", BUCKET),
    op("PutBucketOwnershipControls", BUCKET),
    op("GetBucketAcl", BUCKET),
    op("GetBucketCors", BUCKET),
    op("PutBucketCors", BUCKET),
    op("DeleteBucketCors", BUCKET),
];

const MANAGEMENT: &[Op] = &[
    op("GetBucketLifecycleConfiguration", BUCKET),
    op("PutBucketLifecycleConfiguration", BUCKET),
    op("DeleteBucketLifecycle", BUCKET),
];

/// The pages of a bucket, as tabs: its objects, then what is set on it.
pub(super) const PAGES: &[Deep] = &[
    Deep {
        id: "objects",
        title: "Objects",
        ops: OBJECTS,
    },
    Deep {
        id: "properties",
        title: "Properties",
        ops: PROPERTIES,
    },
    Deep {
        id: "permissions",
        title: "Permissions",
        ops: PERMISSIONS,
    },
    Deep {
        id: "management",
        title: "Management",
        ops: MANAGEMENT,
    },
];
