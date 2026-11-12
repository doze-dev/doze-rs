//! Resource policies: the statements `AddPermission` writes, and the whole-document form (`PutResourcePolicy`). The policy is what a
//! service principal (S3, SNS, EventBridge) is checked against when it invokes the function.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::{Core, Out};
use crate::errors::{conflict, invalid, not_found, precondition};
use crate::functions::uuid;
use crate::req::Req;

/// The `Principal` of a statement for the string a caller gave.
fn principal(p: &str) -> Value {
    if p == "*" {
        json!("*")
    } else if p.ends_with(".amazonaws.com") || p.ends_with(".amazonaws.com.cn") {
        json!({"Service": p})
    } else if p.len() == 12 && p.bytes().all(|b| b.is_ascii_digit()) {
        json!({"AWS": format!("arn:aws:iam::{p}:root")})
    } else {
        json!({"AWS": p})
    }
}

/// One statement from an `AddPermission` request, naming `resource`.
fn statement(r: &Req, resource: &str) -> Value {
    let mut s = Map::new();
    s.insert("Sid".into(), json!(r.text("StatementId")));
    s.insert("Effect".into(), json!("Allow"));
    s.insert("Principal".into(), principal(r.text("Principal")));
    s.insert("Action".into(), json!(r.text("Action")));
    s.insert("Resource".into(), json!(resource));
    let mut condition = Map::new();
    if let Some(arn) = r.opt("SourceArn") {
        condition.insert("ArnLike".into(), json!({"AWS:SourceArn": arn}));
    }
    let mut equals = Map::new();
    if let Some(a) = r.opt("SourceAccount") {
        equals.insert("AWS:SourceAccount".into(), json!(a));
    }
    if let Some(o) = r.opt("PrincipalOrgID") {
        equals.insert("aws:PrincipalOrgID".into(), json!(o));
    }
    if let Some(t) = r.opt("EventSourceToken") {
        equals.insert("lambda:EventSourceToken".into(), json!(t));
    }
    if let Some(t) = r.opt("FunctionUrlAuthType") {
        equals.insert("lambda:FunctionUrlAuthType".into(), json!(t));
    }
    if !equals.is_empty() {
        condition.insert("StringEquals".into(), Value::Object(equals));
    }
    if !condition.is_empty() {
        s.insert("Condition".into(), Value::Object(condition));
    }
    Value::Object(s)
}

fn document(statements: Vec<Value>) -> Value {
    json!({"Version": "2012-10-17", "Id": "default", "Statement": statements})
}

fn statements_of(doc: &str) -> Vec<Value> {
    serde_json::from_str::<Value>(doc)
        .ok()
        .and_then(|d| d.get("Statement").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

const NO_POLICY: &str = "The resource you requested does not exist.";

impl Core {
    pub(crate) fn add_permission(&mut self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let key = f.qualifier.clone().unwrap_or_default();
        let sid = r.text("StatementId");
        let mut statements = function.policies.get(&key).map(|d| statements_of(d)).unwrap_or_default();
        if statements.iter().any(|s| s.get("Sid").and_then(Value::as_str) == Some(sid)) {
            return Err(conflict(format!(
                "The statement id ({sid}) provided already exists. Please provide a new statement id, or remove the existing statement."
            )));
        }
        if let Some(rev) = r.opt("RevisionId")
            && rev != function.latest.revision
        {
            return Err(precondition("The Revision Id provided does not match the latest Revision Id. Call the GetPolicy API to retrieve the latest Revision Id"));
        }
        let resource = self.arn(r, &f.name, f.qualifier.as_deref());
        let added = statement(r, &resource);
        statements.push(added.clone());
        self.find_mut(r, &f)?.policies.insert(key, document(statements).to_string());
        Ok(Out { json: json!({"Statement": added.to_string()}), status: Some(201) })
    }

    pub(crate) fn remove_permission(&mut self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let key = f.qualifier.clone().unwrap_or_default();
        let mut statements = function.policies.get(&key).map(|d| statements_of(d)).unwrap_or_default();
        let sid = r.text("StatementId");
        if statements.is_empty() {
            return Err(not_found("No policy is associated with the given resource."));
        }
        let before = statements.len();
        statements.retain(|s| s.get("Sid").and_then(Value::as_str) != Some(sid));
        if statements.len() == before {
            return Err(not_found(format!("Statement {sid} is not found in resource policy.")));
        }
        let function = self.find_mut(r, &f)?;
        if statements.is_empty() {
            function.policies.remove(&key);
        } else {
            function.policies.insert(key, document(statements).to_string());
        }
        Ok(Out { json: json!({}), status: Some(204) })
    }

    pub(crate) fn get_policy(&self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let key = f.qualifier.clone().unwrap_or_default();
        let doc = function.policies.get(&key).ok_or_else(|| not_found(NO_POLICY))?;
        Ok(json!({"Policy": doc, "RevisionId": function.latest.revision}).into())
    }

    /// The function an ARN in `ResourceArn` names.
    fn function_of_arn(&self, r: &Req) -> Result<crate::arn::FnRef, ApiError> {
        crate::arn::parse(r.text("ResourceArn"), None, r.region(), r.account())
    }

    pub(crate) fn put_resource_policy(&mut self, r: &Req) -> Result<Out, ApiError> {
        let f = self.function_of_arn(r)?;
        self.find(r, &f)?;
        let policy = r.text("Policy");
        let parsed: Value = serde_json::from_str(policy).map_err(|_| invalid("The provided policy is not valid JSON."))?;
        if !parsed.is_object() {
            return Err(invalid("The provided policy is not a JSON object."));
        }
        let (key, revision) = (f.qualifier.clone().unwrap_or_default(), uuid()?);
        let function = self.find_mut(r, &f)?;
        function.policies.insert(key, policy.to_owned());
        Ok(Out { json: json!({"Policy": policy, "RevisionId": revision}), status: Some(201) })
    }

    pub(crate) fn get_resource_policy(&self, r: &Req) -> Result<Out, ApiError> {
        let f = self.function_of_arn(r)?;
        let function = self.find(r, &f)?;
        let doc = function
            .policies
            .get(&f.qualifier.clone().unwrap_or_default())
            .ok_or_else(|| not_found(NO_POLICY))?;
        Ok(json!({"Policy": doc, "RevisionId": function.latest.revision}).into())
    }

    pub(crate) fn delete_resource_policy(&mut self, r: &Req) -> Result<Out, ApiError> {
        let f = self.function_of_arn(r)?;
        self.find(r, &f)?;
        self.find_mut(r, &f)?.policies.remove(&f.qualifier.clone().unwrap_or_default());
        Ok(Out { json: json!({}), status: Some(204) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_principal_is_shaped_by_what_it_is() {
        assert_eq!(principal("sns.amazonaws.com"), json!({"Service": "sns.amazonaws.com"}));
        assert_eq!(principal("111122223333"), json!({"AWS": "arn:aws:iam::111122223333:root"}));
        assert_eq!(principal("*"), json!("*"));
        assert_eq!(principal("arn:aws:iam::1:role/r"), json!({"AWS": "arn:aws:iam::1:role/r"}));
    }
}
