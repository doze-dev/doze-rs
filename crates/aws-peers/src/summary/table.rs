//! Which operations have a summary, and where it is read from: one table, not a reader per service. Two halves, because what a
//! call did is sometimes in what it was asked (the size of a message sent) and sometimes in the answer (how many came back).
//! Both halves match on the API's SDK id and the operation's name.

use super::{Fields, Kind, Summary};

/// The most bytes of detail kept (a session name is at most 64).
pub const MOST_DETAIL: usize = 64;

/// The count `member` holds, clamped to a count.
fn small(n: u32) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

fn number(f: &dyn Fields, member: &str) -> u32 {
    f.number(member)
        .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX))
}

fn set_detail(detail: &mut String, text: Option<&str>) {
    detail.clear();
    let Some(text) = text else { return };
    let mut end = text.len().min(MOST_DETAIL);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    detail.push_str(text.get(..end).unwrap_or_default());
}

/// Reads what the operation was asked for: `input` is its decoded input.
pub fn from_input(
    sdk_id: &str,
    action: &str,
    input: &dyn Fields,
    s: &mut Summary,
    detail: &mut String,
) {
    match (sdk_id, action) {
        ("SQS", "SendMessage") => {
            let n = input.text("MessageBody").map_or(0, str::len);
            *s = Summary::of(Kind::Sent, u32::try_from(n).unwrap_or(u32::MAX), 0);
        }
        ("S3", "PutObject" | "UploadPart") => {
            if let Some(n) = input.number("ContentLength") {
                *s = Summary::of(Kind::Size, u32::try_from(n).unwrap_or(u32::MAX), 0);
            }
        }
        ("CloudWatch Logs", "PutLogEvents") => {
            *s = Summary::of(Kind::Events, input.count("logEvents").unwrap_or(0), 0);
        }
        ("EventBridge", "PutEvents") => {
            *s = Summary::of(Kind::Events, input.count("Entries").unwrap_or(0), 0);
        }
        ("Kinesis", "PutRecords") => {
            *s = Summary::of(Kind::Records, input.count("Records").unwrap_or(0), 0);
        }
        ("Kinesis", "PutRecord") => *s = Summary::of(Kind::Records, 1, 0),
        ("CloudWatch", "PutMetricData") => {
            *s = Summary::of(Kind::Metrics, input.count("MetricData").unwrap_or(0), 0);
        }
        ("DynamoDB", "Query" | "Scan") => set_detail(detail, input.text("IndexName")),
        ("STS", "AssumeRole" | "AssumeRoleWithWebIdentity" | "AssumeRoleWithSAML") => {
            if input.text("RoleSessionName").is_some() {
                *s = Summary::of(Kind::Session, 0, 0);
                set_detail(detail, input.text("RoleSessionName"));
            }
        }
        ("IAM", "AttachRolePolicy" | "DetachRolePolicy") => to(input, "RoleName", s, detail),
        ("IAM", "AttachUserPolicy" | "DetachUserPolicy") => to(input, "UserName", s, detail),
        ("IAM", "AttachGroupPolicy" | "DetachGroupPolicy") => to(input, "GroupName", s, detail),
        ("IAM", "AddUserToGroup" | "RemoveUserFromGroup") => to(input, "UserName", s, detail),
        _ => {}
    }
}

fn to(input: &dyn Fields, member: &str, s: &mut Summary, detail: &mut String) {
    if let Some(name) = input.text(member) {
        *s = Summary::of(Kind::To, 0, 0);
        set_detail(detail, Some(name));
    }
}

