//! The member ids handed to clients that have not come back with them: how many one group may have, and how long one lasts.

use super::*;

#[tokio::test(start_paused = true)]
async fn one_group_cannot_use_up_the_room_of_the_others() {
    let groups = groups();
    let hog = group("hog");
    for _ in 0..1_000 {
        let told = joined(groups.ask(&hog, join(None, None)).await);
        assert_eq!(told.error, Some(ResponseError::MemberIdRequired));
    }
    // It is full, and the answer is one a client tries again on.
    let full = joined(groups.ask(&hog, join(None, None)).await);
    assert_eq!(full.error, Some(ResponseError::CoordinatorNotAvailable));
    // Another group is still given its ids.
    let other = joined(groups.ask(&group("other"), join(None, None)).await);
    assert_eq!(other.error, Some(ResponseError::MemberIdRequired));
    // One that comes back with its id leaves room for another.
    let id = {
        let issued = groups.issued();
        issued
            .ids
            .keys()
            .find(|(g, _)| g.as_str() == "hog")
            .map(|(_, id)| id.clone())
            .unwrap()
    };
    let mut back = join(Some(&id), None);
    if let Req::Join(j) = &mut back {
        j.id_first = false;
    }
    assert!(joined(groups.ask(&hog, back).await).error.is_none());
    let room = joined(groups.ask(&hog, join(None, None)).await);
    assert_eq!(room.error, Some(ResponseError::MemberIdRequired));
}

#[tokio::test(start_paused = true)]
async fn an_id_is_good_for_the_session_timeout_of_the_join_that_was_given_it() {
    let groups = groups();
    let g = group("g");
    // The join says its session is 10 seconds (the test's join does): the id lapses after that, not after five minutes.
    let told = joined(groups.ask(&g, join(None, None)).await);
    let id = told.member_id.unwrap();
    assert!(groups.is_issued(&g, &id));
    tokio::time::sleep(Duration::from_secs(9)).await;
    assert!(groups.is_issued(&g, &id));
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(!groups.is_issued(&g, &id), "past the 10 seconds");
}
