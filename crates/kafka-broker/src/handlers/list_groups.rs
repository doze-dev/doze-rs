//! ListGroups.

use std::collections::BTreeMap;

use kafka_storage::fs::Fs;
use kafka_wire::StrBytes;
use kafka_wire::messages::list_groups_response::ListedGroup;
use kafka_wire::messages::{GroupId as WireGroupId, ListGroupsRequest, ListGroupsResponse};

use super::describe_groups::state_name;
use crate::{Broker, Outcome, blocking};

impl<F: Fs> Broker<F> {
    /// Every group there is: those in memory, and those known only by their committed offsets (which outlive the
    /// members, and a restart), by id. The state and type filters (v4, v5) match without regard to case, as Kafka's
    /// do.
    pub(crate) async fn list_groups(&self, req: &ListGroupsRequest) -> Outcome {
        let mut resp = ListGroupsResponse::default();
        // (protocol type, state)
        let mut all: BTreeMap<String, (String, &'static str)> = BTreeMap::new();
        match blocking::run(|| self.meta.groups_with_offsets()) {
            Ok(ids) => {
                for id in ids {
                    all.insert(id.to_string(), (String::new(), "Empty"));
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "listing groups: committed offsets could not be read")
            }
        }
        for (id, d) in self.groups.list().await {
            all.insert(
                id.to_string(),
                (d.protocol_type.unwrap_or_default(), state_name(d.state)),
            );
        }
        let wanted = |filter: &[StrBytes], value: &str| {
            filter.is_empty()
                || filter
                    .iter()
                    .any(|f| f.as_str().eq_ignore_ascii_case(value))
        };
        for (id, (protocol_type, state)) in all {
            if !wanted(&req.states_filter, state) || !wanted(&req.types_filter, "classic") {
                continue;
            }
            let mut g = ListedGroup::default();
            g.group_id = WireGroupId(StrBytes::from_string(id));
            g.protocol_type = StrBytes::from_string(protocol_type);
            g.group_state = StrBytes::from_static_str(state);
            g.group_type = StrBytes::from_static_str("classic");
            resp.groups.push(g);
        }
        Outcome::Respond(kafka_wire::api::Response::ListGroups(resp))
    }
}