/// Reads what the operation answered: `output` is its decoded answer. Called only for an answer that succeeded.
pub fn from_output(sdk_id: &str, action: &str, output: &dyn Fields, s: &mut Summary) {
    let count = |member: &str| output.count(member).unwrap_or(0);
    match (sdk_id, action) {
        ("SQS", "ReceiveMessage") => *s = Summary::of(Kind::Messages, count("Messages"), 0),
        ("SQS", "SendMessageBatch") => {
            *s = Summary::of(
                Kind::SentMessages,
                count("Successful"),
                small(count("Failed")),
            );
        }
        ("SQS", "DeleteMessageBatch") => {
            *s = Summary::of(
                Kind::DeletedMessages,
                count("Successful"),
                small(count("Failed")),
            );
        }
        ("S3", _) => from_s3(action, output, s),
        ("DynamoDB", "Query" | "Scan") => *s = Summary::of(Kind::Items, number(output, "Count"), 0),
        ("DynamoDB", "GetItem") => {
            *s = Summary::of(Kind::Items, u32::from(output.present("Item")), 0);
        }
        ("CloudWatch Logs", "GetLogEvents" | "FilterLogEvents") => {
            *s = Summary::of(Kind::Events, count("events"), 0);
        }
        ("EventBridge", "PutEvents") => {
            s.failed = small(number(output, "FailedEntryCount"));
        }
        ("Kinesis", "GetRecords") => *s = Summary::of(Kind::Records, count("Records"), 0),
        ("Kinesis", "PutRecords") => s.failed = small(number(output, "FailedRecordCount")),
        ("SNS", "ListSubscriptionsByTopic" | "ListSubscriptions") => {
            *s = Summary::of(Kind::Subscriptions, count("Subscriptions"), 0);
        }
        ("SNS", "Subscribe") => {
            if let Some(arn) = output.text("SubscriptionArn") {
                let pending = arn.eq_ignore_ascii_case("pending confirmation");
                *s = Summary::of(
                    if pending {
                        Kind::Pending
                    } else {
                        Kind::Confirmed
                    },
                    0,
                    0,
                );
            }
        }
        ("SNS", "Publish" | "PublishBatch") => *s = Summary::of(Kind::Delivered, 0, 0),
        ("IAM", _) => from_iam(action, output, s),
        ("SFN", "GetActivityTask") => {
            *s = Summary::of(Kind::Task, u32::from(output.present("taskToken")), 0);
        }
        ("Lambda", "Invoke") if output.flag(super::DROPPED) == Some(true) => {
            *s = Summary::of(Kind::Dropped, 0, 0);
        }
        ("CloudWatch", "GetMetricData") => {
            *s = Summary::of(Kind::Series, count("MetricDataResults"), 0);
        }
        _ => {}
    }
}

/// What an IAM list or `GetPolicyVersion` answered: how many it holds, or which version it read.
fn from_iam(action: &str, output: &dyn Fields, s: &mut Summary) {
    let count = |member: &str| output.count(member).unwrap_or(0);
    match action {
        "ListGroupsForUser" => *s = Summary::of(Kind::Groups, count("Groups"), 0),
        "ListAttachedUserPolicies" | "ListAttachedGroupPolicies" | "ListAttachedRolePolicies" => {
            *s = Summary::of(Kind::Policies, count("AttachedPolicies"), 0);
        }
        "ListUserPolicies" | "ListGroupPolicies" | "ListRolePolicies" => {
            *s = Summary::of(Kind::Policies, count("PolicyNames"), 0);
        }
        "ListAccessKeys" => *s = Summary::of(Kind::AccessKeys, count("AccessKeyMetadata"), 0),
        "ListMFADevices" => *s = Summary::of(Kind::MfaDevices, count("MFADevices"), 0),
        "ListEntitiesForPolicy" => {
            let n = count("PolicyUsers")
                .saturating_add(count("PolicyGroups"))
                .saturating_add(count("PolicyRoles"));
            *s = Summary::of(Kind::Entities, n, 0);
        }
        "ListPolicyVersions" => *s = Summary::of(Kind::Versions, count("Versions"), 0),
        "GetPolicyVersion" => {
            // IAM numbers versions `v1`, `v2`...; an id of another shape is not guessed at.
            let n = output
                .text_in("PolicyVersion", "VersionId")
                .and_then(|v| v.strip_prefix('v'))
                .and_then(|v| v.parse::<u32>().ok());
            if let Some(n) = n {
                *s = Summary::of(Kind::Version, n, 0);
            }
        }
        _ => {}
    }
}

/// What an S3 operation answered.
fn from_s3(action: &str, output: &dyn Fields, s: &mut Summary) {
    let count = |member: &str| output.count(member).unwrap_or(0);
    match action {
        "ListObjectsV2" | "ListObjects" => {
            let more = output.flag("IsTruncated") == Some(true);
            *s = Summary::of(Kind::Keys, count("Contents"), u16::from(more));
        }
        "ListObjectVersions" => {
            let more = output.flag("IsTruncated") == Some(true);
            *s = Summary::of(Kind::Keys, count("Versions"), u16::from(more));
        }
        "ListBuckets" => *s = Summary::of(Kind::Buckets, count("Buckets"), 0),
        "GetObject" | "HeadObject" => {
            if let Some(n) = output.number("ContentLength") {
                *s = Summary::of(Kind::Size, u32::try_from(n).unwrap_or(u32::MAX), 0);
            }
        }
        "DeleteObjects" => {
            *s = Summary::of(Kind::DeletedKeys, count("Deleted"), small(count("Errors")));
        }
        _ => {}
    }
}
