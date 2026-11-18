//! Which machine a group not in memory starts as: what it was when the broker last stopped, what the request that begins
//! it asks for, and Kafka's rule that a share group and the other kinds do not become each other.

use kafka_wire::ids::GroupId;

use super::Groups;
use crate::groups::actor::Req;
use crate::groups::consumer::assign::Assignor;
use crate::groups::group::Group;

impl Groups {
    /// The machine a request that begins `group` starts it with: a group of the new protocol is as it was when the broker
    /// last stopped, if it was empty then.
    pub(super) fn machine_for(&self, group: &GroupId, first: &Req) -> Group {
        if matches!(first, Req::ShareHeartbeat(_)) {
            return self.share_machine_for(group);
        }
        if matches!(first, Req::StreamsHeartbeat(_)) {
            return self.streams_machine_for(group);
        }
        let life = crate::blocking::run(|| self.meta.group_life(group))
            .ok()
            .flatten();
        // A share group stays one, whatever asks for it; so does a streams group.
        if let Some(share) = life.as_ref().and_then(|l| l.share.as_ref()) {
            return Group::share(self.settings, Some(share));
        }
        if let Some(streams) = life.as_ref().and_then(|l| l.streams.as_ref()) {
            return Group::streams(self.settings, Some(streams));
        }
        if !matches!(first, Req::ConsumerHeartbeat(_)) {
            return Group::classic(self.settings);
        }
        match life.and_then(|l| l.consumer) {
            Some(c) => Group::restored(
                self.settings,
                c.epoch,
                c.assignment_epoch,
                Assignor::parse(&c.assignor).unwrap_or_default(),
            ),
            None => Group::consumer(self.settings),
        }
    }

    /// The machine a share heartbeat finds for a group not in memory: the share group it was when the broker last stopped,
    /// or the consumer or classic group the id was used as (which refuses it: Kafka converts neither to a share group),
    /// or a new share group.
    fn share_machine_for(&self, group: &GroupId) -> Group {
        let (life, has_offsets) = crate::blocking::run(|| {
            let life = self.meta.group_life(group).ok().flatten();
            let offsets = self.meta.group_offsets(group).is_ok_and(|o| !o.is_empty());
            (life, offsets)
        });
        match life {
            Some(l) if l.share.is_some() => Group::share(self.settings, l.share.as_ref()),
            Some(l) if l.streams.is_some() => Group::streams(self.settings, l.streams.as_ref()),
            Some(l) if l.consumer.is_some() => Group::consumer(self.settings),
            Some(_) => Group::classic(self.settings),
            None if has_offsets => Group::classic(self.settings),
            None => Group::share(self.settings, None),
        }
    }

    /// The machine a streams heartbeat finds for a group not in memory: the streams group it was when the broker last
    /// stopped; the consumer or share group the id was used as (which refuses it); or an empty classic group (offsets
    /// only, or nothing), which the join makes a streams group (Kafka converts an empty classic group).
    fn streams_machine_for(&self, group: &GroupId) -> Group {
        let life = crate::blocking::run(|| self.meta.group_life(group).ok().flatten());
        match life {
            Some(l) if l.streams.is_some() => Group::streams(self.settings, l.streams.as_ref()),
            Some(l) if l.share.is_some() => Group::share(self.settings, l.share.as_ref()),
            Some(l) if l.consumer.is_some() => Group::consumer(self.settings),
            Some(_) => Group::classic(self.settings),
            None => Group::streams(self.settings, None),
        }
    }
}
