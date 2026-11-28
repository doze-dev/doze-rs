//! What a request asks to do, for the authorizer: the IAM action (`dynamodb:PutItem`) and the ARN of each table or index it names.
//!
//! The context keys `dynamodb:LeadingKeys` and `dynamodb:Attributes` are not supplied: a policy that tests them finds them absent.

use aws_model_dynamodb::Operation;
use aws_model_dynamodb::tables::TABLES;
use aws_peers::Need;
use aws_peers::input::In;
use aws_wire::service::Call;

use crate::req::table_arn;

/// The text of member `key` of `v`.
fn str_of<'a>(v: &In<'a>, key: &str) -> Option<&'a str> {
    v.text(key)
}

/// Every thing `call` asks to do.
pub(crate) fn needs(call: &Call) -> Vec<Need> {
    let Some(op) = Operation::from_index(call.op) else {
        return Vec::new();
    };
    let body = In::of(&TABLES, call);
    let (region, account) = (call.caller.region.as_str(), call.caller.account_id.as_str());
    let on = |action: &str, name: &str| {
        let bare = name.rsplit_once("table/").map_or(name, |(_, n)| n);
        Need::new(
            format!("dynamodb:{action}"),
            table_arn(region, account, bare),
        )
    };
    let action = format!("{op:?}");
    if let Some(n) = task_needs(op, &body, &action, &on) {
        return n;
    }
    match op {
        Operation::ListTables | Operation::DescribeLimits | Operation::DescribeEndpoints => {
            vec![Need::new(format!("dynamodb:{action}"), "*")]
        }
        Operation::Query | Operation::Scan => {
            let table = str_of(&body, "TableName").unwrap_or_default();
            let bare = table.rsplit_once("table/").map_or(table, |(_, n)| n);
            let mut arn = table_arn(region, account, bare);
            if let Some(ix) = str_of(&body, "IndexName") {
                arn = format!("{arn}/index/{ix}");
            }
            vec![Need::new(format!("dynamodb:{action}"), arn)]
        }
        Operation::BatchGetItem | Operation::BatchWriteItem => body
            .get("RequestItems")
            .into_iter()
            .flat_map(In::keys)
            .map(|t| on(&action, t))
            .collect(),
        Operation::TransactWriteItems | Operation::TransactGetItems => transaction(&body, &on),
        Operation::ExecuteStatement => partiql(vec![body], region, account),
        Operation::BatchExecuteStatement => partiql(
            body.get("Statements")
                .and_then(|v| v.as_array())
                .unwrap_or_default(),
            region,
            account,
        ),
        Operation::ExecuteTransaction => partiql(
            body.get("TransactStatements")
                .and_then(|v| v.as_array())
                .unwrap_or_default(),
            region,
            account,
        ),
        Operation::TagResource | Operation::UntagResource | Operation::ListTagsOfResource => {
            vec![Need::new(
                format!("dynamodb:{action}"),
                str_of(&body, "ResourceArn").unwrap_or("*"),
            )]
        }
        _ => vec![on(&action, str_of(&body, "TableName").unwrap_or_default())],
    }
}

/// What the backup, restore, export, import and policy operations ask for.
fn task_needs(
    op: Operation,
    body: &In<'_>,
    action: &str,
    on: &dyn Fn(&str, &str) -> Need,
) -> Option<Vec<Need>> {
    Some(match op {
        Operation::ListBackups | Operation::ListExports | Operation::ListImports => {
            vec![Need::new(format!("dynamodb:{action}"), "*")]
        }
        // On the backup, export or import itself, or the resource a policy is on.
        Operation::DescribeBackup | Operation::DeleteBackup => by_arn(action, body, "BackupArn"),
        Operation::DescribeExport => by_arn(action, body, "ExportArn"),
        Operation::DescribeImport => by_arn(action, body, "ImportArn"),
        Operation::PutResourcePolicy
        | Operation::GetResourcePolicy
        | Operation::DeleteResourcePolicy => by_arn(action, body, "ResourceArn"),
        // A restore or an import makes the table it names.
        Operation::RestoreTableFromBackup | Operation::RestoreTableToPointInTime => {
            vec![on(
                action,
                str_of(body, "TargetTableName").unwrap_or_default(),
            )]
        }
        Operation::ImportTable => vec![on(
            action,
            body.get("TableCreationParameters")
                .and_then(|p| str_of(&p, "TableName"))
                .unwrap_or_default(),
        )],
        Operation::ExportTableToPointInTime => {
            vec![on(action, str_of(body, "TableArn").unwrap_or_default())]
        }
        _ => return None,
    })
}

/// An action on the ARN a member gives.
fn by_arn(action: &str, body: &In<'_>, member: &str) -> Vec<Need> {
    vec![Need::new(
        format!("dynamodb:{action}"),
        str_of(body, member).unwrap_or("*"),
    )]
}

/// A PartiQL statement asks for `dynamodb:PartiQLSelect` (`Insert`, `Update`, `Delete`) on the table, or on the index it reads. One that
/// does not parse asks for nothing: it is refused on its own terms.
fn partiql(statements: Vec<In<'_>>, region: &str, account: &str) -> Vec<Need> {
    statements
        .into_iter()
        .filter_map(|s| crate::partiql::action_of(str_of(&s, "Statement")?))
        .map(|(action, table, index)| {
            let mut arn = table_arn(region, account, &table);
            if let Some(ix) = index {
                arn = format!("{arn}/index/{ix}");
            }
            Need::new(format!("dynamodb:{action}"), arn)
        })
        .collect()
}

/// A transaction asks for the action of each of its parts.
fn transaction(body: &In<'_>, on: &dyn Fn(&str, &str) -> Need) -> Vec<Need> {
    let items = body
        .get("TransactItems")
        .and_then(|v| v.as_array())
        .unwrap_or_default();
    items
        .iter()
        .filter_map(|i| {
            [
                ("ConditionCheck", "ConditionCheckItem"),
                ("Put", "PutItem"),
                ("Delete", "DeleteItem"),
                ("Update", "UpdateItem"),
                ("Get", "GetItem"),
            ]
            .iter()
            .find_map(|(key, action)| {
                let part = i.get(key)?;
                Some(on(action, str_of(&part, "TableName").unwrap_or_default()))
            })
        })
        .collect()
}
