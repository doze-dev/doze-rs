//! The requests in flight, and what is told of them while they are answered: a refusal IAM's soft mode lets through, the policy that
//! allowed a request.

use std::sync::{Arc, Mutex, PoisonError};

use super::{Notes, Seen};

/// The requests in flight, so a refusal IAM's soft mode lets through can be marked on the request it was for ([`crate::Config::would_deny`]).
///
/// IAM is told only the caller and the action, not which request asked, so a refusal is matched by the caller's key and region,
/// then by the operation (`sqs:SendMessage` to `SendMessage`). When the caller has one request in flight it is that one, whatever
/// the action is called (S3's `ListObjectsV2` asks for `s3:ListBucket`); when it has several, those whose operation is the
/// action's are marked; when none is, none is (a guess would mark a call that was allowed).
#[derive(Debug, Default)]
pub struct WouldDeny(Mutex<Vec<Arc<Seen>>>);

impl WouldDeny {
    /// Soft mode let `access_key_id`, signed for `region`, do `action` (`sqs:SendMessage`), which its policies refuse.
    pub fn let_through(&self, access_key_id: &str, region: &str, action: &str) {
        self.mark(access_key_id, region, action, |n| n.would_be_denied = true);
    }

    /// The identity policy `policy` (of `kind`) allowed `access_key_id`, signed for `region`, to do `action`: the request is marked
    /// with it. The first policy told stands (a request asks IAM once for each thing it does, and the first is the one named).
    ///
    /// Only when the request is certain: IAM is not told which request asked, so when the caller has several in flight for the action
    /// the policy cannot be told to be theirs (two resources can be allowed by two policies), and none is named.
    pub fn allowed_by(
        &self,
        access_key_id: &str,
        region: &str,
        action: &str,
        policy: &str,
        kind: &str,
    ) {
        let flight = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let [only] = Self::asking(&flight, access_key_id, region, action).as_slice() {
            let mut n = only.notes();
            if n.allowed_by.is_none() {
                n.allowed_by = Some((policy.to_owned(), kind.to_owned()));
            }
        }
    }

    /// Writes `note` on the request the caller has in flight that `action` is for (see the type's words on how it is found).
    fn mark(&self, access_key_id: &str, region: &str, action: &str, note: impl Fn(&mut Notes)) {
        let flight = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        for s in Self::asking(&flight, access_key_id, region, action) {
            note(&mut s.notes());
        }
    }

    /// The requests in `flight` that `action` of the caller may be for: those whose operation is the action's, else the caller's one
    /// request when it has exactly one, else none.
    fn asking<'a>(
        flight: &'a [Arc<Seen>],
        access_key_id: &str,
        region: &str,
        action: &str,
    ) -> Vec<&'a Arc<Seen>> {
        let operation = action.split_once(':').map_or(action, |(_, op)| op);
        let mine: Vec<&Arc<Seen>> = flight
            .iter()
            .filter(|s| {
                let n = s.notes();
                n.access_key_id == access_key_id && n.region == region
            })
            .collect();
        let named: Vec<&Arc<Seen>> = mine
            .iter()
            .copied()
            .filter(|s| s.notes().action == operation)
            .collect();
        if !named.is_empty() {
            named
        } else if mine.len() == 1 {
            mine
        } else {
            Vec::new()
        }
    }

    /// The id of the request `access_key_id`, signed for `region`, is being answered now, when it has exactly one in flight: what
    /// caused a call between services made for that caller on a task the request's own does not reach (a service's actor). With
    /// none in flight, or several, `None`: a guess could name the wrong one.
    #[must_use]
    pub fn request_of(&self, access_key_id: &str, region: &str) -> Option<String> {
        let flight = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let mut mine = flight.iter().filter(|s| {
            let n = s.notes();
            n.access_key_id == access_key_id && n.region == region
        });
        let only = mine.next()?;
        if mine.next().is_some() {
            return None;
        }
        let id = only.notes().request_id.clone();
        (!id.is_empty()).then_some(id)
    }

    pub(super) fn enter(&self, seen: &Arc<Seen>) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Arc::clone(seen));
    }

    pub(super) fn leave(&self, seen: &Arc<Seen>) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|s| !Arc::ptr_eq(s, seen));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(flight: &WouldDeny, key: &str, operation: &'static str) -> Arc<Seen> {
        let seen = Arc::new(Seen::default());
        {
            let mut n = seen.notes();
            n.access_key_id = key.to_owned();
            n.region = "us-east-1".to_owned();
            n.action = operation;
        }
        flight.enter(&seen);
        seen
    }

    fn named(seen: &Seen) -> Option<String> {
        seen.notes().allowed_by.as_ref().map(|(p, _)| p.clone())
    }

    #[test]
    fn a_policy_is_named_only_for_a_request_that_is_certain() {
        let flight = WouldDeny::default();
        // One request for the action: it is that one's.
        let one = request(&flight, "AKID", "GetObject");
        flight.allowed_by("AKID", "us-east-1", "s3:GetObject", "ReadOnly", "user");
        assert_eq!(named(&one), Some("ReadOnly".into()));
        // The same caller, two requests of the one operation, perhaps allowed by two different policies (two resources): IAM is not
        // told which asked, so neither is named.
        let a = request(&flight, "AKID2", "GetObject");
        let b = request(&flight, "AKID2", "GetObject");
        flight.allowed_by("AKID2", "us-east-1", "s3:GetObject", "ReadOnly", "user");
        assert_eq!((named(&a), named(&b)), (None, None));
        // Two requests of different operations: the one whose operation it is.
        let get = request(&flight, "AKID3", "GetObject");
        let put = request(&flight, "AKID3", "PutObject");
        flight.allowed_by("AKID3", "us-east-1", "s3:PutObject", "Writer", "user");
        assert_eq!((named(&get), named(&put)), (None, Some("Writer".into())));
        // Several in flight and none of the action's: none is named (a guess could name a call it was not for).
        flight.allowed_by("AKID3", "us-east-1", "s3:DeleteObject", "Deleter", "user");
        assert_eq!(named(&get), None);
    }
}
