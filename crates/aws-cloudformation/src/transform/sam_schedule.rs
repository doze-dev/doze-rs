//! SAM's `ScheduleV2` event: an EventBridge Scheduler schedule (`AWS::Scheduler::Schedule`), not a rule. Until Scheduler was served
//! doze-aws made these as rules (close, but a rule has no time zone, no flexible window, no `at()` and no end date); now it makes what
//! SAM makes. SAM's expansion is from its documentation and memory (`(memory)`): the schedule is named `<owner><event>` unless
//! `OmitName`, and when no `RoleArn` is given an execution role is made that trusts `scheduler.amazonaws.com` and may do the one action
//! the target needs.

use serde_json::{Value, json};

use super::sam::resource;
use super::sam_events::{Out, pick};
use crate::j::J;

/// What the schedule's target does, for the role SAM makes when the event names none.
pub(crate) struct Target<'a> {
    /// The target's ARN (a `Ref` or `Fn::GetAtt`).
    pub arn: Value,
    /// The one action the role must allow: `lambda:InvokeFunction` or `states:StartExecution`.
    pub action: &'a str,
}

/// Puts the schedule (and its role, when SAM would make one) for event `ev` of `owner` into `out`.
pub(crate) fn schedule_event(
    owner: &str,
    ev: &str,
    props: &Value,
    target: &Target<'_>,
    r: &Value,
    out: &mut Out,
) {
    let id = format!("{owner}{ev}");
    let mut s = pick(
        props,
        &[
            "ScheduleExpression",
            "ScheduleExpressionTimezone",
            "Description",
            "GroupName",
            "StartDate",
            "EndDate",
            "State",
            "KmsKeyArn",
        ],
    );
    if props.get("OmitName").and_then(Value::as_bool) != Some(true) {
        s.put(
            "Name",
            props
                .get("Name")
                .cloned()
                .unwrap_or_else(|| id.clone().into()),
        );
    }
    s.put(
        "FlexibleTimeWindow",
        props
            .get("FlexibleTimeWindow")
            .cloned()
            .unwrap_or_else(|| json!({"Mode": "OFF"})),
    );
    let role = match props.get("RoleArn") {
        Some(arn) => arn.clone(),
        None => {
            let role_id = format!("{id}Role");
            out.put(&role_id, execution_role(&id, target, props, r));
            json!({"Fn::GetAtt": [role_id, "Arn"]})
        }
    };
    let mut t = pick(props, &["Input", "RetryPolicy"]);
    t.put("Arn", target.arn.clone());
    t.put("RoleArn", role);
    if let Some(dlq) = props.get("DeadLetterConfig").and_then(|d| d.get("Arn")) {
        t.put("DeadLetterConfig", json!({"Arn": dlq}));
    }
    s.put("Target", t);
    out.put(&id, resource("AWS::Scheduler::Schedule", s, r));
}

/// The role SAM makes for a schedule: trusted by Scheduler, allowed the target's one action (and the dead-letter send, if any).
fn execution_role(id: &str, target: &Target<'_>, props: &Value, r: &Value) -> Value {
    let mut statements =
        vec![json!({"Effect": "Allow", "Action": target.action, "Resource": target.arn})];
    if let Some(dlq) = props.get("DeadLetterConfig").and_then(|d| d.get("Arn")) {
        statements.push(json!({"Effect": "Allow", "Action": "sqs:SendMessage", "Resource": dlq}));
    }
    let trust = json!({"Version": "2012-10-17", "Statement": [
        {"Effect": "Allow", "Action": ["sts:AssumeRole"], "Principal": {"Service": ["scheduler.amazonaws.com"]}}]});
    let mut role = json!({
        "AssumeRolePolicyDocument": trust,
        "Policies": [{"PolicyName": format!("{id}StartExecutionPolicy"),
            "PolicyDocument": {"Version": "2012-10-17", "Statement": statements}}],
    });
    if let Some(b) = props.get("PermissionsBoundary") {
        role.put("PermissionsBoundary", b.clone());
    }
    resource("AWS::IAM::Role", role, r)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::tmpl::eval::Pseudo;

    fn expand(t: Value) -> Value {
        let mut t = t;
        let pseudo = Pseudo {
            account: "111122223333".into(),
            region: "us-east-1".into(),
            stack_id: "id".into(),
            stack_name: "s".into(),
            notification_arns: Vec::new(),
        };
        super::super::sam::apply(&mut t, &pseudo).expect("expands");
        t
    }

    #[test]
    fn a_function_schedule_v2_is_a_scheduler_schedule_with_a_role_scheduler_may_assume() {
        let t = expand(
            json!({"Resources": {"Fn": {"Type": "AWS::Serverless::Function", "Properties": {
            "Runtime": "python3.12", "Handler": "a.h", "InlineCode": "x",
            "Events": {"Tick": {"Type": "ScheduleV2", "Properties": {
                "ScheduleExpression": "cron(0 9 * * ? *)", "ScheduleExpressionTimezone": "Europe/London"}}}}}}}),
        );
        let s = &t["Resources"]["FnTick"];
        assert_eq!(s["Type"], "AWS::Scheduler::Schedule");
        assert_eq!(s["Properties"]["Name"], "FnTick");
        assert_eq!(
            s["Properties"]["ScheduleExpressionTimezone"],
            "Europe/London"
        );
        assert_eq!(
            s["Properties"]["FlexibleTimeWindow"],
            json!({"Mode": "OFF"})
        );
        assert_eq!(
            s["Properties"]["Target"]["Arn"],
            json!({"Fn::GetAtt": ["Fn", "Arn"]})
        );
        assert_eq!(
            s["Properties"]["Target"]["RoleArn"],
            json!({"Fn::GetAtt": ["FnTickRole", "Arn"]})
        );
        let role = &t["Resources"]["FnTickRole"]["Properties"];
        assert_eq!(
            role["AssumeRolePolicyDocument"]["Statement"][0]["Principal"]["Service"][0],
            "scheduler.amazonaws.com"
        );
        assert_eq!(
            role["Policies"][0]["PolicyDocument"]["Statement"][0]["Action"],
            "lambda:InvokeFunction"
        );
        assert!(
            t["Resources"].get("FnTickPermission").is_none(),
            "a schedule invokes with its role, not a resource policy"
        );
    }

    #[test]
    fn a_given_role_is_used_and_none_is_made_and_omit_name_leaves_the_name_to_cloudformation() {
        let t = expand(
            json!({"Resources": {"Sm": {"Type": "AWS::Serverless::StateMachine", "Properties": {
            "Definition": {"StartAt": "A", "States": {"A": {"Type": "Pass", "End": true}}},
            "Events": {"Nightly": {"Type": "ScheduleV2", "Properties": {
                "ScheduleExpression": "rate(1 day)", "RoleArn": "arn:aws:iam::111122223333:role/r", "OmitName": true}}}}}}}),
        );
        let s = &t["Resources"]["SmNightly"]["Properties"];
        assert_eq!(s["Target"]["RoleArn"], "arn:aws:iam::111122223333:role/r");
        assert_eq!(s["Target"]["Arn"], json!({"Ref": "Sm"}));
        assert!(s.get("Name").is_none());
        assert!(t["Resources"].get("SmNightlyRole").is_none());
    }
}
