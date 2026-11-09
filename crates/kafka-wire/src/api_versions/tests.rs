use super::*;
use crate::profile::KafkaMajor;

#[test]
fn an_unreadable_body_at_a_served_version_gets_the_list() {
    let (version, resp) = answer(
        Profile::new(KafkaMajor::DEFAULT),
        Features::default(),
        4,
        None,
    );
    assert_eq!((version, resp.error_code), (4, 0));
    assert!(!resp.api_keys.is_empty());
}
