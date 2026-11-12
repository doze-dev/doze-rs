//! A parameter's life: versions, labels, history, paths, paging and tags. The expected answers are those AWS gave when the
//! conformance scenarios were recorded, where they were.

use serde_json::json;

use super::World;

#[test]
fn a_parameter_from_creation_to_deletion() {
    let mut w = World::new();
    let put = w.ok(
        "PutParameter",
        &json!({"Name": "/app/db/host", "Value": "one", "Type": "String"}),
    );
    assert_eq!(put, json!({"Version": 1, "Tier": "Standard"}));
    let got = w.ok("GetParameter", &json!({"Name": "/app/db/host"}));
    let p = &got["Parameter"];
    assert_eq!(
        (
            p["Value"].as_str(),
            p["Version"].as_i64(),
            p["Type"].as_str(),
            p["DataType"].as_str()
        ),
        (Some("one"), Some(1), Some("String"), Some("text"))
    );
    assert_eq!(
        p["ARN"],
        "arn:aws:ssm:us-east-1:000000000000:parameter/app/db/host"
    );

    assert_eq!(
        w.err(
            "PutParameter",
            &json!({"Name": "/app/db/host", "Value": "two", "Type": "String"})
        )
        .0,
        "ParameterAlreadyExists"
    );
    let two = w.put("/app/db/host", "two");
    assert_eq!(two["Version"], 2);
    assert_eq!(
        w.ok("GetParameter", &json!({"Name": "/app/db/host"}))["Parameter"]["Value"],
        "two"
    );
    let by_version = w.ok("GetParameter", &json!({"Name": "/app/db/host:1"}));
    assert_eq!(
        (
            by_version["Parameter"]["Value"].as_str(),
            by_version["Parameter"]["Selector"].as_str()
        ),
        (Some("one"), Some(":1"))
    );
}

