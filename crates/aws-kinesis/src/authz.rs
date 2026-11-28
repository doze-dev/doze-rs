//! What a request asks to do, for the authorizer: the IAM action (`kinesis:PutRecord`) and the ARN of the stream it names.

use aws_model_kinesis::Operation;
use aws_model_kinesis::tables::TABLES;
use aws_peers::Need;
use aws_peers::input::In;
use aws_wire::service::Call;

use crate::model::stream_arn;
use crate::req::name_of_arn;

/// Every thing `call` asks to do.
pub(crate) fn needs(call: &Call) -> Vec<Need> {
    use Operation as O;
    let Some(op) = Operation::from_index(call.op) else {
        return Vec::new();
    };
    // Read from the input the gateway decoded, not by parsing the body again (a 5 MB PutRecords batch).
    let body = In::of(&TABLES, call);
    let text = |key: &str| body.text(key);
    let (region, account) = (&call.caller.region, &call.caller.account_id);
    let action = format!("kinesis:{op:?}");
    let resource = match op {
        O::ListStreams
        | O::DescribeLimits
        | O::DescribeAccountSettings
        | O::UpdateAccountSettings => "*".to_owned(),
        O::GetRecords => text("ShardIterator")
            .and_then(crate::reading::stream_of_iterator)
            .map_or_else(
                || format!("arn:aws:kinesis:{region}:{account}:stream/*"),
                |name| stream_arn(region, account, &name),
            ),
        O::DescribeStreamConsumer | O::DeregisterStreamConsumer | O::SubscribeToShard
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
