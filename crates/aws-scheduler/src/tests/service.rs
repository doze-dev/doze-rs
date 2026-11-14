use std::sync::Arc;

use aws_peers::Peers;
use aws_testkit::{Harness, TempDir};
use aws_wire::service::Service;
use doze_runtime::ManualClock;
use serde_json::json;

use super::{ACCOUNT, START, caller, ctx, schedule};
use crate::Scheduler;
use crate::core::Core;
use crate::deliver::{Outcome, attempt};
use crate::fire::Invocation;

#[test]
fn schedules_and_their_next_fire_survive_a_restart() {
    let dir = TempDir::new("scheduler-persist");
    let clock = Arc::new(ManualClock::at(START));
    let h = Harness::new(Scheduler::new(ctx(&std::env::temp_dir(), None)));
    let mut core = Core::open_in(dir.path(), clock.clone(), None).unwrap();
    let call = h
        .call(
            h.op("CreateSchedule"),
            &schedule(
                "p",
                "cron(0 12 * * ? *)",
                "arn:aws:sqs:us-east-1:000000000000:q",
            ),
        )
        .unwrap();
    core.ensure_default(ACCOUNT);
    core.dispatch(&call).unwrap();
    if let Some(saved) = core.flush().unwrap() {
        saved.wait().unwrap();
    }
    let before = core.next_due();
    core.db.take().unwrap().close();
    let again = Core::open_in(dir.path(), clock, None).unwrap();
    assert_eq!(again.next_due(), before);
    assert_eq!(before, Some(START + 12 * 3_600_000));
    assert_eq!(again.groups.len(), 1, "the default group was kept");
}

#[tokio::test]
async fn a_region_holds_a_timer_only_while_something_is_scheduled() {
    let dir = TempDir::new("scheduler-timer");
    let s = Scheduler::new(ctx(dir.path(), None));
    let h = Harness::new(Scheduler::new(ctx(&std::env::temp_dir(), None)));
    let req = schedule(
        "t",
        "cron(0 12 * * ? 2099)",
        "arn:aws:sqs:us-east-1:000000000000:q",
    );
    s.call(h.call(h.op("CreateSchedule"), &req).unwrap())
        .await
        .unwrap();
    assert!(s.timer_armed("us-east-1"));
    s.call(
        h.call(h.op("DeleteSchedule"), &json!({"Name": "t"}))
            .unwrap(),
    )
    .await
    .unwrap();
    for _ in 0..50 {
        if !s.timer_armed("us-east-1") {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the timer outlived the last schedule");
}

/// IAM enforcing, with SQS and a queue: the invocation is made as the role, and the role must exist, trust Scheduler and be allowed.
#[test]
fn under_enforce_the_role_decides() {
    let dir = TempDir::new("scheduler-enforce");
    let peers = Peers::new();
    let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
    let (iam, sts) = aws_iam::services(ctx(&dir.path().join("iam"), Some(ext.clone())));
    let iam = Arc::new(iam);
    peers.register(iam.clone());
    peers.register(Arc::new(sts));
    peers.set_authorizer(iam);
    peers.register(Arc::new(aws_sqs::Sqs::new(ctx(
        &dir.path().join("sqs"),
        Some(ext),
    ))));
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let admin = caller();
    let call = |op: &str, body: serde_json::Value| {
        rt.block_on(peers.call(
            "test",
            &admin,
            op.split_once(':').unwrap().0,
            op.split_once(':').unwrap().1,
            &body,
        ))
    };
    call("sqs:CreateQueue", json!({"QueueName": "q"})).unwrap();
    peers.set_mode(aws_peers::Mode::Enforce);
    let sh = Arc::new(crate::Shared {
        peers: Some(peers.clone()),
        clock: Arc::new(ManualClock::at(START)),
        tasks: doze_runtime::Tasks::new(),
        sessions: crate::role::Sessions::default(),
    });
    let inv = Invocation {
        schedule_arn: "arn:aws:scheduler:us-east-1:000000000000:schedule/default/s".into(),
        account: ACCOUNT.into(),
        region: "us-east-1".into(),
        target: json!({"Arn": "arn:aws:sqs:us-east-1:000000000000:q", "RoleArn": super::ROLE, "Input": "x"}),
        scheduled_ms: START,
        execution_id: "0123456789abcdef".into(),
    };
    let missing = rt.block_on(attempt(&sh, &inv, 1));
    assert!(
        matches!(&missing, Outcome::Failed(f) if f.code == "AccessDeniedException"),
        "{missing:?}"
    );
    let trust = json!({"Statement": [{"Effect": "Allow", "Principal": {"Service": "scheduler.amazonaws.com"}, "Action": "sts:AssumeRole"}]});
    call(
        "iam:CreateRole",
        json!({"RoleName": "scheduler", "AssumeRolePolicyDocument": trust.to_string()}),
    )
    .unwrap();
    let denied = rt.block_on(attempt(&sh, &inv, 1));
    assert!(
        matches!(&denied, Outcome::Failed(f) if !f.retryable),
        "a role without sqs:SendMessage: {denied:?}"
    );
    let allow =
        json!({"Statement": [{"Effect": "Allow", "Action": "sqs:SendMessage", "Resource": "*"}]});
    call(
        "iam:PutRolePolicy",
        json!({"RoleName": "scheduler", "PolicyName": "send", "PolicyDocument": allow.to_string()}),
    )
    .unwrap();
    let sent = rt.block_on(attempt(&sh, &inv, 1));
    assert!(matches!(sent, Outcome::Sent), "{sent:?}");
}
