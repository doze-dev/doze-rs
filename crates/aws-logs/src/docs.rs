//! The documents that are stored and returned and applied to nothing: resource policies, account policies, query definitions and
//! data protection policies. One table of (kind, id) to the document as sent, shown the way the API shows it. The flag a client
//! cannot see is that no policy here is evaluated and no query definition is run: Insights is not built.

use aws_model_cloudwatch_logs::Operation;
use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::{Doc, doc_key};
use crate::req::{Put, Req, name_of_identifier, no_group};

const RESOURCE_POLICY: &str = "resource-policy";
const ACCOUNT_POLICY: &str = "account-policy";
const QUERY: &str = "query-definition";
const PROTECTION: &str = "data-protection";

impl Core {
    pub(crate) fn run_docs(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        use Operation as O;
        match op {
            O::PutResourcePolicy => self.put_resource_policy(r),
            O::DescribeResourcePolicies => {
                Ok(self.list_docs(RESOURCE_POLICY, "resourcePolicies", r, |_| true))
            }
            O::DeleteResourcePolicy => self.delete_doc(RESOURCE_POLICY, r.s("policyName")),
            O::PutAccountPolicy => self.put_account_policy(r),
            O::DescribeAccountPolicies => {
                let (kind, name) = (
                    r.s("policyType").to_owned(),
                    r.opt("policyName").map(str::to_owned),
                );
                Ok(self.list_docs(ACCOUNT_POLICY, "accountPolicies", r, |d| {
                    d.get("policyType").and_then(Value::as_str) == Some(kind.as_str())
                        && name
                            .as_deref()
                            .is_none_or(|n| d.get("policyName").and_then(Value::as_str) == Some(n))
                }))
            }
            O::DeleteAccountPolicy => {
                let id = format!("{}/{}", r.s("policyType"), r.s("policyName"));
                self.delete_doc(ACCOUNT_POLICY, &id)
            }
            O::PutQueryDefinition => self.put_query(r),
            O::DescribeQueryDefinitions => {
                let prefix = r.opt("queryDefinitionNamePrefix").map(str::to_owned);
                Ok(self.list_docs(QUERY, "queryDefinitions", r, |d| {
                    prefix.as_deref().is_none_or(|p| {
                        d.get("name")
                            .and_then(Value::as_str)
                            .is_some_and(|n| n.starts_with(p))
                    })
                }))
            }
            O::DeleteQueryDefinition => {
                let found = self
                    .docs
                    .remove(&doc_key(QUERY, r.s("queryDefinitionId")))
                    .is_some();
                Ok(json!({"success": found}))
            }
            O::PutDataProtectionPolicy => self.put_protection(r),
            O::GetDataProtectionPolicy => self.get_protection(r),
            O::DeleteDataProtectionPolicy => {
                let name = name_of_identifier(r.s("logGroupIdentifier"));
                self.delete_doc(PROTECTION, &name)
            }
            _ => Err(crate::core::not_built()),
        }
    }

    fn store(&mut self, kind: &str, id: &str, doc: &Value) {
        let json = doc.to_string();
        let updated_ms = self.now_ms();
        self.docs
            .insert(doc_key(kind, id), Doc { json, updated_ms });
    }

    fn delete_doc(&mut self, kind: &str, id: &str) -> Result<Value, ApiError> {
        match self.docs.remove(&doc_key(kind, id)) {
            Some(_) => Ok(json!({})),
            None => Err(ApiError::modeled(
                "ResourceNotFoundException",
                "The specified policy does not exist.",
            )),
        }
    }

