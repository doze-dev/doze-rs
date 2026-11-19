//! Service last accessed data: which services a user, group, role or managed policy can reach, and when each was last asked for. The
//! authorizer notes every question an IAM user or role asks (`note_access`), allowed or not, as AWS counts "all attempts" (the model);
//! the root and keys IAM did not issue are not noted, and with `--iam off` nothing is asked, so nothing is noted.
//!
//! A job is answered at once (`COMPLETED`, a snapshot taken when it is generated), where AWS takes a while.

use std::collections::BTreeSet;

use aws_iam_policy::Document;
use aws_peers::Need;
use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::errors::{invalid_input, no_such};
use crate::gather::{default_document, read};
use crate::grants::{self, service_name};
use crate::held::{Access, Job, ServiceUse, Use};
use crate::model::{Entity, Kind};
use crate::req::{Req, page, secs};

/// A use is written at most once a minute for each entity and service (and each action), as access keys' last use is.
const EVERY_MS: i64 = 60_000;
/// The most jobs kept: the oldest goes when a new one would pass it.
const MAX_JOBS: usize = 100;

fn key(entity: &str, namespace: &str) -> String {
    format!("{entity} {namespace}")
}

fn no_job(id: &str) -> ApiError {
    // (memory)
    no_such("job", id)
}

impl Core {
    /// Notes that `entity` (a user's or role's ARN) asked for `needs`.
    pub(crate) fn note_access(&mut self, entity: &str, needs: &[Need], region: &str) {
        let now = self.now_ms();
        for need in needs {
            let (ns, action) = need.action.split_once(':').unwrap_or((&need.action, ""));
            let ns = ns.to_ascii_lowercase();
            let k = key(entity, &ns);
            let used = Use {
                at_ms: now,
                region: region.to_owned(),
            };
            let fresh = |u: &Use| now - u.at_ms < EVERY_MS;
            match self.held.access.get(&k) {
                Some(a) if fresh(&a.last) && a.actions.get(action).is_some_and(fresh) => {}
                Some(_) => {
                    if let Some(a) = self.held.access.get_mut(&k) {
                        a.last = used.clone();
                        a.actions.insert(action.to_owned(), used);
                    }
                }
                None => {
                    let actions = [(action.to_owned(), used.clone())].into();
                    self.held.access.insert(
                        k,
                        Access {
                            last: used,
                            actions,
                        },
                    );
                }
            }
        }
    }

    /// A deleted user or role takes its use with it.
    pub(crate) fn forget_access(&mut self, entity: &str) {
        let prefix = format!("{entity} ");
        let keys: Vec<String> = self
            .held
            .access
            .with_prefix(&prefix)
            .map(|(k, _)| k.to_owned())
            .collect();
        for k in keys {
            self.held.access.remove(&k);
        }
    }

    /// The documents an identity (or a policy) grants with, and the users and roles whose use counts for it.
    fn subject_of(&self, arn: &str) -> Result<(Vec<Document>, Vec<String>), ApiError> {
        if arn.contains(":policy/") {
            let p = self
                .policies
                .get(arn)
                .ok_or_else(|| crate::errors::no_such_policy(arn))?;
            let docs = default_document(p).into_iter().collect();
            let holders = self
                .entities
                .iter()
                .map(|(_, e)| e)
                .filter(|e| e.attached.contains(arn))
                .flat_map(|e| self.members(e))
                .collect::<BTreeSet<String>>();
            return Ok((docs, holders.into_iter().collect()));
        }
        if !arn.starts_with("arn:aws:iam::") {
            return Err(invalid_input(format!("ARN {arn} is not valid.")));
        }
        let e = self.entity_of_arn(arn)?;
        let mut docs = Vec::new();
        self.documents_of(e, &mut docs);
        if e.kind == Kind::User {
            for g in e
                .groups
                .iter()
                .filter_map(|g| self.entity(Kind::Group, g).ok())
            {
                self.documents_of(g, &mut docs);
            }
        }
        Ok((docs, self.members(e)))
    }

    /// A user or role itself; a group's members.
    fn members(&self, e: &Entity) -> Vec<String> {
        match e.kind {
            Kind::User | Kind::Role => vec![e.arn.clone()],
            Kind::Group => self
                .entities
                .with_prefix("user/")
                .filter(|(_, u)| u.groups.contains(&e.name))
                .map(|(_, u)| u.arn.clone())
                .collect(),
        }
    }

