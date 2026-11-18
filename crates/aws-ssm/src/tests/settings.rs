//! Service settings and public parameters: defaults until updated, and AWS's read-only names under `/aws/service/`.

use serde_json::json;

use super::World;

#[test]
fn service_settings_hold_defaults_until_updated() {
    let mut w = World::new();
    let id = "/ssm/parameter-store/high-throughput-enabled";
    let got = w.ok("GetServiceSetting", &json!({"SettingId": id}))["ServiceSetting"].clone();
    assert_eq!(
        (got["SettingValue"].as_str(), got["Status"].as_str()),
        (Some("false"), Some("Default"))
    );
    assert_eq!(
        got["ARN"],
        format!("arn:aws:ssm:us-east-1:000000000000:servicesetting{id}")
    );
    w.ok(
        "UpdateServiceSetting",
        &json!({"SettingId": got["ARN"], "SettingValue": "true"}),
    );
    let now = w.ok("GetServiceSetting", &json!({"SettingId": id}))["ServiceSetting"].clone();
    assert_eq!(
        (now["SettingValue"].as_str(), now["Status"].as_str()),
        (Some("true"), Some("Customized"))
    );
    let reset = w.ok("ResetServiceSetting", &json!({"SettingId": id}))["ServiceSetting"].clone();
    assert_eq!(reset["SettingValue"], "false");
    assert_eq!(
        w.err(
            "UpdateServiceSetting",
            &json!({"SettingId": id, "SettingValue": "maybe"})
        )
        .0,
        "ValidationException"
    );
    assert_eq!(
        w.err("GetServiceSetting", &json!({"SettingId": "/ssm/nothing"}))
            .0,
        "ServiceSettingNotFound"
    );
    assert_eq!(
        w.err(
            "GetServiceSetting",
            &json!({"SettingId": "/ssm/managed-instance/default-ec2-instance-management-role"})
        )
        .0,
        "ServiceSettingNotFound"
    );
    // The default tier is the tier of a parameter written without one.
    w.ok("UpdateServiceSetting", &json!({"SettingId": "/ssm/parameter-store/default-parameter-tier", "SettingValue": "Advanced"}));
    assert_eq!(w.put("/tiered", "v")["Tier"], "Advanced");
}

#[test]
fn public_parameters_are_read_and_never_written() {
    let mut w = World::new();
    let name = "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64";
    let p = w.ok("GetParameter", &json!({"Name": name}))["Parameter"].clone();
    assert!(p["Value"].as_str().unwrap().starts_with("ami-0"));
    assert_eq!(p["ARN"], format!("arn:aws:ssm:us-east-1::parameter{name}"));
    assert_eq!(
        w.ok("GetParameter", &json!({"Name": format!("{name}:1")}))["Parameter"]["Selector"],
        ":1"
    );
    assert_eq!(
        w.err("GetParameter", &json!({"Name": format!("{name}:2")}))
            .0,
        "ParameterVersionNotFound"
    );
    assert_eq!(
        w.err("GetParameter", &json!({"Name": "/aws/service/ecs/absent"}))
            .0,
        "ParameterNotFound"
    );
    let many = w.ok(
        "GetParameters",
        &json!({"Names": [name, "/aws/service/ecs/absent"]}),
    );
    assert_eq!(many["Parameters"].as_array().unwrap().len(), 1);
    assert_eq!(
        many["InvalidParameters"],
        json!(["/aws/service/ecs/absent"])
    );
    let path = "/aws/service/global-infrastructure/services";
    let page = w.ok(
        "GetParametersByPath",
        &json!({"Path": path, "MaxResults": 5}),
    );
    assert_eq!(page["Parameters"].as_array().unwrap().len(), 5);
    let rest = w.ok(
        "GetParametersByPath",
        &json!({"Path": path, "NextToken": page["NextToken"], "MaxResults": 10}),
    );
    assert_eq!(rest["Parameters"][0]["Name"], format!("{path}/ecs"));
    let deep = w.ok("GetParametersByPath", &json!({"Path": "/aws/service/ami-amazon-linux-latest", "Recursive": true, "MaxResults": 10}));
    assert!(deep["NextToken"].is_string());
    assert_eq!(
        w.ok("DescribeParameters", &json!({}))["Parameters"],
        json!([])
    );
    assert_eq!(
        w.err(
            "PutParameter",
            &json!({"Name": name, "Value": "ami-x", "Type": "String", "Overwrite": true})
        )
        .0,
        "AccessDeniedException"
    );
}

#[test]
fn a_name_outside_every_aws_namespace_is_refused_by_every_read() {
    let mut w = World::new();
    let refused =
        "No access to \"/aws/\" namespace: aws/service/not-a-parameter is not a valid namespace";
    let made_up = "/aws/service/not-a-parameter";
    let al = "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64";
    for (op, request) in [
        ("GetParameter", json!({"Name": made_up})),
        ("GetParameters", json!({"Names": [al, made_up]})),
        ("GetParametersByPath", json!({"Path": made_up})),
        (
            "DescribeParameters",
            json!({"ParameterFilters": [{"Key": "Name", "Option": "BeginsWith", "Values": [made_up]}]}),
        ),
    ] {
        let (code, message) = w.err(op, &request);
        assert_eq!(
            (code.as_str(), message.as_str()),
            ("AccessDeniedException", refused),
            "{op}"
        );
    }
    let (_, message) = w.err(
        "DescribeParameters",
        &json!({"ParameterFilters": [{"Key": "Name", "Option": "BeginsWith", "Values": ["/aws/service/"]}]}),
    );
    assert!(
        message.contains("aws/service/ is not a valid namespace"),
        "{message}"
    );
}

#[test]
fn describe_parameters_lists_aws_parameters_inside_a_namespace() {
    let mut w = World::new();
    let prefix = "/aws/service/ami-amazon-linux-latest/";
    let filter = json!([{"Key": "Name", "Option": "BeginsWith", "Values": [prefix]}]);
    let page = w.ok("DescribeParameters", &json!({"ParameterFilters": filter}));
    let listed = page["Parameters"].as_array().unwrap();
    assert_eq!(listed.len(), 10);
    assert!(page["NextToken"].is_string());
    let first = listed.first().unwrap();
    assert_eq!(
        (
            first["Type"].as_str(),
            first["Tier"].as_str(),
            first["DataType"].as_str()
        ),
        (Some("String"), Some("Standard"), Some("text"))
    );
    assert_eq!(first["Policies"], json!([]));
    assert!(
        first["ARN"]
            .as_str()
            .unwrap()
            .starts_with("arn:aws:ssm:us-east-1::parameter/aws/")
    );
    let rest = w.ok(
        "DescribeParameters",
        &json!({"ParameterFilters": filter, "NextToken": page["NextToken"]}),
    );
    assert!(
        rest["Parameters"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["Name"].as_str().unwrap().starts_with(prefix))
    );
}
