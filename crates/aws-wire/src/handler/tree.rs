//! A handler's `serde_json::Value` answer read into the arena by the output shape, without writing it as text first.
//!
//! It holds the answer to what [`aws_schema::json::decode_lenient`] held the text to (the shape must fit: types, a union's one
//! member; sizes, patterns and required members are not checked), and builds the same tree, every string copied into the arena's
//! own text. Writing the value out and decoding it again was most of a 1 MB DynamoDB `Query` page's time.

use aws_schema::arena::{Arena, NIL, Node, Val};
use aws_schema::json::Violation;
use aws_schema::tables::{
    K_BLOB, K_BOOL, K_EVENT, K_FLOAT, K_INT, K_LIST, K_MAP, K_STR, K_STREAM, K_STRUCT, K_TS,
    K_UNION, MemberRef, Tables,
};
use serde_json::{Map, Value};

/// Deeper than any answer; as the decoder's limit.
const MAX_DEPTH: u32 = 100;

type R<X> = Result<X, Violation>;

/// `value` as the structure `shape`; returns the root node.
///
/// # Errors
/// What [`aws_schema::json::decode_lenient`] would have said of the value's text, but for `Syntax`, which a value cannot have.
pub(super) fn read(t: &Tables, shape: u32, value: &Value, ar: &mut Arena) -> R<u32> {
    let Value::Object(map) = value else {
        return Err(Violation::Syntax { at: 0 });
    };
    let v = object(t, shape, map, ar, 0)?;
    Ok(ar.push(Node {
        next: NIL,
        member: NIL,
        val: v,
    }))
}

fn type_error(mi: u32, expected: &'static str) -> Violation {
    Violation::Type {
        member: mi,
        expected,
    }
}

fn value(t: &Tables, m: &MemberRef<'_>, mi: u32, v: &Value, ar: &mut Arena, depth: u32) -> R<Val> {
    if depth > MAX_DEPTH {
        return Err(Violation::TooDeep);
    }
    if v.is_null() {
        return Ok(Val::Null);
    }
    match (m.kind, v) {
        (K_BOOL, Value::Bool(b)) => Ok(Val::Bool(*b)),
        (K_BOOL, _) => Err(type_error(mi, "boolean")),
        (K_INT, Value::Number(n)) => n
            .as_i64()
            .map(Val::Int)
            .ok_or_else(|| type_error(mi, "integer")),
        (K_INT, _) => Err(type_error(mi, "integer")),
        (K_TS, Value::String(s)) if aws_schema::time::valid(s) => Ok(ar.push_text(s)),
        (K_TS, Value::String(_)) => Err(type_error(mi, "timestamp")),
        (K_FLOAT | K_TS, Value::Number(n)) => n
            .as_f64()
            .map(Val::Float)
            .ok_or_else(|| type_error(mi, "number")),
        (K_FLOAT | K_TS, _) => Err(type_error(mi, "number")),
        (K_STR, Value::String(s)) => Ok(ar.push_text(s)),
        (K_STR, _) => Err(type_error(mi, "string")),
        (K_BLOB, Value::String(s)) => aws_core::base64::decode(s)
            .map(|b| ar.push_blob(&b))
            .ok_or_else(|| type_error(mi, "base64 string")),
        (K_BLOB, _) => Err(type_error(mi, "base64 string")),
        (K_LIST, Value::Array(items)) => list(t, m, items, ar, depth),
        (K_MAP, Value::Object(map)) => entries(t, m, map, ar, depth),
        (K_STRUCT | K_UNION, Value::Object(map)) => object(t, m.target, map, ar, depth + 1),
        // The decoder found no `[` or `{` where the shape wants one.
        (K_LIST | K_MAP | K_STRUCT | K_UNION, _) => Err(Violation::Syntax { at: 0 }),
        // A streaming member is not a JSON key: ignored, as the decoder ignores it.
        (K_STREAM | K_EVENT, _) => Ok(Val::Null),
        _ => Err(Violation::Table),
    }
}

/// Appends `ix` to the chain that runs from `first` to `last`.
fn chain(ar: &mut Arena, (first, last): &mut (u32, u32), ix: u32) {
    if *first == NIL {
        *first = ix;
    } else {
        ar.link(*last, ix);
    }
    *last = ix;
}

fn list(t: &Tables, m: &MemberRef<'_>, items: &[Value], ar: &mut Arena, depth: u32) -> R<Val> {
    let elem = t.member(m.target).ok_or(Violation::Table)?;
    let mut ends = (NIL, NIL);
    for item in items {
        let v = value(t, &elem, m.target, item, ar, depth + 1)?;
        let ix = ar.push(Node {
            next: NIL,
            member: NIL,
            val: v,
        });
        chain(ar, &mut ends, ix);
    }
    Ok(Val::Seq(ends.0))
}

fn entries(
    t: &Tables,
    m: &MemberRef<'_>,
    map: &Map<String, Value>,
    ar: &mut Arena,
    depth: u32,
) -> R<Val> {
    let val_m = t.member(m.target + 1).ok_or(Violation::Table)?;
    let mut ends = (NIL, NIL);
    for (k, v) in map {
        let key = ar.push_text(k);
        let v = value(t, &val_m, m.target + 1, v, ar, depth + 1)?;
        let kx = ar.push(Node {
            next: NIL,
            member: NIL,
            val: key,
        });
        let vx = ar.push(Node {
            next: NIL,
            member: NIL,
            val: v,
        });
        ar.link(kx, vx);
        if ends.0 == NIL {
            ends.0 = kx;
        } else {
            ar.link(ends.1, kx);
        }
        ends.1 = vx;
    }
    Ok(Val::Seq(ends.0))
}