    fn documents_of(&self, e: &Entity, out: &mut Vec<Document>) {
        out.extend(e.inline.values().filter_map(|t| read(t)));
        out.extend(
            e.attached
                .iter()
                .filter_map(|a| self.policies.get(a))
                .filter_map(default_document),
        );
    }

    pub(crate) fn generate_service_last_accessed(
        &mut self,
        r: &Req<'_>,
    ) -> Result<Value, ApiError> {
        let arn = r.s("Arn");
        let (docs, entities) = self.subject_of(arn)?;
        let mut namespaces = BTreeSet::new();
        for d in &docs {
            grants::namespaces(d, &mut namespaces);
        }
        let services = namespaces
            .into_iter()
            .map(|ns| self.service_use(&ns, &entities))
            .collect();
        let id = aws_seal::uuid_v4().ok_or_else(|| ApiError::internal("no randomness"))?;
        if self.held.jobs.len() >= MAX_JOBS
            && let Some(oldest) = self
                .held
                .jobs
                .iter()
                .min_by_key(|(_, j)| j.created_ms)
                .map(|(k, _)| k.to_owned())
        {
            self.held.jobs.remove(&oldest);
        }
        let job = Job {
            arn: arn.to_owned(),
            actions: r.s("Granularity") == "ACTION_LEVEL",
            created_ms: self.now_ms(),
            entities,
            services,
        };
        self.held.jobs.insert(id.clone(), job);
        Ok(json!({"JobId": id}))
    }

    /// One service's use by `entities`, newest first, with each action's last use.
    fn service_use(&self, ns: &str, entities: &[String]) -> ServiceUse {
        let mut used: Vec<(String, Use)> = Vec::new();
        let mut actions: Vec<(String, String, Use)> = Vec::new();
        for e in entities {
            let Some(a) = self.held.access.get(&key(e, ns)) else {
                continue;
            };
            used.push((e.clone(), a.last.clone()));
            for (name, u) in &a.actions {
                match actions.iter_mut().find(|(n, _, _)| n == name) {
                    Some(slot) if slot.2.at_ms < u.at_ms => {
                        *slot = (name.clone(), e.clone(), u.clone())
                    }
                    Some(_) => {}
                    None => actions.push((name.clone(), e.clone(), u.clone())),
                }
            }
        }
        used.sort_by_key(|(_, u)| std::cmp::Reverse(u.at_ms));
        actions.sort_by(|a, b| a.0.cmp(&b.0));
        ServiceUse {
            namespace: ns.to_owned(),
            entities: used,
            actions,
        }
    }

    fn job(&self, r: &Req<'_>) -> Result<&Job, ApiError> {
        self.held
            .jobs
            .get(r.s("JobId"))
            .ok_or_else(|| no_job(r.s("JobId")))
    }

    pub(crate) fn get_service_last_accessed(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let job = self.job(r)?;
        let items = job
            .services
            .iter()
            .map(|s| service_json(s, job.actions))
            .collect();
        let mut out = page(r, items, "ServicesLastAccessed")?;
        if let Value::Object(m) = &mut out {
            job_fields(m, job);
            m.insert(
                "JobType".into(),
                json!(if job.actions {
                    "ACTION_LEVEL"
                } else {
                    "SERVICE_LEVEL"
                }),
            );
        }
        Ok(out)
    }

    pub(crate) fn get_service_last_accessed_with_entities(
        &self,
        r: &Req<'_>,
    ) -> Result<Value, ApiError> {
        let job = self.job(r)?;
        let ns = r.s("ServiceNamespace");
        let service = job.services.iter().find(|s| s.namespace == ns);
        let mut rows: Vec<(Option<i64>, Value)> = job
            .entities
            .iter()
            .filter_map(|arn| self.entity_of_arn(arn).ok())
            .map(|e| {
                let at = service
                    .and_then(|s| s.entities.iter().find(|(a, _)| *a == e.arn))
                    .map(|(_, u)| u.at_ms);
                (at, entity_details(e, at))
            })
            .collect();
        // The most recent first, as the model says; never used last.
        rows.sort_by_key(|(at, _)| std::cmp::Reverse(at.unwrap_or(i64::MIN)));
        let mut out = page(
            r,
            rows.into_iter().map(|(_, v)| v).collect(),
            "EntityDetailsList",
        )?;
        if let Value::Object(m) = &mut out {
            job_fields(m, job);
        }
        Ok(out)
    }

