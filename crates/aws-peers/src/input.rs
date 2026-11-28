//! A call's decoded input, read where the gateway left it: the arena and the body its strings point into. It answers what a
//! service's `authz::needs` asks of a request (a member's text, a list's items, a map's keys) without parsing the body again, which
//! a `serde_json::Value` of the body did for every authorized call (a 6 MB Lambda payload, a 5 MB Kinesis batch).
//!
//! It reads like the part of `serde_json::Value` those functions used: [`In::get`], [`In::as_str`], [`In::as_array`],
//! [`In::keys`] and [`In::pointer`], with a structure's members found by the name JSON gives them (the model's `wire` name).

use aws_schema::arena::{Arena, NIL, Val};
use aws_schema::tables::{K_LIST, K_MAP, Tables};
use aws_wire::service::Call;

/// One value of a decoded input.
#[derive(Clone, Copy)]
pub struct In<'a> {
    tables: &'static Tables,
    arena: &'a Arena,
    body: &'a str,
    val: Val,
    /// The kind of value it is (`K_LIST`, `K_MAP`, a structure's...), and the member that says what its parts are.
    kind: u8,
    target: u32,
}

impl std::fmt::Debug for In<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("In")
            .field("val", &self.val)
            .finish_non_exhaustive()
    }
}

impl<'a> In<'a> {
    /// The input of `call`, whose model is `tables`. A call with no input reads as an empty structure.
    #[must_use]
    pub fn of(tables: &'static Tables, call: &'a Call) -> Self {
        let val = call
            .arena
            .node(call.root)
            .filter(|_| call.root != NIL)
            .map_or(Val::Null, |n| n.val);
        Self {
            tables,
            arena: &call.arena,
            body: &call.body,
            val,
            kind: aws_schema::tables::K_STRUCT,
            target: NIL,
        }
    }

    /// The value of member `m`.
    fn at(&self, val: Val, member: u32) -> Self {
        let (kind, target) = self
            .tables
            .member(member)
            .map_or((u8::MAX, NIL), |m| (m.kind, m.target));
        Self {
            val,
            kind,
            target,
            ..*self
        }
    }

