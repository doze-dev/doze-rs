//! The credential report, and service last accessed data from what the authorizer was asked.

use aws_peers::{Mode, Need};
use aws_wire::service::Caller;
use serde_json::{Value, json};

use super::{READ_S3, START, TRUST, World};

/// The report's CSV, after a `GenerateCredentialReport` that answers `state`.
fn report(w: &mut World, state: &str) -> String {
    assert_eq!(w.ok("GenerateCredentialReport", &json!({}))["State"], state);
    let got = w.ok("GetCredentialReport", &json!({}));
    assert_eq!(got["ReportFormat"], "text/csv");
    String::from_utf8(aws_core::base64::decode(got["Content"].as_str().unwrap()).unwrap()).unwrap()
}

const MADE: &str = "2023-11-14T22:13:20+00:00";

#[test]
fn the_credential_report_has_the_guides_columns() {
    let mut w = World::new();
    assert_eq!(
        w.err("GetCredentialReport", &json!({})).0,
        "CredentialReportNotPresentException"
    );
    w.ok("CreateUser", &json!({"UserName": "u"}));
    w.ok(
        "CreateLoginProfile",
        &json!({"UserName": "u", "Password": "pw"}),
    );
    w.ok("CreateAccessKey", &json!({"UserName": "u"}));
    let csv = report(&mut w, "STARTED");
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].starts_with("user,arn,user_creation_time,password_enabled,"));
    assert_eq!(lines[0].split(',').count(), 22);
    let root: Vec<&str> = lines[1].split(',').collect();
    assert_eq!(
        &root[..4],
        [
            "<root_account>",
            "arn:aws:iam::000000000000:root",
            MADE,
            "not_supported"
        ]
    );
    let u: Vec<&str> = lines[2].split(',').collect();
    let start = [
        "u",
        "arn:aws:iam::000000000000:user/u",
        MADE,
        "true",
        "no_information",
        MADE,
        "N/A",
        "false",
    ];
    assert_eq!(&u[..8], start);
    assert_eq!(&u[8..10], ["true", MADE]);
    let rest = [
        "false", "N/A", "N/A", "N/A", "N/A", "false", "N/A", "false", "N/A",
    ];
    assert_eq!(&u[13..], rest);
}

#[test]
fn the_credential_report_is_made_once_in_four_hours_and_then_expires() {
    let mut w = World::new();
    w.ok("CreateUser", &json!({"UserName": "u"}));
    assert_eq!(report(&mut w, "STARTED").lines().count(), 3);
    w.ok("CreateUser", &json!({"UserName": "later"}));
    assert_eq!(
        report(&mut w, "COMPLETE").lines().count(),
        3,
        "`later` came after the report"
    );
    w.clock.set(START + 4 * 3600 * 1000);
    assert_eq!(
        w.err("GetCredentialReport", &json!({})).0,
        "CredentialReportExpiredException"
    );
    assert_eq!(report(&mut w, "STARTED").lines().count(), 4);
}

fn caller(key: &str, token: Option<&str>) -> Caller {
    Caller {
        access_key_id: key.into(),
        account_id: "000000000000".into(),
        region: "eu-west-1".into(),
        endpoint: "http://localhost:4566".into(),
        service: None,
        session_token: token.map(str::to_owned),
    }
}

const SQS_AND_S3: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":["sqs:SendMessage","s3:Get*"],"Resource":"*"}]}"#;
const USER: &str = "arn:aws:iam::000000000000:user/u";

/// A user `u` in group `g` (whose inline policy allows SQS and S3) with managed policy `m` (S3), that asked for two SQS actions two
/// minutes apart; the policy's ARN.
fn asked() -> (World, Value) {
    let mut w = World::new();
    w.ok("CreateUser", &json!({"UserName": "u"}));
    w.ok("CreateGroup", &json!({"GroupName": "g"}));
    w.ok(
        "AddUserToGroup",
        &json!({"GroupName": "g", "UserName": "u"}),
    );
    let p = json!({"GroupName": "g", "PolicyName": "p", "PolicyDocument": SQS_AND_S3});
    w.ok("PutGroupPolicy", &p);
    let m = json!({"PolicyName": "m", "PolicyDocument": READ_S3});
    let policy = w.ok("CreatePolicy", &m)["Policy"]["Arn"].clone();
    w.ok(
        "AttachUserPolicy",
        &json!({"UserName": "u", "PolicyArn": policy}),
    );
    let key =
        w.ok("CreateAccessKey", &json!({"UserName": "u"}))["AccessKey"]["AccessKeyId"].clone();
    w.core.mode = Mode::Soft;
    let c = caller(key.as_str().unwrap(), None);
    w.core
        .authorize(&c, &[Need::new("sqs:SendMessage", "*")])
        .unwrap();
    w.clock.advance(120_000);
    // Refused or not, an attempt counts.
    w.core
        .authorize(&c, &[Need::new("sqs:DeleteQueue", "*")])
        .unwrap();
    (w, policy)
}

/// A job's report for `arn`.
fn job(w: &mut World, request: &Value) -> (Value, Value) {
    let id = w.ok("GenerateServiceLastAccessedDetails", request)["JobId"].clone();
    assert_eq!(id.as_str().unwrap().len(), 36);
    let got = w.ok("GetServiceLastAccessedDetails", &json!({"JobId": id}));
    assert_eq!(got["JobStatus"], "COMPLETED");
    (id, got)
}