    pub(crate) fn list_policies_granting_service_access(
        &self,
        r: &Req<'_>,
    ) -> Result<Value, ApiError> {
        let arn = r.s("Arn");
        if arn.contains(":policy/") {
            return Err(invalid_input(
                "ListPoliciesGrantingServiceAccess takes a user, group or role, not a policy.",
            ));
        }
        let e = self.entity_of_arn(arn)?;
        let mut holders = vec![e];
        if e.kind == Kind::User {
            holders.extend(
                e.groups
                    .iter()
                    .filter_map(|g| self.entity(Kind::Group, g).ok()),
            );
        }
        let items: Vec<Value> = r
            .strings("ServiceNamespaces")
            .iter()
            .map(|ns| {
                let policies: Vec<Value> =
                    holders.iter().flat_map(|h| self.granting(h, ns)).collect();
                json!({"ServiceNamespace": ns, "Policies": policies})
            })
            .collect();
        Ok(json!({"PoliciesGrantingServiceAccess": items, "IsTruncated": false}))
    }

    /// The policies of `e` itself that grant `ns`.
    fn granting(&self, e: &Entity, ns: &str) -> Vec<Value> {
        let entity_type = e.kind.word().to_ascii_uppercase();
        let inline = e
            .inline
            .iter()
            .filter(|(_, t)| read(t).is_some_and(|d| grants::grants(&d, ns)))
            .map(|(name, _)| json!({"PolicyName": name, "PolicyType": "INLINE", "EntityType": entity_type, "EntityName": e.name}));
        let managed = e
            .attached
            .iter()
            .filter_map(|a| self.policies.get(a))
            .filter(|p| default_document(p).is_some_and(|d| grants::grants(&d, ns)))
            .map(|p| json!({"PolicyName": p.name, "PolicyType": "MANAGED", "PolicyArn": p.arn}));
        inline.chain(managed).collect()
    }
}

fn job_fields(m: &mut Map<String, Value>, job: &Job) {
    m.insert("JobStatus".into(), json!("COMPLETED"));
    m.insert("JobCreationDate".into(), secs(job.created_ms));
    m.insert("JobCompletionDate".into(), secs(job.created_ms));
}

/// DynamoDB's tracked actions (recorded 2026-11-19).
const TRACKED_DYNAMODB: &[&str] = &[
    "CreateBackup",
    "CreateGlobalTable",
    "CreateTable",
    "DeleteBackup",
    "DeleteTable",
    "DescribeBackup",
    "DescribeContinuousBackups",
    "DescribeContributorInsights",
    "DescribeEndpoints",
    "DescribeExport",
    "DescribeGlobalTable",
    "DescribeGlobalTableSettings",
    "DescribeImport",
    "DescribeKinesisStreamingDestination",
    "DescribeLimits",
    "DescribeStream",
    "DescribeTable",
    "DescribeTableReplicaAutoScaling",
    "DescribeTimeToLive",
    "DisableKinesisStreamingDestination",
    "EnableKinesisStreamingDestination",
    "ExportTableToPointInTime",
    "GetResourcePolicy",
    "ImportTable",
    "ListBackups",
    "ListBulkOperations",
    "ListContributorInsights",
    "ListExports",
    "ListGlobalTables",
    "ListImports",
    "ListStreams",
    "ListTables",
    "RestoreTableFromBackup",
    "RestoreTableToPointInTime",
    "UpdateContinuousBackups",
    "UpdateContributorInsights",
    "UpdateGlobalTable",
    "UpdateGlobalTableSettings",
    "UpdateKinesisStreamingDestination",
    "UpdateTable",
    "UpdateTableReplicaAutoScaling",
    "UpdateTimeToLive",
];
/// S3's tracked actions (recorded 2026-11-19).
const TRACKED_S3: &[&str] = &[
    "GetAccelerateConfiguration",
    "GetAccessGrant",
    "GetAccessGrantsInstance",
    "GetAccessGrantsInstanceForPrefix",
    "GetAccessGrantsInstanceResourcePolicy",
    "GetAccessGrantsLocation",
    "GetAccessPoint",
    "GetAccessPointConfigurationForObjectLambda",
    "GetAccessPointForObjectLambda",
    "GetAccessPointPolicy",
    "GetAccessPointPolicyForObjectLambda",
    "GetAccessPointPolicyStatus",
    "GetAccessPointPolicyStatusForObjectLambda",
    "GetAccountPublicAccessBlock",
    "GetAnalyticsConfiguration",
    "GetBucketAbac",
    "GetBucketAcl",
    "GetBucketCORS",
    "GetBucketLocation",
    "GetBucketLogging",
    "GetBucketNotification",
    "GetBucketObjectLockConfiguration",
    "GetBucketOwnershipControls",
    "GetBucketPolicy",
    "GetBucketPolicyStatus",
    "GetBucketPublicAccessBlock",
    "GetBucketRequestPayment",
    "GetBucketVersioning",
    "GetBucketWebsite",
    "GetDataAccess",
    "GetEncryptionConfiguration",
    "GetIntelligentTieringConfiguration",
    "GetInventoryConfiguration",
    "GetLifecycleConfiguration",
    "GetMetricsConfiguration",
    "GetMultiRegionAccessPoint",
    "GetMultiRegionAccessPointPolicy",
    "GetMultiRegionAccessPointPolicyStatus",
    "GetMultiRegionAccessPointRoutes",
    "GetReplicationConfiguration",
    "GetStorageLensConfiguration",
    "GetStorageLensDashboard",
];

