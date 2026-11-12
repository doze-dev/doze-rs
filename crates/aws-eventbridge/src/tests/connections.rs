//! Connections and API destinations: stored, described without their secrets, and not called.

use serde_json::{Value, json};

use super::World;

fn basic() -> Value {
    json!({"BasicAuthParameters": {"Username": "ada", "Password": "hunter2"}})
}

fn connection(w: &mut World, name: &str) -> String {
    w.ok(
        "CreateConnection",
        &json!({"Name": name, "AuthorizationType": "BASIC", "AuthParameters": basic()}),
    )["ConnectionArn"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn a_connection_is_described_without_its_secrets() {
    let mut w = World::new();
    let made = w.ok(
        "CreateConnection",
        &json!({"Name": "c", "AuthorizationType": "API_KEY", "Description": "d",
        "AuthParameters": {
            "ApiKeyAuthParameters": {"ApiKeyName": "x-key", "ApiKeyValue": "s3cret"},
            "InvocationHttpParameters": {"HeaderParameters": [
                {"Key": "plain", "Value": "visible", "IsValueSecret": false},
                {"Key": "token", "Value": "hidden", "IsValueSecret": true}]},
        }}),
    );
    assert_eq!(made["ConnectionState"], "CREATING");
    assert!(
        made["ConnectionArn"]
            .as_str()
            .unwrap()
            .starts_with("arn:aws:events:us-east-1:000000000000:connection/c/")
    );
    let d = w.ok("DescribeConnection", &json!({"Name": "c"}));
    assert_eq!(d["ConnectionState"], "AUTHORIZED");
    assert_eq!(d["AuthorizationType"], "API_KEY");
    assert!(
        d["SecretArn"]
            .as_str()
            .unwrap()
            .contains(":secret:events!connection/c/")
    );
    let shown = d["AuthParameters"].to_string();
    assert!(shown.contains("x-key") && shown.contains("visible"));
    assert!(
        !shown.contains("s3cret") && !shown.contains("hidden"),
        "{shown}"
    );
    let basic = connection(&mut w, "b");
    let d = w.ok("DescribeConnection", &json!({"Name": "b"}));
    assert_eq!(
        d["AuthParameters"]["BasicAuthParameters"],
        json!({"Username": "ada"})
    );
    assert_eq!(d["ConnectionArn"], basic);
}

#[test]
fn oauth_client_secrets_are_kept_and_not_returned() {
    let mut w = World::new();
    w.ok("CreateConnection", &json!({"Name": "o", "AuthorizationType": "OAUTH_CLIENT_CREDENTIALS", "AuthParameters": {"OAuthParameters": {
        "ClientParameters": {"ClientID": "id", "ClientSecret": "shh"}, "AuthorizationEndpoint": "https://auth.example/token", "HttpMethod": "POST"}}}));
    let d = w.ok("DescribeConnection", &json!({"Name": "o"}));
    let p = &d["AuthParameters"]["OAuthParameters"];
    assert_eq!(p["ClientParameters"], json!({"ClientID": "id"}));
    assert_eq!(p["AuthorizationEndpoint"], "https://auth.example/token");
    assert!(
        w.core
            .connections
            .get("000000000000/o")
            .unwrap()
            .auth_parameters
            .contains("shh")
    );
}

#[test]
fn a_connection_moves_through_update_deauthorize_and_delete() {
    let mut w = World::new();
    connection(&mut w, "c");
    w.ok("UpdateConnection", &json!({"Name": "c", "Description": "new", "AuthParameters": {"BasicAuthParameters": {"Username": "bob"}}}));
    // The password was not sent again, so the old one stays.
    assert!(
        w.core
            .connections
            .get("000000000000/c")
            .unwrap()
            .auth_parameters
            .contains("hunter2")
    );
    let d = w.ok("DescribeConnection", &json!({"Name": "c"}));
    assert_eq!(
        (
            d["Description"].clone(),
            d["AuthParameters"]["BasicAuthParameters"]["Username"].clone()
        ),
        (json!("new"), json!("bob"))
    );
    assert_eq!(
        w.ok("DeauthorizeConnection", &json!({"Name": "c"}))["ConnectionState"],
        "DEAUTHORIZING"
    );
    assert_eq!(
        w.ok("DescribeConnection", &json!({"Name": "c"}))["ConnectionState"],
        "DEAUTHORIZED"
    );
    assert_eq!(
        w.ok("ListConnections", &json!({"ConnectionState": "AUTHORIZED"}))["Connections"],
        json!([])
    );
    assert_eq!(
        w.ok("ListConnections", &json!({"NamePrefix": "c"}))["Connections"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        w.ok("DeleteConnection", &json!({"Name": "c"}))["ConnectionState"],
        "DELETING"
    );
    assert_eq!(
        w.err("DescribeConnection", &json!({"Name": "c"})).0,
        "ResourceNotFoundException"
    );
}

#[test]
fn the_connection_refusals() {
    let mut w = World::new();
    connection(&mut w, "c");
    assert_eq!(
        w.err(
            "CreateConnection",
            &json!({"Name": "c", "AuthorizationType": "BASIC", "AuthParameters": basic()})
        )
        .0,
        "ResourceAlreadyExistsException"
    );
    assert_eq!(
        w.err(
            "CreateConnection",
            &json!({"Name": "k", "AuthorizationType": "API_KEY", "AuthParameters": basic()})
        )
        .0,
        "ValidationException"
    );
}

#[test]
fn an_api_destination_is_active_while_its_connection_is() {
    let mut w = World::new();
    let conn = connection(&mut w, "c");
    let made = w.ok("CreateApiDestination", &json!({"Name": "d", "ConnectionArn": conn, "InvocationEndpoint": "http://localhost:9000/*/hook", "HttpMethod": "POST"}));
    assert_eq!(made["ApiDestinationState"], "ACTIVE");
    let d = w.ok("DescribeApiDestination", &json!({"Name": "d"}));
    assert_eq!(d["InvocationRateLimitPerSecond"], 300);
    assert_eq!(d["InvocationEndpoint"], "http://localhost:9000/*/hook");
    w.ok(
        "UpdateApiDestination",
        &json!({"Name": "d", "HttpMethod": "PUT", "InvocationRateLimitPerSecond": 5}),
    );
    let d = w.ok("DescribeApiDestination", &json!({"Name": "d"}));
    assert_eq!(
        (
            d["HttpMethod"].clone(),
            d["InvocationRateLimitPerSecond"].clone()
        ),
        (json!("PUT"), json!(5))
    );
    assert_eq!(
        w.ok("ListApiDestinations", &json!({"ConnectionArn": conn}))["ApiDestinations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    w.ok("DeleteConnection", &json!({"Name": "c"}));
    assert_eq!(
        w.ok("DescribeApiDestination", &json!({"Name": "d"}))["ApiDestinationState"],
        "INACTIVE"
    );
    w.ok("DeleteApiDestination", &json!({"Name": "d"}));
    assert_eq!(
        w.err("DescribeApiDestination", &json!({"Name": "d"})).0,
        "ResourceNotFoundException"
    );
    assert_eq!(w.err("CreateApiDestination", &json!({"Name": "d", "ConnectionArn": conn, "InvocationEndpoint": "http://x", "HttpMethod": "GET"})).0, "ResourceNotFoundException");
}
