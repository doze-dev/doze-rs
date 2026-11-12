use super::*;

#[tokio::test]
async fn a_request_outside_the_layer_wants_the_whole_document() {
    assert!(!wanted());
}

#[tokio::test]
async fn scoped_says_what_the_request_in_flight_wants() {
    assert!(scoped(true, async { wanted() }).await);
    assert!(!scoped(false, async { wanted() }).await);
}