/// The actions AWS tracks for a service in an `ACTION_LEVEL` report, listed whether asked for or not. Recorded 2026-11-19 for
/// DynamoDB and S3 (42 each, in this order); AWS tracks others (IAM, EC2, Lambda) whose lists were not recorded.
fn tracked_actions(namespace: &str) -> &'static [&'static str] {
    match namespace {
        "dynamodb" => TRACKED_DYNAMODB,
        "s3" => TRACKED_S3,
        _ => &[],
    }
}

fn service_json(s: &ServiceUse, actions: bool) -> Value {
    let mut m = Map::new();
    m.insert("ServiceName".into(), json!(service_name(&s.namespace)));
    m.insert("ServiceNamespace".into(), json!(s.namespace));
    if let Some((entity, u)) = s.entities.first() {
        m.insert("LastAuthenticated".into(), secs(u.at_ms));
        m.insert("LastAuthenticatedEntity".into(), json!(entity));
        m.insert("LastAuthenticatedRegion".into(), json!(u.region));
    }
    // Recorded 2026-11-19: the count is there, 0, for a service no one asked for.
    m.insert("TotalAuthenticatedEntities".into(), json!(s.entities.len()));
    if actions {
        // A tracked action no one asked for is listed by name alone (recorded 2026-11-19); one asked for says who, when and where.
        // Actions asked for of a service with no recorded list are listed too, where AWS may track none.
        let mut names: BTreeSet<&str> = tracked_actions(&s.namespace).iter().copied().collect();
        names.extend(s.actions.iter().map(|(name, _, _)| name.as_str()));
        let tracked: Vec<Value> = names
            .into_iter()
            .map(|name| match s.actions.iter().find(|(n, _, _)| n == name) {
                Some((_, entity, u)) => json!({"ActionName": name, "LastAccessedEntity": entity, "LastAccessedTime": secs(u.at_ms), "LastAccessedRegion": u.region}),
                None => json!({"ActionName": name}),
            })
            .collect();
        if !tracked.is_empty() {
            m.insert("TrackedActionsLastAccessed".into(), Value::Array(tracked));
        }
    }
    Value::Object(m)
}

fn entity_details(e: &Entity, at: Option<i64>) -> Value {
    let mut m = Map::new();
    m.insert(
        "EntityInfo".into(),
        json!({"Arn": e.arn, "Name": e.name, "Type": e.kind.word().to_ascii_uppercase(), "Id": e.id, "Path": e.path}),
    );
    if let Some(at) = at {
        m.insert("LastAuthenticated".into(), secs(at));
    }
    Value::Object(m)
}

#[cfg(test)]
mod tracked {
    /// The report lists tracked actions in sorted order; the recorded lists must already be in it, so none is reordered.
    #[test]
    fn the_recorded_lists_are_in_the_order_aws_gave_them() {
        for ns in ["dynamodb", "s3"] {
            let list = super::tracked_actions(ns);
            assert_eq!(list.len(), 42, "{ns}");
            assert!(list.is_sorted(), "{ns}");
        }
    }
}
