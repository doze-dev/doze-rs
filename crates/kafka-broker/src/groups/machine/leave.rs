//! LeaveGroup: members say they are going.

use kafka_wire::ResponseError;
use kafka_wire::ids::MemberId;

use super::{Answer, Effect, GroupMachine, LeaveResult, LeaveTarget, Millis, Token};

impl GroupMachine {
    pub(super) fn leave(
        &mut self,
        token: Token,
        targets: &[LeaveTarget],
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let mut results = Vec::with_capacity(targets.len());
        let mut left: Vec<MemberId> = Vec::new();
        for t in targets {
            let found = self.who_leaves(t);
            let error = match found {
                Ok(id) => {
                    left.push(id);
                    None
                }
                Err(e) => Some(e),
            };
            results.push(LeaveResult {
                member_id: t.member_id.clone(),
                instance: t.instance.clone(),
                error,
            });
        }
        for id in &left {
            self.drop_member(id, out);
        }
        out.push(Effect::Reply(token, Answer::Leave(results)));
        // Members that were not there do not disturb the group; members that were do.
        if !left.is_empty() {
            self.rebalance_after_change(now, out);
        }
    }

    /// The member a LeaveGroup entry names: by id, or by instance when it gives no id.
    fn who_leaves(&self, t: &LeaveTarget) -> Result<MemberId, ResponseError> {
        match (&t.member_id, &t.instance) {
            (Some(id), instance) => self.fenced(id, instance.as_ref()).map(|()| id.clone()),
            (None, Some(inst)) => self
                .statics
                .get(inst)
                .cloned()
                .ok_or(ResponseError::UnknownMemberId),
            (None, None) => Err(ResponseError::UnknownMemberId),
        }
    }
}
