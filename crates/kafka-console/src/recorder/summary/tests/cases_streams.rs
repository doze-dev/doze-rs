//! The streams group APIs' cases (KIP-1071): a member's join with a word count's topology and the tasks it was given, a
//! describe, and the topology description update a broker without the plugin refuses.

use kafka_wire::api::{Request, Response};
use kafka_wire::messages::*;

use super::cases::{group, named, s, topic};

fn tasks(sub: &str, partitions: &[i32]) -> streams_group_heartbeat_response::TaskIds {
    named(|t: &mut streams_group_heartbeat_response::TaskIds| {
        t.subtopology_id = s(sub);
        t.partitions = partitions.to_vec();
    })
}

pub(super) fn streams(out: &mut Vec<(ApiKey, Request, Response)>) {
    macro_rules! case {
        ($key:ident, $req:expr, $resp:expr) => {
            out.push((
                ApiKey::$key,
                Request::$key($req.into()),
                Response::$key($resp.into()),
            ));
        };
    }
    case!(
        StreamsGroupHeartbeat,
        named(|r: &mut StreamsGroupHeartbeatRequest| {
            r.group_id = group("counts");
            r.member_id = s("m");
            r.topology = Some(named(
                |t: &mut streams_group_heartbeat_request::Topology| {
                    t.subtopologies = vec![named(
                        |sub: &mut streams_group_heartbeat_request::Subtopology| {
                            sub.subtopology_id = s("0");
                            sub.source_topics = vec![topic("words")];
                            sub.repartition_sink_topics = vec![topic("counts-words-repartition")];
                        },
                    )];
                },
            ));
        }),
        named(|r: &mut StreamsGroupHeartbeatResponse| {
            r.member_epoch = 3;
            r.active_tasks = Some(vec![tasks("0", &[0, 1, 2]), tasks("1", &[0])]);
            r.standby_tasks = Some(Vec::new());
            r.warmup_tasks = Some(Vec::new());
        })
    );
    case!(
        StreamsGroupDescribe,
        named(|r: &mut StreamsGroupDescribeRequest| r.group_ids = vec![group("counts")]),
        StreamsGroupDescribeResponse::default()
    );
    case!(
        StreamsGroupTopologyDescriptionUpdate,
        named(|r: &mut StreamsGroupTopologyDescriptionUpdateRequest| {
            r.group_id = group("counts");
            r.member_id = s("m");
        }),
        named(|r: &mut StreamsGroupTopologyDescriptionUpdateResponse| {
            r.error_code = 35;
            r.error_message = Some(s(
                "The broker has no streams group topology description plugin configured.",
            ));
        })
    );
}