#[test]
fn versions_have_labels_a_history_and_a_description() {
    let mut w = World::new();
    w.put("/app/db/host", "one");
    w.put("/app/db/host", "two");
    let labelled = w.ok(
        "LabelParameterVersion",
        &json!({"Name": "/app/db/host", "ParameterVersion": 1, "Labels": ["stable"]}),
    );
    assert_eq!(
        labelled,
        json!({"InvalidLabels": [], "ParameterVersion": 1})
    );
    let by_label = w.ok("GetParameter", &json!({"Name": "/app/db/host:stable"}));
    assert_eq!(by_label["Parameter"]["Value"], "one");

    let history = w.ok("GetParameterHistory", &json!({"Name": "/app/db/host"}));
    let versions: Vec<_> = history["Parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| (h["Version"].as_i64().unwrap(), h["Labels"].clone()))
        .collect();
    assert_eq!(versions, [(1, json!(["stable"])), (2, json!([]))]);

    let described = w.ok(
        "DescribeParameters",
        &json!({"ParameterFilters": [{"Key": "Name", "Values": ["/app/db/host"]}]}),
    );
    let d = &described["Parameters"][0];
    assert_eq!(
        (
            d["Version"].as_i64(),
            d["Tier"].as_str(),
            d["Policies"].clone()
        ),
        (Some(2), Some("Standard"), json!([]))
    );

    w.ok("DeleteParameter", &json!({"Name": "/app/db/host"}));
    let (code, message) = w.err("GetParameter", &json!({"Name": "/app/db/host"}));
    assert_eq!((code.as_str(), message.as_str()), ("ParameterNotFound", ""));
}

#[test]
fn a_label_lives_on_one_version_and_moves() {
    let mut w = World::new();
    w.put("/p", "1");
    w.put("/p", "2");
    w.ok(
        "LabelParameterVersion",
        &json!({"Name": "/p", "ParameterVersion": 1, "Labels": ["live"]}),
    );
    w.ok(
        "LabelParameterVersion",
        &json!({"Name": "/p", "ParameterVersion": 2, "Labels": ["live"]}),
    );
    assert_eq!(
        w.ok("GetParameter", &json!({"Name": "/p:live"}))["Parameter"]["Version"],
        2
    );
    let bad = w.ok(
        "LabelParameterVersion",
        &json!({"Name": "/p", "Labels": ["1bad", "good"]}),
    );
    assert_eq!(bad["InvalidLabels"], json!(["1bad"]));
    let un = w.ok(
        "UnlabelParameterVersion",
        &json!({"Name": "/p", "ParameterVersion": 2, "Labels": ["live", "absent"]}),
    );
    assert_eq!(
        (un["RemovedLabels"].clone(), un["InvalidLabels"].clone()),
        (json!(["live"]), json!(["absent"]))
    );
}

#[test]
fn a_secure_string_is_sealed_until_it_is_asked_for() {
    let mut w = World::new();
    w.ok(
        "PutParameter",
        &json!({"Name": "/pw", "Value": "hunter2", "Type": "SecureString"}),
    );
    let sealed = w.ok("GetParameter", &json!({"Name": "/pw"}));
    assert_ne!(sealed["Parameter"]["Value"], "hunter2");
    let open = w.ok(
        "GetParameter",
        &json!({"Name": "/pw", "WithDecryption": true}),
    );
    assert_eq!(open["Parameter"]["Value"], "hunter2");
    // An overwrite that leaves the type out keeps it: the new value is sealed too.
    w.ok(
        "PutParameter",
        &json!({"Name": "/pw", "Value": "second", "Overwrite": true}),
    );
    assert_ne!(
        w.ok("GetParameter", &json!({"Name": "/pw"}))["Parameter"]["Value"],
        "second"
    );
    assert_eq!(
        w.ok(
            "GetParameter",
            &json!({"Name": "/pw", "WithDecryption": true})
        )["Parameter"]["Value"],
        "second"
    );
    let described = w.ok("DescribeParameters", &json!({}));
    assert_eq!(described["Parameters"][0]["KeyId"], "alias/aws/ssm");
}

#[test]
fn paths_list_one_level_or_all_and_page() {
    let mut w = World::new();
    for leaf in ["a", "b", "nested/c"] {
        w.put(&format!("/r/{leaf}"), leaf);
    }
    w.put("/other/x", "x");
    let names = |v: &serde_json::Value| -> Vec<String> {
        v["Parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["Name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        names(&w.ok("GetParametersByPath", &json!({"Path": "/r"}))),
        ["/r/a", "/r/b"]
    );
    assert_eq!(
        names(&w.ok(
            "GetParametersByPath",
            &json!({"Path": "/r", "Recursive": true})
        )),
        ["/r/a", "/r/b", "/r/nested/c"]
    );
    let first = w.ok(
        "GetParametersByPath",
        &json!({"Path": "/r", "Recursive": true, "MaxResults": 2}),
    );
    assert_eq!(names(&first), ["/r/a", "/r/b"]);
    let rest = w.ok(
        "GetParametersByPath",
        &json!({"Path": "/r", "Recursive": true, "MaxResults": 2, "NextToken": first["NextToken"]}),
    );
    assert_eq!(names(&rest), ["/r/nested/c"]);
    assert!(rest.get("NextToken").is_none());
    // Exactly at the end of a page there is no token to follow.
    let exact = w.ok(
        "GetParametersByPath",
        &json!({"Path": "/r", "MaxResults": 2}),
    );
    assert!(exact.get("NextToken").is_none());
}

#[test]
fn bulk_reads_and_deletes_name_what_is_absent() {
    let mut w = World::new();
    w.put("/b/a", "a");
    let got = w.ok("GetParameters", &json!({"Names": ["/b/a", "/b/absent"]}));
    assert_eq!(
        (
            got["Parameters"][0]["Name"].as_str(),
            got["InvalidParameters"].clone()
        ),
        (Some("/b/a"), json!(["/b/absent"]))
    );
    let del = w.ok("DeleteParameters", &json!({"Names": ["/b/a", "/b/absent"]}));
    assert_eq!(
        (
            del["DeletedParameters"].clone(),
            del["InvalidParameters"].clone()
        ),
        (json!(["/b/a"]), json!(["/b/absent"]))
    );
}

#[test]
fn tags_are_added_listed_and_removed() {
    let mut w = World::new();
    w.ok("PutParameter", &json!({"Name": "/t", "Value": "v", "Type": "String", "Tags": [{"Key": "env", "Value": "dev"}]}));
    w.ok("AddTagsToResource", &json!({"ResourceType": "Parameter", "ResourceId": "/t", "Tags": [{"Key": "team", "Value": "x"}]}));
    let listed = w.ok(
        "ListTagsForResource",
        &json!({"ResourceType": "Parameter", "ResourceId": "/t"}),
    );
    assert_eq!(
        listed["TagList"],
        json!([{"Key": "env", "Value": "dev"}, {"Key": "team", "Value": "x"}])
    );
    w.ok(
        "RemoveTagsFromResource",
        &json!({"ResourceType": "Parameter", "ResourceId": "/t", "TagKeys": ["env"]}),
    );
    assert_eq!(
        w.ok(
            "ListTagsForResource",
            &json!({"ResourceType": "Parameter", "ResourceId": "/t"})
        )["TagList"],
        json!([{"Key": "team", "Value": "x"}])
    );
    assert_eq!(
        w.err(
            "ListTagsForResource",
            &json!({"ResourceType": "Parameter", "ResourceId": "/absent"})
        )
        .0,
        "InvalidResourceId"
    );
    assert_eq!(
        w.err(
            "ListTagsForResource",
            &json!({"ResourceType": "Document", "ResourceId": "d"})
        )
        .0,
        "InvalidResourceType"
    );
}
