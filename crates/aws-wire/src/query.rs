//! The awsQuery protocol (IAM, STS, SNS, CloudFormation): a form-encoded `Action` in, XML out. `form` reads the request into the
//! JSON the model decodes; [`success`] and [`failure`] write the answers.

use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use http::{Response, StatusCode, header};
use serde_json::Value;

use crate::{ApiError, Fault, RequestId};

mod form;
pub(crate) mod xml;

pub use form::{parse_form, to_json};

/// The operation a Query request names: its `Action` and the version it was made for.
#[must_use]
pub fn action(form: &std::collections::BTreeMap<String, String>) -> Option<&str> {
    form.get("Action").map(String::as_str)
}

fn base(status: StatusCode, id: &RequestId) -> http::response::Builder {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/xml")
        .header("x-amzn-RequestId", id.as_str())
}

/// The 200 answer for operation `op`, whose output (if the model has one) is `output`.
#[must_use]
pub fn success(
    meta: &ServiceMeta,
    tables: &Tables,
    op: u32,
    output: &Value,
    id: &RequestId,
) -> Response<Vec<u8>> {
    let o = tables.op(op);
    let name = o.map_or("", |o| o.name);
    let shape = o.and_then(|o| o.output);
    let body = xml::response(
        tables,
        meta.xml_namespace,
        name,
        shape.map(|s| (s, output)),
        id.as_str(),
    );
    base(StatusCode::OK, id)
        .body(body.into_bytes())
        .unwrap_or_default()
}

/// The answer for `err`. A modelled error says its Query code, status and fault in the model; any other states them itself.
#[must_use]
pub fn failure(
    meta: &ServiceMeta,
    tables: &Tables,
    id: &RequestId,
    err: &ApiError,
) -> Response<Vec<u8>> {
    let modeled = err
        .shape
        .as_deref()
        .and_then(|name| crate::json::modeled(tables, name));
    let (status, fault, code) = match &modeled {
        Some(m) => (m.status, m.fault, m.query_code.as_str()),
        None => (err.status, err.fault, err.code.as_ref()),
    };
    let who = if fault == Fault::Receiver {
        "Receiver"
    } else {
        "Sender"
    };
    let body = xml::error(meta.xml_namespace, who, code, &err.message, id.as_str());
    let mut builder = base(
        StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST),
        id,
    );
    // An operation the service refuses by name says so in a header, as the JSON services do.
    if err.status == 501 && err.code == "NotImplemented" {
        builder = builder.header("x-doze-not-built", "1");
    }
    builder.body(body.into_bytes()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use aws_model_iam::meta::META;
    use aws_model_iam::tables::TABLES;
    use serde_json::json;

    use super::*;

    fn input(op: &str) -> u32 {
        TABLES
            .op(TABLES.find_op(op).unwrap())
            .unwrap()
            .input
            .unwrap()
    }

    #[test]
    fn a_refusal_by_name_carries_the_not_built_header() {
        let r = failure(
            &META,
            &TABLES,
            &RequestId::new(),
            &ApiError::new(501, "NotImplemented", "X is not built in doze-aws."),
        );
        assert_eq!(r.status(), 501);
        assert_eq!(r.headers().get("x-doze-not-built").unwrap(), "1");
        let other = failure(
            &META,
            &TABLES,
            &RequestId::new(),
            &ApiError::new(400, "Nope", "m"),
        );
        assert!(other.headers().get("x-doze-not-built").is_none());
    }

    #[test]
    fn a_form_with_a_list_of_structures_becomes_json() {
        let form = parse_form(
            "Action=CreateRole&RoleName=r&AssumeRolePolicyDocument=%7B%7D&Tags.member.2.Key=b&Tags.member.2.Value=2&Tags.member.1.Key=a&Tags.member.1.Value=1&MaxSessionDuration=7200",
        );
        let v = to_json(&TABLES, input("CreateRole"), &form);
        assert_eq!(v["RoleName"], "r");
        assert_eq!(v["MaxSessionDuration"], 7200);
        assert_eq!(
            v["Tags"],
            json!([{"Key": "a", "Value": "1"}, {"Key": "b", "Value": "2"}])
        );
        assert!(
            v.get("Action").is_none(),
            "only the operation's own members"
        );
    }

    #[test]
    fn a_list_of_strings_and_a_boolean() {
        let form = parse_form("Action=UntagRole&RoleName=r&TagKeys.member.1=a&TagKeys.member.2=b");
        assert_eq!(
            to_json(&TABLES, input("UntagRole"), &form)["TagKeys"],
            json!(["a", "b"])
        );
        let form = parse_form(
            "Action=CreatePolicyVersion&PolicyArn=x&PolicyDocument=%7B%7D&SetAsDefault=true",
        );
        assert_eq!(
            to_json(&TABLES, input("CreatePolicyVersion"), &form)["SetAsDefault"],
            true
        );
    }

    #[test]
    fn an_answer_is_xml_with_members_in_a_result_element() {
        let id = RequestId::new();
        let out = json!({"Roles": [{"RoleName": "r", "Path": "/", "RoleId": "AROAX", "Arn": "arn:x", "CreateDate": 1_700_000_000.0, "Tags": [{"Key": "k", "Value": "<v&>"}]}], "IsTruncated": false});
        let r = success(
            &META,
            &TABLES,
            TABLES.find_op("ListRoles").unwrap(),
            &out,
            &id,
        );
        let body = String::from_utf8(r.into_body()).unwrap();
        assert!(body.contains("<ListRolesResponse xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\"><ListRolesResult><Roles><member><Path>/</Path><RoleName>r</RoleName>"), "{body}");
        assert!(
            body.contains("<CreateDate>2023-11-14T22:13:20.000Z</CreateDate>"),
            "{body}"
        );
        assert!(
            body.contains(
                "<Tags><member><Key>k</Key><Value>&lt;v&amp;&gt;</Value></member></Tags>"
            ),
            "{body}"
        );
        assert!(
            body.contains(
                "<IsTruncated>false</IsTruncated></ListRolesResult><ResponseMetadata><RequestId>"
            ),
            "{body}"
        );
    }

    #[test]
    fn an_operation_with_no_output_has_no_result_element() {
        let id = RequestId::new();
        let r = success(
            &META,
            &TABLES,
            TABLES.find_op("DeleteRole").unwrap(),
            &json!({}),
            &id,
        );
        let body = String::from_utf8(r.into_body()).unwrap();
        assert!(
            body.contains("<DeleteRoleResponse xmlns=") && !body.contains("<DeleteRoleResult>"),
            "{body}"
        );
    }

    #[test]
    fn an_error_is_an_error_response_with_the_models_code_and_status() {
        let id = RequestId::new();
        let err = ApiError::modeled(
            "NoSuchEntityException",
            "The role with name r cannot be found.",
        );
        let r = failure(&META, &TABLES, &id, &err);
        assert_eq!(r.status(), 404);
        let body = String::from_utf8(r.into_body()).unwrap();
        assert!(body.contains("<Error><Type>Sender</Type><Code>NoSuchEntity</Code><Message>The role with name r cannot be found.</Message></Error>"), "{body}");
    }
}
