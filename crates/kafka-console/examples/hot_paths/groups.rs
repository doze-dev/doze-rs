//! The `groups` mode: classic members heartbeating their own groups, alone and while another client deletes groups.

use super::*;

async fn join(c: &mut Conn, group: &str) -> (StrBytes, i32) {
    let mut join = JoinGroupRequest::default();
    join.group_id = GroupId(StrBytes::from_string(group.to_owned()));
    join.session_timeout_ms = 30_000;
    join.rebalance_timeout_ms = 30_000;
    join.protocol_type = StrBytes::from_static_str("consumer");
    let mut p = JoinGroupRequestProtocol::default();
    p.name = StrBytes::from_static_str("range");
    join.protocols.push(p);
    let joined: JoinGroupResponse = c.call(ApiKey::JoinGroup, 3, &join).await;
    assert_eq!(joined.error_code, 0, "join {group}");
    let mut sync = SyncGroupRequest::default();
    sync.group_id = join.group_id.clone();
    sync.generation_id = joined.generation_id;
    sync.member_id = joined.member_id.clone();
    let synced: SyncGroupResponse = c.call(ApiKey::SyncGroup, 3, &sync).await;
    assert_eq!(synced.error_code, 0);
    (joined.member_id, joined.generation_id)
}

const MEMBERS: usize = 8;

/// Heartbeats per second across `MEMBERS` connections, each its own group, for `for_how_long`.
async fn heartbeats(server: &Running, tag: &str, for_how_long: Duration) -> f64 {
    let count = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let mut tasks = Vec::new();
    for i in 0..MEMBERS {
        let mut c = Conn::open(server).await;
        let group = format!("{tag}-{i}");
        let (member, generation) = join(&mut c, &group).await;
        let (count, stop) = (Arc::clone(&count), Arc::clone(&stop));
        tasks.push(tokio::spawn(async move {
            let mut hb = HeartbeatRequest::default();
            hb.group_id = GroupId(StrBytes::from_string(group));
            hb.generation_id = generation;
            hb.member_id = member;
            while !stop.load(Ordering::Relaxed) {
                let r: HeartbeatResponse = c.call(ApiKey::Heartbeat, 3, &hb).await;
                assert_eq!(r.error_code, 0);
                count.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }
    let began = Instant::now();
    tokio::time::sleep(for_how_long).await;
    stop.store(true, Ordering::Relaxed);
    let n = count.load(Ordering::Relaxed);
    let took = began.elapsed().as_secs_f64();
    for t in tasks {
        t.await.unwrap();
    }
    n as f64 / took
}

pub(super) async fn groups() {
    let mut alone = Measured::new();
    let mut deleting = Measured::new();
    let mut deletes = Measured::new();
    for round in 0..=ROUNDS {
        let server = broker().await;
        let a = heartbeats(&server, "alone", Duration::from_millis(500)).await;
        // Another client deletes groups nobody uses, as fast as it is answered.
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicU64::new(0));
        let mut c = Conn::open(&server).await;
        let deleter = {
            let (stop, done) = (Arc::clone(&stop), Arc::clone(&done));
            tokio::spawn(async move {
                let mut del = DeleteGroupsRequest::default();
                del.groups_names = (0..4)
                    .map(|i| GroupId(StrBytes::from_string(format!("gone-{i}"))))
                    .collect();
                while !stop.load(Ordering::Relaxed) {
                    let _: DeleteGroupsResponse = c.call(ApiKey::DeleteGroups, 2, &del).await;
                    done.fetch_add(1, Ordering::Relaxed);
                }
            })
        };
        let began = Instant::now();
        let d = heartbeats(&server, "busy", Duration::from_millis(500)).await;
        stop.store(true, Ordering::Relaxed);
        let deleted = done.load(Ordering::Relaxed) as f64 / began.elapsed().as_secs_f64();
        deleter.await.unwrap();
        if round > 0 {
            for (m, rate) in [(&mut alone, a), (&mut deleting, d), (&mut deletes, deleted)] {
                m.rates.push(rate);
            }
        }
    }
    summary(&format!("heartbeats, {MEMBERS} groups"), "hb", &alone);
    summary(
        &format!("heartbeats, {MEMBERS} groups, deletes running"),
        "hb",
        &deleting,
    );
    summary("the deletes meanwhile (4 groups each)", "req", &deletes);
}
