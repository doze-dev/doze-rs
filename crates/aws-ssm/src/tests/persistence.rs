//! State on disk: after every call of a broad scenario the service is stopped and started again, and what it loads is exactly
//! what it had. A change that is made and not written shows up here as a difference.

use serde_json::{Value, json};

use super::{World, temp};
use crate::core::Core;

struct Run<'a> {
    w: World,
    dir: &'a std::path::Path,
}

impl<'a> Run<'a> {
    fn new(dir: &'a std::path::Path) -> Self {
        let clock = std::sync::Arc::new(doze_runtime::ManualClock::at(1_700_000_000_000));
        let bus = super::Bus::new();
        let core = Core::open_in(dir, clock.clone(), Some(bus.peers.clone())).unwrap();
        Self {
            w: World::with(core, clock, bus),
            dir,
        }
    }

    fn state(&self) -> String {
        format!("{:?}", self.w.core.params)
    }

    /// Runs `op`, then proves a restart changes nothing.
    fn step(&mut self, op: &str, request: &Value) -> Value {
        let answer = self.w.ok(op, request);
        let before = self.state();
        if let Some(db) = self.w.core.db.take() {
            db.close();
        }
        self.w.core = Core::open_in(
            self.dir,
            self.w.clock.clone(),
            Some(self.w.bus.peers.clone()),
        )
        .unwrap();
        assert_eq!(before, self.state(), "after {op} {request}");
        answer
    }
}

#[test]
fn a_restart_changes_nothing_whatever_was_done_before_it() {
    let dir = temp();
    let mut r = Run::new(dir.path());
    r.step("PutParameter", &json!({"Name": "/s/a", "Value": "one", "Type": "String", "Description": "d", "Tags": [{"Key": "k", "Value": "v"}]}));
    r.step(
        "PutParameter",
        &json!({"Name": "/s/a", "Value": "two", "Overwrite": true}),
    );
    r.step(
        "LabelParameterVersion",
        &json!({"Name": "/s/a", "ParameterVersion": 1, "Labels": ["stable"]}),
    );
    r.step(
        "UnlabelParameterVersion",
        &json!({"Name": "/s/a", "ParameterVersion": 1, "Labels": ["stable"]}),
    );
    r.step(
        "LabelParameterVersion",
        &json!({"Name": "/s/a", "Labels": ["latest"]}),
    );
    r.step("AddTagsToResource", &json!({"ResourceType": "Parameter", "ResourceId": "/s/a", "Tags": [{"Key": "t2", "Value": "x"}]}));
    r.step(
        "RemoveTagsFromResource",
        &json!({"ResourceType": "Parameter", "ResourceId": "/s/a", "TagKeys": ["k"]}),
    );
    r.step(
        "PutParameter",
        &json!({"Name": "/s/secret", "Value": "hunter2", "Type": "SecureString"}),
    );
    r.step("PutParameter", &json!({"Name": "/s/adv", "Value": "v", "Type": "String", "Tier": "Advanced",
        "Policies": r#"[{"Type":"Expiration","Version":"1.0","Attributes":{"Timestamp":"2030-01-01T00:00:00.000Z"}}]"#}));
    // Reads change nothing and a read after a restart still opens the sealed value with the key the first run made.
    r.step("GetParameter", &json!({"Name": "/s/secret"}));
    assert_eq!(
        r.w.ok(
            "GetParameter",
            &json!({"Name": "/s/secret", "WithDecryption": true})
        )["Parameter"]["Value"],
        "hunter2"
    );
    r.step("DeleteParameter", &json!({"Name": "/s/a"}));
    r.step(
        "DeleteParameters",
        &json!({"Names": ["/s/adv", "/s/nothing"]}),
    );
    assert_eq!(
        r.w.ok("DescribeParameters", &json!({}))["Parameters"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_call_writes_the_rows_it_changed_and_not_the_others() {
    let dir = temp();
    let mut r = Run::new(dir.path());
    for n in 0..20 {
        r.w.put(&format!("/many/{n}"), "v");
    }
    let put = json!({"Name": "/many/7", "Value": "x", "Type": "String", "Overwrite": true});
    assert_eq!(
        r.w.rows_of("PutParameter", &put),
        1,
        "one parameter changed, not twenty"
    );
    assert_eq!(
        r.w.rows_of("GetParameter", &json!({"Name": "/many/7"})),
        0,
        "a read changes nothing"
    );
    let label = json!({"Name": "/many/8", "Labels": ["x"]});
    assert_eq!(r.w.rows_of("LabelParameterVersion", &label), 1);
    assert_eq!(
        r.w.rows_of("DeleteParameter", &json!({"Name": "/many/9"})),
        1
    );
    assert_eq!(r.w.rows_of("DescribeParameters", &json!({})), 0);
}
