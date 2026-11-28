//! What a request asks to do, for the authorizer: the IAM action (`sqs:SendMessage`), the queue's ARN, and the policy on the queue.

use aws_model_sqs::Operation;
use aws_model_sqs::tables::TABLES;
use aws_peers::Need;
use aws_peers::input::In;
use aws_wire::service::Call;

use crate::core::Core;
use crate::policy::arn;
use crate::url;

impl Core {
    /// Every thing `call` asks to do. Empty for a request that is not understood (it is refused on its own terms).
    pub(crate) fn needs(&self, call: &Call) -> Vec<Need> {
        let Some(op) = Operation::from_index(call.op) else {
            return Vec::new();
        };
        // Read from the input the gateway decoded, not by parsing the body again (a 256 KiB message, a batch of ten).
        let body = In::of(&TABLES, call);
        let text = |key: &str| body.text(key);
        let (region, account) = (&call.caller.region, &call.caller.account_id);
        let action = format!("sqs:{}", action_of(op));
        let on = |name: &str| {
            let queue = self.queues.get(name);
            let policy = queue.and_then(|q| q.stored.get("Policy").cloned());
            Need::new(action.clone(), arn(region, account, name)).with_policy(policy)
        };
        let named = match op {
            Operation::CreateQueue | Operation::GetQueueUrl => text("QueueName").map(on),
            Operation::ListQueues => Some(Need::new(action.clone(), arn(region, account, "*"))),
            Operation::StartMessageMoveTask | Operation::ListMessageMoveTasks => text("SourceArn")
                .map(|a| {
                    let name = a.rsplit(':').next().unwrap_or_default();
                    let mut need = on(name);
                    need.resource = a.to_owned();
                    need
                }),
            Operation::CancelMessageMoveTask => Some(Need::new(action.clone(), "*")),
            _ => text("QueueUrl")
                .and_then(url::parse)
                .map(|(_, name)| on(name)),
        };
        named.into_iter().collect()
    }
}

/// The IAM action an operation is authorized as: the batch forms are authorized as the single ones.
fn action_of(op: Operation) -> String {
    let name = format!("{op:?}");
    match name.strip_suffix("Batch") {
        Some(single) => single.to_owned(),
        None => name,
    }
}
