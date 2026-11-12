//! What is refused, and in whose words. Where AWS's message was recorded it is asserted whole; the rest assert the error's name.

use serde_json::json;

use super::World;

#[test]
fn what_aws_refused_it_refuses_in_its_words() {
    let mut w = World::new();
    let (code, message) = w.err(
        "PutParameter",
        &json!({"Name": "/a", "Value": "v", "Type": "Sentence"}),
    );
    assert_eq!(code, "ValidationException");
    assert_eq!(
        message,
        "1 validation error detected: Value 'Sentence' at 'type' failed to satisfy constraint: Member must satisfy enum value set: [SecureString, StringList, String]"
    );
    let (code, message) = w.err("PutParameter", &json!({"Name": "/a", "Value": "v"}));
    assert_eq!(
        (code.as_str(), message.as_str()),
        (
            "ValidationException",
            "A parameter type is required when you create a parameter."
        )
    );
    w.put("/a", "v");
    let (code, message) = w.err(
        "PutParameter",
        &json!({"Name": "/a", "Value": "w", "Type": "String"}),
    );
    assert_eq!(code, "ParameterAlreadyExists");
    assert_eq!(
        message,
        "The parameter already exists. To overwrite this value, set the overwrite option in the request to true."
    );
    let (code, message) = w.err("GetParameter", &json!({"Name": "/a:9"}));
    assert_eq!(code, "ParameterVersionNotFound");
    assert_eq!(
        message,
        "Systems Manager could not find version 9 of /a. Verify the version and try again."
    );
    assert_eq!(
        w.err(
            "LabelParameterVersion",
            &json!({"Name": "/a", "ParameterVersion": 9, "Labels": ["l"]})
        )
        .0,
        "ParameterVersionNotFound"
    );
    let (code, message) = w.err("DeleteParameter", &json!({"Name": "/absent"}));
    assert_eq!((code.as_str(), message.as_str()), ("ParameterNotFound", ""));
}

#[test]
fn a_name_with_a_character_it_may_not_have_is_refused() {
    let mut w = World::new();
    let (code, message) = w.err(
        "PutParameter",
        &json!({"Name": "/x/has space", "Value": "v", "Type": "String"}),
    );
    assert_eq!(code, "ValidationException");
    assert!(
        message.starts_with("Parameter name: can't be prefixed with \"ssm\" (case-insensitive)."),
        "{message}"
    );
    for reserved in ["/aws/not-yours", "ssm-not-yours"] {
        assert_eq!(
            w.err(
                "PutParameter",
                &json!({"Name": reserved, "Value": "v", "Type": "String"})
            )
            .0,
            "ValidationException"
        );
    }
    // Recorded: deeper in the path the word is only a word.
    w.ok(
        "PutParameter",
        &json!({"Name": "/x/aws-inside", "Value": "v", "Type": "String"}),
    );
    // A type change on overwrite is allowed (recorded: String to StringList was accepted).
    w.ok(
        "PutParameter",
        &json!({"Name": "/x/aws-inside", "Value": "a,b", "Type": "StringList", "Overwrite": true}),
    );
    assert_eq!(
        w.err("GetParametersByPath", &json!({"Path": "no-leading-slash"}))
            .0,
        "ValidationException"
    );
    let deep = format!("/{}", ["a"; 16].join("/"));
    assert_eq!(
        w.err(
            "PutParameter",
            &json!({"Name": deep, "Value": "v", "Type": "String"})
        )
        .0,
        "HierarchyLevelLimitExceededException"
    );
}