    /// A structure's member by its JSON name, or a map's value by its key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<Self> {
        match (self.kind, self.val) {
            (K_MAP, _) => self.pairs().find(|(k, _)| *k == key).map(|(_, v)| v),
            (_, Val::Obj(first)) => {
                let mut at = first;
                while at != NIL {
                    let n = self.arena.node(at)?;
                    if self.tables.member(n.member).is_some_and(|m| m.wire == key) {
                        return Some(self.at(n.val, n.member));
                    }
                    at = n.next;
                }
                None
            }
            _ => None,
        }
    }

    /// The text of a string (or a timestamp written as one).
    #[must_use]
    pub fn as_str(&self) -> Option<&'a str> {
        self.arena.str_of(self.body, self.val)
    }

    /// The bytes of a blob, already decoded from base64 by the gateway.
    #[must_use]
    pub fn as_bytes(&self) -> Option<&'a [u8]> {
        self.arena.bytes_of(self.val)
    }

    /// An integer.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self.val {
            Val::Int(n) => Some(n),
            _ => None,
        }
    }

    /// The text of string member `key`.
    #[must_use]
    pub fn text(&self, key: &str) -> Option<&'a str> {
        self.get(key)?.as_str()
    }

    /// A list's items.
    #[must_use]
    pub fn as_array(&self) -> Option<Vec<Self>> {
        let (K_LIST, Val::Seq(first)) = (self.kind, self.val) else {
            return None;
        };
        let mut items = Vec::new();
        let mut at = first;
        while let Some(n) = self.arena.node(at).filter(|_| at != NIL) {
            items.push(self.at(n.val, self.target));
            at = n.next;
        }
        Some(items)
    }

    /// Whether it is a list with at least one item.
    #[must_use]
    pub fn is_nonempty_list(&self) -> bool {
        matches!((self.kind, self.val), (K_LIST, Val::Seq(first)) if first != NIL)
    }

    /// A map's keys and values, in the order sent.
    pub fn pairs(self) -> impl Iterator<Item = (&'a str, Self)> + 'a {
        let first = match (self.kind, self.val) {
            (K_MAP, Val::Seq(first)) => first,
            _ => NIL,
        };
        let mut at = first;
        std::iter::from_fn(move || {
            let k = self.arena.node(at).filter(|_| at != NIL)?;
            let v = self.arena.node(k.next)?;
            at = v.next;
            let key = self.arena.str_of(self.body, k.val)?;
            Some((key, self.at(v.val, self.target.wrapping_add(1))))
        })
    }

    /// A map's keys.
    pub fn keys(self) -> impl Iterator<Item = &'a str> + 'a {
        self.pairs().map(|(k, _)| k)
    }

    /// The value a JSON pointer names (`/routingConfiguration/0/stateMachineVersionArn`).
    #[must_use]
    pub fn pointer(&self, pointer: &str) -> Option<Self> {
        let mut here = *self;
        for part in pointer.split('/').skip(1) {
            here = match here.kind {
                K_LIST => here.as_array()?.into_iter().nth(part.parse().ok()?)?,
                _ => here.get(part)?,
            };
        }
        Some(here)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use aws_schema::json::decode;
    use aws_schema::patterns::Patterns;
    use aws_wire::RequestId;
    use aws_wire::service::Caller;

    use super::*;

    fn call(tables: &'static Tables, op: &str, body: &str) -> Call {
        let op = tables.find_op(op).unwrap();
        let shape = tables.op(op).unwrap().input.unwrap();
        let mut arena = Arena::new();
        let root = decode(tables, &Patterns::new(tables), shape, body, &mut arena).unwrap();
        Call {
            op,
            id: RequestId::new(),
            caller: Caller {
                access_key_id: "test".into(),
                account_id: "000000000000".into(),
                region: "us-east-1".into(),
                endpoint: String::new(),
                service: None,
                session_token: None,
            },
            body: body.to_owned(),
            arena,
            root,
        }
    }

    #[test]
    fn members_lists_maps_and_pointers_read_as_the_body_says() {
        let tables = &aws_model_sfn::tables::TABLES;
        let c = call(
            tables,
            "CreateStateMachineAlias",
            r#"{"name":"live","routingConfiguration":[{"stateMachineVersionArn":"arn:sm:m:1","weight":100}]}"#,
        );
        let input = In::of(tables, &c);
        assert_eq!(input.get("name").and_then(|v| v.as_str()), Some("live"));
        assert!(input.get("description").is_none());
        assert!(
            input
                .get("routingConfiguration")
                .unwrap()
                .is_nonempty_list()
        );
        assert_eq!(
            input
                .pointer("/routingConfiguration/0/stateMachineVersionArn")
                .and_then(|v| v.as_str()),
            Some("arn:sm:m:1")
        );

        let tables = &aws_model_dynamodb::tables::TABLES;
        let c = call(
            tables,
            "BatchGetItem",
            r#"{"RequestItems":{"orders":{"Keys":[{"id":{"S":"a"}}]},"carts":{"Keys":[{"id":{"S":"b"}}],"ConsistentRead":true}}}"#,
        );
        let items = In::of(tables, &c).get("RequestItems").unwrap();
        assert_eq!(items.keys().collect::<Vec<_>>(), ["orders", "carts"]);
        let keys = items.get("orders").unwrap().get("Keys").unwrap();
        assert_eq!(keys.as_array().unwrap().len(), 1);
        // Only a list is a list.
        assert!(
            !items
                .get("carts")
                .unwrap()
                .get("ConsistentRead")
                .unwrap()
                .is_nonempty_list()
        );
        // An escaped string reads decoded.
        let c = call(tables, "DescribeTable", r#"{"TableName":"abc"}"#);
        assert_eq!(
            In::of(tables, &c).get("TableName").and_then(|v| v.as_str()),
            Some("abc")
        );
    }
}