    /// The documents of one kind that `keep` accepts, under `member`, in id order.
    fn list_docs(
        &self,
        kind: &str,
        member: &str,
        r: &Req<'_>,
        keep: impl Fn(&Value) -> bool,
    ) -> Value {
        let prefix = doc_key(kind, "");
        let items: Vec<(String, Value)> = self
            .docs
            .with_prefix(&prefix)
            .filter_map(|(k, d)| {
                Some((
                    k.strip_prefix(&prefix)?.to_owned(),
                    serde_json::from_str(&d.json).ok()?,
                ))
            })
            .filter(|(_, v)| keep(v))
            .collect();
        let limit = r
            .int("limit")
            .or_else(|| r.int("maxResults"))
            .map_or(50, |n| usize::try_from(n).unwrap_or(50));
        let after = r.opt("nextToken");
        let rest: Vec<&(String, Value)> = items
            .iter()
            .filter(|(id, _)| after.is_none_or(|a| id.as_str() > a))
            .collect();
        let mut out =
            json!({ member: rest.iter().take(limit).map(|(_, v)| v.clone()).collect::<Vec<_>>() });
        if let (true, Some((id, _))) = (rest.len() > limit, rest.get(limit.saturating_sub(1))) {
            out.put("nextToken", json!(id));
        }
        out
    }

    fn put_resource_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let id = r
            .opt("resourceArn")
            .or(r.opt("policyName"))
            .unwrap_or("default")
            .to_owned();
        let policy = json!({
            "policyName": r.opt("policyName").unwrap_or_default(),
            "policyDocument": r.s("policyDocument"),
            "lastUpdatedTime": self.now_ms(),
            "policyScope": if r.opt("resourceArn").is_some() { "RESOURCE" } else { "ACCOUNT" },
            "revisionId": format!("{:020}", self.now_ms()),
        });
        let mut policy = policy;
        if let Some(arn) = r.opt("resourceArn") {
            policy.put("resourceArn", json!(arn));
        }
        self.store(RESOURCE_POLICY, &id, &policy);
        let revision = policy.get("revisionId").cloned().unwrap_or_default();
        Ok(json!({"resourcePolicy": policy, "revisionId": revision}))
    }

    fn put_account_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let mut policy = json!({
            "policyName": r.s("policyName"),
            "policyDocument": r.s("policyDocument"),
            "lastUpdatedTime": self.now_ms(),
            "policyType": r.s("policyType"),
            "scope": r.opt("scope").unwrap_or("ALL"),
            "accountId": r.account(),
        });
        if let Some(s) = r.opt("selectionCriteria") {
            policy.put("selectionCriteria", json!(s));
        }
        let id = format!("{}/{}", r.s("policyType"), r.s("policyName"));
        self.store(ACCOUNT_POLICY, &id, &policy);
        Ok(json!({"accountPolicy": policy}))
    }

    fn put_query(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let id = r
            .opt("queryDefinitionId")
            .map(str::to_owned)
            .or_else(aws_seal::uuid_v4)
            .unwrap_or_default();
        let mut def = json!({
            "queryDefinitionId": id,
            "name": r.s("name"),
            "queryString": r.s("queryString"),
            "lastModified": self.now_ms(),
            "queryLanguage": r.opt("queryLanguage").unwrap_or("CWLI"),
        });
        if let Some(v) = r.v.get("logGroupNames") {
            def.put("logGroupNames", v.clone());
        }
        if let Some(v) = r.v.get("parameters") {
            def.put("parameters", v.clone());
        }
        self.store(QUERY, &id, &def);
        Ok(json!({"queryDefinitionId": id}))
    }

    fn put_protection(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = name_of_identifier(r.s("logGroupIdentifier"));
        if self.groups.get(&name).is_none() {
            return Err(no_group());
        }
        let doc = json!({
            "logGroupIdentifier": r.s("logGroupIdentifier"),
            "policyDocument": r.s("policyDocument"),
            "lastUpdatedTime": self.now_ms(),
        });
        self.store(PROTECTION, &name, &doc);
        Ok(doc)
    }

    fn get_protection(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = name_of_identifier(r.s("logGroupIdentifier"));
        if self.groups.get(&name).is_none() {
            return Err(no_group());
        }
        // (memory)
        self.docs
            .get(&doc_key(PROTECTION, &name))
            .and_then(|d| serde_json::from_str(&d.json).ok())
            .ok_or_else(|| {
                ApiError::modeled(
                    "ResourceNotFoundException",
                    "The specified data protection policy does not exist.",
                )
            })
    }
}