#[test]
fn values_are_held_to_their_tier_and_pattern() {
    let mut w = World::new();
    let big = "x".repeat(4097);
    assert_eq!(
        w.err(
            "PutParameter",
            &json!({"Name": "/big", "Value": big, "Type": "String"})
        )
        .0,
        "ValidationException"
    );
    let ok = w.ok(
        "PutParameter",
        &json!({"Name": "/big", "Value": big, "Type": "String", "Tier": "Advanced"}),
    );
    assert_eq!(ok["Tier"], "Advanced");
    let auto = w.ok(
        "PutParameter",
        &json!({"Name": "/auto", "Value": big, "Type": "String", "Tier": "Intelligent-Tiering"}),
    );
    assert_eq!(auto["Tier"], "Advanced");
    assert_eq!(w.err("PutParameter", &json!({"Name": "/big", "Value": "s", "Type": "String", "Tier": "Standard", "Overwrite": true})).0, "ValidationException", "an advanced parameter cannot go back");

    w.ok(
        "PutParameter",
        &json!({"Name": "/n", "Value": "42", "Type": "String", "AllowedPattern": "^\\d+$"}),
    );
    assert_eq!(
        w.err(
            "PutParameter",
            &json!({"Name": "/n", "Value": "forty", "Type": "String", "Overwrite": true})
        )
        .0,
        "ParameterPatternMismatchException"
    );
    assert_eq!(
        w.err(
            "PutParameter",
            &json!({"Name": "/bad", "Value": "v", "Type": "String", "AllowedPattern": "("})
        )
        .0,
        "InvalidAllowedPatternException"
    );
}

#[test]
fn a_parameter_keeps_a_hundred_versions_and_a_label_protects_one() {
    let mut w = World::new();
    for n in 1..=100 {
        w.put("/h", &n.to_string());
    }
    w.ok(
        "LabelParameterVersion",
        &json!({"Name": "/h", "ParameterVersion": 1, "Labels": ["keep"]}),
    );
    // The 101st drops the oldest version without a label: version 2.
    assert_eq!(w.put("/h", "101")["Version"], 101);
    let history = w.ok(
        "GetParameterHistory",
        &json!({"Name": "/h", "MaxResults": 50}),
    );
    let numbers: Vec<i64> = history["Parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["Version"].as_i64().unwrap())
        .collect();
    assert_eq!(&numbers[..2], &[1, 3]);
    assert!(history.get("NextToken").is_some());
    // With every version labelled there is nothing to drop.
    let mut w = World::new();
    for n in 1..=100 {
        w.put("/l", &n.to_string());
        w.ok(
            "LabelParameterVersion",
            &json!({"Name": "/l", "ParameterVersion": n, "Labels": [format!("v{n}")]}),
        );
    }
    assert_eq!(
        w.err(
            "PutParameter",
            &json!({"Name": "/l", "Value": "x", "Type": "String", "Overwrite": true})
        )
        .0,
        "ParameterMaxVersionLimitExceeded"
    );
}

#[test]
fn an_expiration_policy_removes_the_parameter_when_its_time_comes() {
    let mut w = World::new();
    // The manual clock reads 1_700_000_000_000 ms (2023-11-14T22:13:20Z).
    let policy = r#"[{"Type":"Expiration","Version":"1.0","Attributes":{"Timestamp":"2023-11-14T23:00:00.000Z"}}]"#;
    assert_eq!(
        w.err(
            "PutParameter",
            &json!({"Name": "/e", "Value": "v", "Type": "String", "Policies": policy})
        )
        .0,
        "ValidationException",
        "standard parameters carry no policies"
    );
    w.ok("PutParameter", &json!({"Name": "/e", "Value": "v", "Type": "String", "Tier": "Advanced", "Policies": policy}));
    assert_eq!(
        w.ok("DescribeParameters", &json!({}))["Parameters"][0]["Policies"][0]["PolicyType"],
        "Expiration"
    );
    w.clock.set(1_700_000_000_000 + 3_000_000);
    assert_eq!(
        w.err("GetParameter", &json!({"Name": "/e"})).0,
        "ParameterNotFound"
    );
    assert_eq!(
        w.ok("DescribeParameters", &json!({}))["Parameters"],
        json!([])
    );
}

#[test]
fn a_tag_call_with_overwrite_is_refused() {
    let mut w = World::new();
    w.put("/o", "v");
    let tags = json!([{"Key": "k", "Value": "v"}]);
    assert_eq!(
        w.err(
            "PutParameter",
            &json!({"Name": "/o", "Value": "w", "Type": "String", "Overwrite": true, "Tags": tags})
        )
        .0,
        "ValidationException"
    );
}
