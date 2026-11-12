//! What a request asks to do, for the authorizer: the IAM action (`kinesis:PutRecord`) and the ARN of the stream it names.

use aws_model_kinesis::Operation;
use aws_peers::Need;
use aws_wire::service::Call;
use serde_json::Value;

use crate::model::stream_arn;
use crate::req::name_of_arn;

/// Every thing `call` asks to do.
pub(crate) fn needs(call: &Call) -> Vec<Need> {
    use Operation as O;
    let Some(op) = Operation::from_index(call.op) else {
        return Vec::new();
    };
    let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
    let text = |key: &str| body.get(key).and_then(Value::as_str);
    let (region, account) = (&call.caller.region, &call.caller.account_id);
    let action = format!("kinesis:{op:?}");
    let resource = match op {
        O::ListStreams | O::DescribeLimits => "*".to_owned(),
        O::GetRecords => text("ShardIterator")
            .and_then(crate::reading::stream_of_iterator)
            .map_or_else(
                || format!("arn:aws:kinesis:{region}:{account}:stream/*"),
                |name| stream_arn(region, account, &name),
            ),
        O::DescribeStreamConsumer | O::DeregisterStreamConsumer
            if text("ConsumerARN").is_some() =>
        {
            text("ConsumerARN").unwrap_or_default().to_owned()
        }
        _ => text("StreamName")
            .map(str::to_owned)
            .or_else(|| text("StreamARN").and_then(name_of_arn))
            .or_else(|| text("ResourceARN").and_then(name_of_arn))
            .or_else(|| text("ConsumerARN").and_then(name_of_arn))
            .map_or_else(
                || format!("arn:aws:kinesis:{region}:{account}:stream/*"),
                |name| stream_arn(region, account, &name),
            ),
    };
    vec![Need::new(action, resource)]
}
