//! What a request asks to do, for the authorizer: the IAM action (`dynamodb:PutItem`) and the ARN of each table or index it names.
//!
//! The context keys `dynamodb:LeadingKeys` and `dynamodb:Attributes` are not supplied: a policy that tests them finds them absent.

use aws_model_dynamodb::Operation;
use aws_peers::Need;
use aws_wire::service::Call;
use serde_json::Value;

use crate::req::{str_of, table_arn};

/// Every thing `call` asks to do.
pub(crate) fn needs(call: &Call) -> Vec<Need> {
    let Some(op) = Operation::from_index(call.op) else {
        return Vec::new();
    };
    let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
    let (region, account) = (call.caller.region.as_str(), call.caller.account_id.as_str());
    let on = |action: &str, name: &str| {
        let bare = name.rsplit_once("table/").map_or(name, |(_, n)| n);
        Need::new(
            format!("dynamodb:{action}"),
            table_arn(region, account, bare),
        )
    };
    let action = format!("{op:?}");
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
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|m| m.keys().map(|t| on(&action, t)))
            .collect(),
        Operation::TransactWriteItems | Operation::TransactGetItems => transaction(&body, &on),
        Operation::ExecuteStatement => partiql(std::slice::from_ref(&body), region, account),
        Operation::BatchExecuteStatement => partiql(
            body.get("Statements")
                .and_then(Value::as_array)
                .map_or(&[][..], Vec::as_slice),
            region,
            account,
        ),
        Operation::ExecuteTransaction => partiql(
            body.get("TransactStatements")
                .and_then(Value::as_array)
                .map_or(&[][..], Vec::as_slice),
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

/// A PartiQL statement asks for `dynamodb:PartiQLSelect` (`Insert`, `Update`, `Delete`) on the table, or on the index it reads. One that
/// does not parse asks for nothing: it is refused on its own terms.
fn partiql(statements: &[Value], region: &str, account: &str) -> Vec<Need> {
    statements
        .iter()
        .filter_map(|s| crate::partiql::action_of(str_of(s, "Statement")?))
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
fn transaction(body: &Value, on: &dyn Fn(&str, &str) -> Need) -> Vec<Need> {
    let items = body
        .get("TransactItems")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
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
                let part = i.get(*key)?;
                Some(on(action, str_of(part, "TableName").unwrap_or_default()))
            })
        })
        .collect()
}