#[test]
fn a_users_report_lists_what_its_policies_allow_and_when_each_was_asked_for() {
    let (mut w, _) = asked();
    let (_, got) = job(&mut w, &json!({"Arn": USER, "Granularity": "ACTION_LEVEL"}));
    assert_eq!(got["JobType"], "ACTION_LEVEL");
    let services = got["ServicesLastAccessed"].as_array().unwrap();
    let names: Vec<&str> = services
        .iter()
        .map(|s| s["ServiceNamespace"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["s3", "sqs"], "the user's and its group's policies");
    let sqs = &services[1];
    assert_eq!(sqs["ServiceName"], "Amazon SQS");
    assert_eq!(sqs["LastAuthenticatedEntity"], USER);
    assert_eq!(sqs["LastAuthenticatedRegion"], "eu-west-1");
    assert_eq!(sqs["LastAuthenticated"].as_f64(), Some(1_700_000_120.0));
    assert_eq!(sqs["TotalAuthenticatedEntities"], 1);
    let actions: Vec<&str> = sqs["TrackedActionsLastAccessed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["ActionName"].as_str().unwrap())
        .collect();
    assert_eq!(actions, ["DeleteQueue", "SendMessage"]);
    assert!(
        services[0].get("LastAuthenticated").is_none(),
        "s3 was never asked for"
    );
    assert_eq!(services[0]["TotalAuthenticatedEntities"], 0);
    let s3 = services[0]["TrackedActionsLastAccessed"]
        .as_array()
        .unwrap();
    assert_eq!(s3.len(), 42, "S3's tracked actions, by name alone");
    assert_eq!(s3[0], json!({"ActionName": "GetAccelerateConfiguration"}));
    let absent = json!({"JobId": "00000000-0000-4000-8000-000000000000"});
    assert_eq!(
        w.err("GetServiceLastAccessedDetails", &absent).0,
        "NoSuchEntityException"
    );
}

#[test]
fn a_groups_report_names_the_member_that_asked_and_a_policys_lists_its_services() {
    let (mut w, policy) = asked();
    let (id, got) = job(&mut w, &json!({"Arn": "arn:aws:iam::000000000000:group/g"}));
    assert_eq!(got["JobType"], "SERVICE_LEVEL");
    assert!(
        got["ServicesLastAccessed"][1]
            .get("TrackedActionsLastAccessed")
            .is_none()
    );
    let request = json!({"JobId": id, "ServiceNamespace": "sqs"});
    let entities = w.ok("GetServiceLastAccessedDetailsWithEntities", &request);
    let first = &entities["EntityDetailsList"][0];
    assert_eq!(
        (
            first["EntityInfo"]["Arn"].as_str(),
            first["EntityInfo"]["Type"].as_str()
        ),
        (Some(USER), Some("USER"))
    );
    assert_eq!(first["LastAuthenticated"].as_f64(), Some(1_700_000_120.0));
    let (_, got) = job(&mut w, &json!({"Arn": policy}));
    assert_eq!(
        got["ServicesLastAccessed"].as_array().unwrap().len(),
        1,
        "s3 only"
    );
}

#[test]
fn the_policies_granting_a_service_are_the_users_and_its_groups() {
    let (mut w, policy) = asked();
    let request = json!({"Arn": USER, "ServiceNamespaces": ["s3", "sqs", "kms"]});
    let granting =
        w.ok("ListPoliciesGrantingServiceAccess", &request)["PoliciesGrantingServiceAccess"]
            .clone();
    assert_eq!(
        granting[0]["Policies"],
        json!([
            {"PolicyName": "m", "PolicyType": "MANAGED", "PolicyArn": policy},
            {"PolicyName": "p", "PolicyType": "INLINE", "EntityType": "GROUP", "EntityName": "g"},
        ])
    );
    assert_eq!(granting[1]["Policies"].as_array().unwrap().len(), 1);
    assert_eq!(granting[2]["Policies"], json!([]));
}

#[test]
fn a_roles_sessions_count_for_the_role_and_a_deleted_role_takes_its_data() {
    let mut w = World::new();
    w.ok(
        "CreateRole",
        &json!({"RoleName": "r", "AssumeRolePolicyDocument": TRUST}),
    );
    let p = json!({"RoleName": "r", "PolicyName": "p", "PolicyDocument": SQS_AND_S3});
    w.ok("PutRolePolicy", &p);
    let role = "arn:aws:iam::000000000000:role/r";
    let s = w.sts_ok(
        "AssumeRole",
        &json!({"RoleArn": role, "RoleSessionName": "ss"}),
    )["Credentials"]
        .clone();
    let c = caller(
        s["AccessKeyId"].as_str().unwrap(),
        s["SessionToken"].as_str(),
    );
    w.core
        .authorize(&c, &[Need::new("s3:GetObject", "*")])
        .unwrap();
    let (_, got) = job(&mut w, &json!({"Arn": role}));
    assert_eq!(
        got["ServicesLastAccessed"][0]["LastAuthenticatedEntity"],
        role
    );
    w.ok(
        "DeleteRolePolicy",
        &json!({"RoleName": "r", "PolicyName": "p"}),
    );
    w.ok("DeleteRole", &json!({"RoleName": "r"}));
    assert_eq!(w.core.held.access.len(), 0);
}
