//! Mobile push and SMS: stored, nothing sent.

use serde_json::json;

use super::World;

#[test]
fn an_application_and_its_endpoints_are_stored_listed_and_deleted_together() {
    let mut w = World::new();
    assert!(
        w.refused(
            "CreatePlatformApplication",
            &json!({"Name": "a", "Platform": "GCM", "Attributes": {}})
        )
        .contains("PlatformCredential")
    );
    assert!(
        w.refused(
            "CreatePlatformApplication",
            &json!({"Name": "a", "Platform": "PIGEON", "Attributes": {"PlatformCredential": "k"}})
        )
        .contains("Unsupported platform")
    );
    let made = w.ok(
        "CreatePlatformApplication",
        &json!({"Name": "app", "Platform": "GCM", "Attributes": {"PlatformCredential": "secret"}}),
    );
    let app = made["PlatformApplicationArn"].as_str().unwrap().to_owned();
    assert_eq!(app, "arn:aws:sns:us-east-1:000000000000:app/GCM/app");
    // The credential is never answered; the application is enabled.
    let attrs = w.ok(
        "GetPlatformApplicationAttributes",
        &json!({"PlatformApplicationArn": app}),
    );
    assert_eq!(attrs["Attributes"], json!({"Enabled": "true"}));
    w.ok("SetPlatformApplicationAttributes", &json!({"PlatformApplicationArn": app, "Attributes": {"SuccessFeedbackRoleArn": "arn:aws:iam::000000000000:role/r"}}));
    assert_eq!(
        w.ok("ListPlatformApplications", &json!({}))["PlatformApplications"][0]["Attributes"]["SuccessFeedbackRoleArn"],
        "arn:aws:iam::000000000000:role/r"
    );
    // The same name again is the same application.
    assert_eq!(w.ok("CreatePlatformApplication", &json!({"Name": "app", "Platform": "GCM", "Attributes": {"PlatformCredential": "secret"}})), made);

    endpoints_of(&mut w, &app);
}

fn endpoints_of(w: &mut World, app: &str) {
    let e = w.ok(
        "CreatePlatformEndpoint",
        &json!({"PlatformApplicationArn": app, "Token": "tok1", "CustomUserData": "me"}),
    );
    let endpoint = e["EndpointArn"].as_str().unwrap().to_owned();
    assert!(endpoint.starts_with("arn:aws:sns:us-east-1:000000000000:endpoint/GCM/app/"));
    let got = w.ok("GetEndpointAttributes", &json!({"EndpointArn": endpoint}));
    assert_eq!(
        got["Attributes"],
        json!({"CustomUserData": "me", "Enabled": "true", "Token": "tok1"})
    );
    // The same token again: the same endpoint if the attributes agree, a refusal if not.
    let again = w.ok(
        "CreatePlatformEndpoint",
        &json!({"PlatformApplicationArn": app, "Token": "tok1", "CustomUserData": "me"}),
    );
    assert_eq!(again["EndpointArn"], endpoint.as_str());
    let clash = w.refused(
        "CreatePlatformEndpoint",
        &json!({"PlatformApplicationArn": app, "Token": "tok1", "CustomUserData": "other"}),
    );
    assert!(
        clash.contains("already exists with the same Token, but different attributes"),
        "{clash}"
    );
    changing_and_deleting(w, app, &endpoint);
}

fn changing_and_deleting(w: &mut World, app: &str, endpoint: &str) {
    w.ok(
        "SetEndpointAttributes",
        &json!({"EndpointArn": endpoint, "Attributes": {"Enabled": "false", "Token": "tok2"}}),
    );
    let got = w.ok("GetEndpointAttributes", &json!({"EndpointArn": endpoint}));
    assert_eq!(
        (&got["Attributes"]["Enabled"], &got["Attributes"]["Token"]),
        (&json!("false"), &json!("tok2"))
    );
    assert!(
        w.run(
            "SetEndpointAttributes",
            &json!({"EndpointArn": endpoint, "Attributes": {"Enabled": "maybe"}})
        )
        .is_err()
    );
    assert_eq!(
        w.ok(
            "ListEndpointsByPlatformApplication",
            &json!({"PlatformApplicationArn": app})
        )["Endpoints"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    w.ok(
        "DeletePlatformApplication",
        &json!({"PlatformApplicationArn": app}),
    );
    w.ok(
        "DeletePlatformApplication",
        &json!({"PlatformApplicationArn": app}),
    );
    assert_eq!(
        w.err("GetEndpointAttributes", &json!({"EndpointArn": endpoint}))
            .0,
        "NotFoundException"
    );
    assert_eq!(
        w.err(
            "ListEndpointsByPlatformApplication",
            &json!({"PlatformApplicationArn": app})
        )
        .0,
        "NotFoundException"
    );
    w.ok("DeleteEndpoint", &json!({"EndpointArn": endpoint}));
}

#[test]
fn an_application_endpoint_can_be_subscribed_and_a_missing_one_cannot() {
    let mut w = World::new();
    let topic = w.topic("t");
    let app = w.ok(
        "CreatePlatformApplication",
        &json!({"Name": "app", "Platform": "APNS", "Attributes": {"PlatformCredential": "k"}}),
    );
    let endpoint = w.ok(
        "CreatePlatformEndpoint",
        &json!({"PlatformApplicationArn": app["PlatformApplicationArn"], "Token": "t"}),
    );
    let sub = w.ok(
        "Subscribe",
        &json!({"TopicArn": topic, "Protocol": "application", "Endpoint": endpoint["EndpointArn"]}),
    );
    assert_ne!(sub["SubscriptionArn"], "pending confirmation");
    assert!(w.run("Subscribe", &json!({"TopicArn": topic, "Protocol": "application", "Endpoint": "arn:aws:sns:us-east-1:000000000000:endpoint/APNS/app/none"})).is_err());
}

#[test]
fn sms_settings_are_kept_and_no_number_is_opted_out() {
    let mut w = World::new();
    assert_eq!(
        w.ok("GetSMSAttributes", &json!({}))["attributes"],
        json!({})
    );
    w.ok(
        "SetSMSAttributes",
        &json!({"attributes": {"DefaultSMSType": "Transactional", "MonthlySpendLimit": "5"}}),
    );
    assert_eq!(
        w.ok(
            "GetSMSAttributes",
            &json!({"attributes": ["DefaultSMSType"]})
        )["attributes"],
        json!({"DefaultSMSType": "Transactional"})
    );
    assert_eq!(
        w.ok("GetSMSAttributes", &json!({}))["attributes"],
        json!({"DefaultSMSType": "Transactional", "MonthlySpendLimit": "5"})
    );
    assert!(
        w.run(
            "SetSMSAttributes",
            &json!({"attributes": {"DefaultSMSType": "Both"}})
        )
        .is_err()
    );
    assert_eq!(
        w.ok(
            "CheckIfPhoneNumberIsOptedOut",
            &json!({"phoneNumber": "+14155552671"})
        )["isOptedOut"],
        false
    );
    assert_eq!(
        w.ok("ListPhoneNumbersOptedOut", &json!({}))["phoneNumbers"],
        json!([])
    );
    w.ok("OptInPhoneNumber", &json!({"phoneNumber": "+14155552671"}));
    assert!(
        w.run("OptInPhoneNumber", &json!({"phoneNumber": "nope"}))
            .is_err()
    );
}
