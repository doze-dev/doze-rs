//! The evaluation rules, case by case.
#![allow(clippy::unwrap_used)]

use super::*;
use crate::parse;

const USER: &str = "arn:aws:iam::111122223333:user/alice";
const QUEUE: &str = "arn:aws:sqs:us-east-1:111122223333:jobs";

fn doc(text: &str) -> Document {
    parse(text).unwrap().unwrap()
}

fn allow(action: &str, resource: &str) -> Document {
    doc(&format!(
        r#"{{"Statement":[{{"Effect":"Allow","Action":"{action}","Resource":"{resource}"}}]}}"#
    ))
}

fn subject() -> Subject {
    Subject {
        arns: vec![USER.into()],
        account: "111122223333".into(),
        service: None,
    }
}

fn ask(p: &Policies<'_>, action: &str, resource: &str, ctx: &Context) -> Decision {
    evaluate(
        p,
        &Request {
            action,
            resource,
            context: ctx,
            subject: &subject(),
        },
    )
}

fn identity<'a>(docs: &'a [&'a Document]) -> Policies<'a> {
    Policies {
        identity: docs,
        ..Policies::default()
    }
}

#[test]
fn nothing_allows_so_it_is_an_implicit_deny() {
    let d = ask(&identity(&[]), "sqs:SendMessage", QUEUE, &Context::new());
    assert_eq!(d.verdict, Verdict::ImplicitDeny);
    assert!(d.matched.is_empty());
}

#[test]
fn actions_ignore_case_and_resources_do_not() {
    let a = allow("SQS:send*", "arn:aws:sqs:*:*:jobs");
    let docs = [&a];
    let p = identity(&docs);
    let c = Context::new();
    assert!(ask(&p, "sqs:SendMessage", QUEUE, &c).allowed());
    assert!(!ask(&p, "sqs:ReceiveMessage", QUEUE, &c).allowed());
    assert!(
        !ask(
            &p,
            "sqs:SendMessage",
            "arn:aws:sqs:us-east-1:111122223333:JOBS",
            &c
        )
        .allowed()
    );
}

#[test]
fn a_deny_beats_an_allow_in_any_layer() {
    let a = allow("sqs:*", "*");
    let deny = doc(r#"{"Statement":{"Effect":"Deny","Action":"sqs:Delete*","Resource":"*"}}"#);
    let c = Context::new();
    let p = Policies {
        identity: &[&a],
        resource: Some(&[&deny]),
        ..Policies::default()
    };
    let d = ask(&p, "sqs:DeleteQueue", QUEUE, &c);
    assert_eq!(d.verdict, Verdict::ExplicitDeny);
    assert_eq!(d.denied_by(), Some(Layer::Resource));
    assert!(ask(&p, "sqs:SendMessage", QUEUE, &c).allowed());
}

#[test]
fn a_boundary_and_a_session_policy_each_limit_the_identity_policy() {
    let a = allow("sqs:*", "*");
    let only_send = allow("sqs:SendMessage", "*");
    let only_recv = allow("sqs:ReceiveMessage", "*");
    let c = Context::new();
    let p = Policies {
        identity: &[&a],
        boundary: Some(&[&only_send]),
        ..Policies::default()
    };
    assert!(ask(&p, "sqs:SendMessage", QUEUE, &c).allowed());
    assert!(!ask(&p, "sqs:ReceiveMessage", QUEUE, &c).allowed());
    let p = Policies {
        identity: &[&a],
        boundary: Some(&[&only_send]),
        session: Some(&[&only_recv]),
        ..Policies::default()
    };
    assert!(!ask(&p, "sqs:SendMessage", QUEUE, &c).allowed());
    assert!(!ask(&p, "sqs:ReceiveMessage", QUEUE, &c).allowed());
}

#[test]
fn a_resource_policy_naming_the_caller_allows_past_the_boundary() {
    let boundary = allow("s3:*", "*");
    let direct = doc(&format!(
        r#"{{"Statement":{{"Effect":"Allow","Principal":{{"AWS":"{USER}"}},"Action":"sqs:SendMessage","Resource":"*"}}}}"#
    ));
    let c = Context::new();
    let p = Policies {
        identity: &[],
        boundary: Some(&[&boundary]),
        resource: Some(&[&direct]),
        ..Policies::default()
    };
    assert!(ask(&p, "sqs:SendMessage", QUEUE, &c).allowed());
    // Naming the account root only hands the decision to the identity policies.
    let root = doc(
        r#"{"Statement":{"Effect":"Allow","Principal":{"AWS":"111122223333"},"Action":"sqs:SendMessage","Resource":"*"}}"#,
    );
    let p = Policies {
        identity: &[],
        resource: Some(&[&root]),
        ..Policies::default()
    };
    assert!(!ask(&p, "sqs:SendMessage", QUEUE, &c).allowed());
    let a = allow("sqs:SendMessage", "*");
    let p = Policies {
        identity: &[&a],
        resource: Some(&[&root]),
        ..Policies::default()
    };
    assert!(ask(&p, "sqs:SendMessage", QUEUE, &c).allowed());
}

#[test]
fn another_principal_in_a_resource_policy_does_not_match() {
    let other = doc(
        r#"{"Statement":{"Effect":"Deny","Principal":{"AWS":"arn:aws:iam::111122223333:user/bob"},"Action":"*","Resource":"*"}}"#,
    );
    let a = allow("*", "*");
    let p = Policies {
        identity: &[&a],
        resource: Some(&[&other]),
        ..Policies::default()
    };
    assert!(ask(&p, "sqs:SendMessage", QUEUE, &Context::new()).allowed());
    let not = doc(
        r#"{"Statement":{"Effect":"Deny","NotPrincipal":{"AWS":"arn:aws:iam::111122223333:user/bob"},"Action":"*","Resource":"*"}}"#,
    );
    let p = Policies {
        identity: &[&a],
        resource: Some(&[&not]),
        ..Policies::default()
    };
    assert_eq!(
        ask(&p, "sqs:SendMessage", QUEUE, &Context::new()).verdict,
        Verdict::ExplicitDeny
    );
}

#[test]
fn not_action_and_not_resource_invert() {
    let d = doc(
        r#"{"Statement":{"Effect":"Allow","NotAction":"iam:*","NotResource":"arn:aws:sqs:*:*:secret"}}"#,
    );
    let docs = [&d];
    let p = identity(&docs);
    let c = Context::new();
    assert!(ask(&p, "sqs:SendMessage", QUEUE, &c).allowed());
    assert!(!ask(&p, "iam:CreateUser", "*", &c).allowed());
    assert!(
        !ask(
            &p,
            "sqs:SendMessage",
            "arn:aws:sqs:us-east-1:111122223333:secret",
            &c
        )
        .allowed()
    );
}

fn conditioned(operator: &str, key: &str, values: &str) -> Document {
    doc(&format!(
        r#"{{"Statement":{{"Effect":"Allow","Action":"*","Resource":"*","Condition":{{"{operator}":{{"{key}":{values}}}}}}}}}"#
    ))
}

fn holds(operator: &str, key: &str, values: &str, ctx: &Context) -> bool {
    let d = conditioned(operator, key, values);
    ask(&identity(&[&d]), "sqs:SendMessage", QUEUE, ctx).allowed()
}

#[test]
fn string_conditions() {
    let mut c = Context::new();
    c.set("aws:username", "alice");
    assert!(holds("StringEquals", "aws:username", r#""alice""#, &c));
    assert!(!holds("StringEquals", "aws:username", r#""Alice""#, &c));
    assert!(holds(
        "StringEqualsIgnoreCase",
        "aws:username",
        r#""ALICE""#,
        &c
    ));
    assert!(holds("StringLike", "aws:username", r#"["bob","al*"]"#, &c));
    assert!(holds("StringNotEquals", "aws:username", r#""bob""#, &c));
    assert!(!holds("StringNotLike", "aws:username", r#""al?ce""#, &c));
    // A missing key fails an ordinary operator and passes a negated one.
    assert!(!holds("StringEquals", "aws:userid", r#""x""#, &c));
    assert!(holds("StringNotEquals", "aws:userid", r#""x""#, &c));
    assert!(holds("StringEqualsIfExists", "aws:userid", r#""x""#, &c));
}

#[test]
fn numeric_date_bool_ip_arn_and_null_conditions() {
    let mut c = Context::new();
    c.set("sqs:count", "5")
        .set("aws:currenttime", "2026-11-12T10:00:00Z")
        .set("aws:securetransport", "true")
        .set("aws:sourceip", "10.1.2.3")
        .set("aws:sourcearn", QUEUE);
    assert!(holds("NumericLessThan", "sqs:count", r#""10""#, &c));
    assert!(!holds(
        "NumericGreaterThanEquals",
        "sqs:count",
        r#""6""#,
        &c
    ));
    assert!(holds(
        "DateGreaterThan",
        "aws:CurrentTime",
        r#""2026-01-01T00:00:00Z""#,
        &c
    ));
    assert!(!holds(
        "DateLessThan",
        "aws:CurrentTime",
        r#""2026-01-01T00:00:00Z""#,
        &c
    ));
    assert!(holds("Bool", "aws:SecureTransport", r#""true""#, &c));
    assert!(holds(
        "IpAddress",
        "aws:SourceIp",
        r#"["192.0.2.0/24","10.0.0.0/8"]"#,
        &c
    ));
    assert!(holds(
        "NotIpAddress",
        "aws:SourceIp",
        r#""192.0.2.0/24""#,
        &c
    ));
    assert!(holds(
        "ArnLike",
        "aws:SourceArn",
        r#""arn:aws:sqs:*:111122223333:*""#,
        &c
    ));
    assert!(!holds(
        "ArnEquals",
        "aws:SourceArn",
        r#""arn:aws:sqs:*:999999999999:*""#,
        &c
    ));
    assert!(holds("Null", "aws:SourceVpce", r#""true""#, &c));
    assert!(holds("Null", "aws:SourceIp", r#""false""#, &c));
    assert!(!holds("Null", "aws:SourceIp", r#""true""#, &c));
}

#[test]
fn set_operators() {
    let mut c = Context::new();
    c.set_many("aws:tagkeys", vec!["env".into(), "team".into()]);
    assert!(holds(
        "ForAllValues:StringEquals",
        "aws:TagKeys",
        r#"["env","team","owner"]"#,
        &c
    ));
    assert!(!holds(
        "ForAllValues:StringEquals",
        "aws:TagKeys",
        r#"["env"]"#,
        &c
    ));
    assert!(holds(
        "ForAnyValue:StringEquals",
        "aws:TagKeys",
        r#"["team"]"#,
        &c
    ));
    assert!(!holds(
        "ForAnyValue:StringEquals",
        "aws:TagKeys",
        r#"["owner"]"#,
        &c
    ));
    // With nothing in the request, ForAllValues is true and ForAnyValue is false.
    let none = Context::new();
    assert!(holds(
        "ForAllValues:StringEquals",
        "aws:TagKeys",
        r#""env""#,
        &none
    ));
    assert!(!holds(
        "ForAnyValue:StringEquals",
        "aws:TagKeys",
        r#""env""#,
        &none
    ));
}

#[test]
fn policy_variables_in_resources_and_conditions() {
    let d = doc(
        r#"{"Statement":{"Effect":"Allow","Action":"s3:*","Resource":"arn:aws:s3:::home/${aws:username}/*"}}"#,
    );
    let mut c = Context::new();
    c.set("aws:username", "alice");
    let docs = [&d];
    let p = identity(&docs);
    assert!(ask(&p, "s3:GetObject", "arn:aws:s3:::home/alice/a.txt", &c).allowed());
    assert!(!ask(&p, "s3:GetObject", "arn:aws:s3:::home/bob/a.txt", &c).allowed());
    assert!(
        !ask(
            &p,
            "s3:GetObject",
            "arn:aws:s3:::home/alice/a.txt",
            &Context::new()
        )
        .allowed()
    );
    let own = conditioned(
        "StringEquals",
        "aws:username",
        r#""${aws:principaltag/name}""#,
    );
    c.set("aws:principaltag/name", "alice");
    assert!(ask(&identity(&[&own]), "x:y", "*", &c).allowed());
}

#[test]
fn a_key_aws_has_and_we_lack_is_reported() {
    let d = conditioned("StringEquals", "aws:PrincipalOrgID", r#""o-abc""#);
    let mut c = Context::new();
    c.leave_unsupplied("aws:PrincipalOrgID");
    let out = ask(&identity(&[&d]), "x:y", "*", &c);
    assert_eq!(out.verdict, Verdict::ImplicitDeny);
    assert_eq!(out.unsupplied, vec!["aws:PrincipalOrgID".to_owned()]);
}

#[test]
fn the_decision_names_the_statements_that_applied() {
    let d = doc(
        r#"{"Statement":[{"Sid":"no","Effect":"Allow","Action":"s3:*","Resource":"*"},{"Sid":"yes","Effect":"Allow","Action":"sqs:*","Resource":"*"}]}"#,
    );
    let out = ask(&identity(&[&d]), "sqs:SendMessage", QUEUE, &Context::new());
    assert_eq!(out.matched.len(), 1);
    assert_eq!(
        (out.matched[0].statement, out.matched[0].sid.as_deref()),
        (1, Some("yes"))
    );
}

#[test]
fn a_gated_resource_needs_its_own_policy_to_let_identity_policies_count() {
    let a = allow("kms:Decrypt", "*");
    let root = doc(
        r#"{"Statement":{"Effect":"Allow","Principal":{"AWS":"arn:aws:iam::111122223333:root"},"Action":"kms:*","Resource":"*"}}"#,
    );
    let c = Context::new();
    let identity_docs = [&a];
    let decide = |resource: Option<&[&Document]>| {
        let p = Policies {
            identity: &identity_docs,
            resource,
            resource_gate: true,
            ..Policies::default()
        };
        ask(&p, "kms:Decrypt", "*", &c).allowed()
    };
    assert!(!decide(Some(&[])));
    assert!(!decide(None));
    assert!(decide(Some(&[&root])));
}