fn object(t: &Tables, shape: u32, map: &Map<String, Value>, ar: &mut Arena, depth: u32) -> R<Val> {
    if depth > MAX_DEPTH {
        return Err(Violation::TooDeep);
    }
    let sh = t.shape(shape).ok_or(Violation::Table)?;
    let mut ends = (NIL, NIL);
    let mut members = 0_usize;
    for (key, v) in map {
        let Some((mi, m)) = (sh.first..sh.first + sh.count)
            .find_map(|i| t.member(i).filter(|m| m.wire == key).map(|m| (i, m)))
        else {
            continue;
        };
        let v = value(t, &m, mi, v, ar, depth)?;
        if matches!(v, Val::Null) {
            continue;
        }
        let ix = ar.push(Node {
            next: NIL,
            member: mi,
            val: v,
        });
        chain(ar, &mut ends, ix);
        members += 1;
    }
    if sh.union && members != 1 {
        return Err(Violation::Union { shape });
    }
    Ok(Val::Obj(ends.0))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use aws_schema::enc::encode;
    use aws_schema::json::decode_lenient;
    use aws_schema::patterns::Patterns;
    use serde_json::json;

    use super::*;

    /// The answer as the text path wrote it: the value as text, decoded leniently, detached, encoded.
    fn by_text(t: &'static Tables, shape: u32, v: &Value) -> Result<Vec<u8>, Violation> {
        let text = v.to_string();
        let mut ar = Arena::new();
        let root = decode_lenient(t, &Patterns::new(t), shape, &text, &mut ar)?;
        ar.detach(&text);
        let mut out = Vec::new();
        encode(t, shape, &ar, "", root, &mut out)?;
        Ok(out)
    }

    fn by_tree(t: &'static Tables, shape: u32, v: &Value) -> Result<Vec<u8>, Violation> {
        let mut ar = Arena::new();
        let root = read(t, shape, v, &mut ar)?;
        let mut out = Vec::new();
        encode(t, shape, &ar, "", root, &mut out)?;
        Ok(out)
    }

    fn output(t: &'static Tables, op: &str) -> u32 {
        t.op(t.find_op(op).unwrap()).unwrap().output.unwrap()
    }

    #[test]
    fn a_value_reads_into_the_same_tree_its_text_decodes_to() {
        let ddb = &aws_model_dynamodb::tables::TABLES;
        let sqs = &aws_model_sqs::tables::TABLES;
        let lambda = &aws_model_lambda::tables::TABLES;
        let cases = [
            (
                ddb,
                "Query",
                json!({"Count": 2, "ScannedCount": 2, "Items": [
                {"pk": {"S": "a\"b\\c\n"}, "n": {"N": "1.5"}, "b": {"B": "aGk="}, "l": {"L": [{"BOOL": true}, {"NULL": true}]},
                 "m": {"M": {"z": {"SS": ["x", "y"]}, "a": {"BS": ["AA=="]}}}},
                {"pk": {"S": "é→😀"}}],
                "LastEvaluatedKey": {"pk": {"S": "a"}}, "ConsumedCapacity": {"TableName": "t", "CapacityUnits": 3}, "Unknown": 1}),
            ),
            (
                sqs,
                "ReceiveMessage",
                json!({"Messages": [{"MessageId": "m", "Body": "b", "Attributes": {"SentTimestamp": "1"},
                "MessageAttributes": {"bin": {"DataType": "Binary", "BinaryValue": "AAEC"}, "s": {"DataType": "String", "StringValue": "v"}}}]}),
            ),
            (
                lambda,
                "GetFunction",
                json!({"Configuration": {"FunctionName": "f", "MemorySize": 128, "Timeout": 3,
                "LastModified": "2026-11-27T00:00:00.000+0000", "Environment": {"Variables": {"A": "1"}}, "Layers": [], "Description": null},
                "Tags": {"k": "v"}}),
            ),
        ];
        for (t, op, v) in &cases {
            let shape = output(t, op);
            let text = by_text(t, shape, v).unwrap();
            assert_eq!(by_tree(t, shape, v).unwrap(), text, "{op}");
        }
        // What the text path refused, this refuses the same way.
        let shape = output(ddb, "Query");
        for bad in [
            json!({"Count": "2"}),
            json!({"Count": 1.5}),
            json!({"Items": {}}),
            json!({"Items": [{"pk": {"S": "a", "N": "1"}}]}),
            json!({"Items": [{"pk": {"B": "not base64!"}}]}),
        ] {
            // A value has no text, so a syntax error has no place in it.
            let placeless = |v: Violation| match v {
                Violation::Syntax { .. } => Violation::Syntax { at: 0 },
                v => v,
            };
            assert_eq!(
                by_tree(ddb, shape, &bad).unwrap_err(),
                placeless(by_text(ddb, shape, &bad).unwrap_err()),
                "{bad}"
            );
        }
    }
}
