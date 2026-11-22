//! What the console's GET addresses may call: a list, a detail, a row's columns, a related list and finding a resource by its
//! name are each made when a page is opened, so each must be a read. A write is only ever a POST or DELETE (behind the
//! same-origin proof), so no page opened from a link can change anything.

use super::ALL;
use crate::resources::{Id, Step};

/// The beginnings of an operation that reads and changes nothing.
const READS: &[&str] = &["Get", "List", "Describe", "Head", "Scan", "Query", "Lookup"];

fn reads(step: &Step) -> bool {
    READS.iter().any(|r| step.op.starts_with(r))
}

#[test]
fn every_operation_a_get_makes_is_a_read() {
    let mut seen = 0;
    for d in ALL.iter().filter_map(|e| e.descriptor) {
        for k in d.kinds {
            let mut steps: Vec<&Step> = vec![&k.list.step];
            steps.extend(k.each.iter());
            steps.extend(k.detail.iter());
            steps.extend(k.subs.iter().map(|s| &s.step));
            if let Id::Call(step, _) = &k.id_of {
                steps.push(step);
            }
            for step in steps {
                seen += 1;
                assert!(
                    reads(step),
                    "{}/{}: a GET makes {}",
                    d.service,
                    k.id,
                    step.op
                );
            }
        }
    }
    assert!(seen > 50, "only {seen} steps were looked at");
}
