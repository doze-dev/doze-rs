//! PartiQL, run on a core: each statement does what the classic operation it stands for does.

use serde_json::{Value, json};

use super::{World, message};

/// The request: the model refuses an empty `Parameters`, so none is sent when there are none.
fn request(statement: &str, params: Value) -> Value {
    let mut req = json!({"Statement": statement});
    if params.as_array().is_some_and(|p| !p.is_empty()) {
        req["Parameters"] = params;
    }
    req
}

fn run(w: &mut World, statement: &str, params: Value) -> Value {
    w.ok("ExecuteStatement", request(statement, params))
}

fn fail(w: &mut World, statement: &str, params: Value) -> aws_wire::ApiError {
    w.err("ExecuteStatement", request(statement, params))
}

fn world() -> World {
    let mut w = World::new();
    w.table("music", true);
    w
}

#[test]
fn insert_then_select_by_key_and_by_other_attributes() {
    let mut w = world();
    run(
        &mut w,
        r#"INSERT INTO "music" VALUE {'pk': ?, 'sk': 1, 'title': 'Hi', 'tags': <<'a', 'b'>>, 'n': 5}"#,
        json!([{"S": "acme"}]),
    );
    run(
        &mut w,
        "INSERT INTO music VALUE {'pk': 'acme', 'sk': 2, 'title': 'Ho', 'n': 9}",
        json!([]),
    );
    let got = run(
        &mut w,
        "SELECT * FROM music WHERE pk = ? AND sk = ?",
        json!([{"S": "acme"}, {"N": "1"}]),
    );
    assert_eq!(got["Items"][0]["title"], json!({"S": "Hi"}));
    assert_eq!(got["Items"][0]["tags"], json!({"SS": ["a", "b"]}));
    let none = run(
        &mut w,
        "SELECT * FROM music WHERE pk = 'x' AND sk = 1",
        json!([]),
    );
    assert_eq!(none["Items"], json!([]));

    let some = run(
        &mut w,
        "SELECT title FROM music WHERE n > ? AND begins_with(title, 'H')",
        json!([{"N": "6"}]),
    );
    assert_eq!(some["Items"], json!([{"title": {"S": "Ho"}}]));
    let both = run(
        &mut w,
        "SELECT pk, sk FROM music WHERE pk = 'acme'",
        json!([]),
    );
    assert_eq!(both["Items"].as_array().unwrap().len(), 2);
    let listed = run(
        &mut w,
        r#"SELECT * FROM "music" WHERE sk IN [1, 3] OR title IS MISSING"#,
        json!([]),
    );
    assert_eq!(listed["Items"].as_array().unwrap().len(), 1);

    let dup = fail(
        &mut w,
        "INSERT INTO music VALUE {'pk': 'acme', 'sk': 1}",
        json!([]),
    );
    assert_eq!(
        (dup.code.as_ref(), message(&dup)),
        (
            "DuplicateItemException",
            "Duplicate primary key exists in table"
        )
    );
}

#[test]
fn a_select_pages_with_limit_and_next_token() {
    let mut w = world();
    for sk in 0..5 {
        w.put(
            "music",
            json!({"pk": {"S": "a"}, "sk": {"N": sk.to_string()}}),
        );
    }
    let mut seen = 0;
    let mut token: Option<String> = None;
    for _ in 0..10 {
        let mut req = json!({"Statement": "SELECT * FROM music", "Limit": 2});
        if let Some(t) = &token {
            req["NextToken"] = json!(t);
        }
        let page = w.ok("ExecuteStatement", req);
        seen += page["Items"].as_array().unwrap().len();
        match page["NextToken"].as_str() {
            Some(t) => token = Some(t.to_owned()),
            None => break,
        }
    }
    assert_eq!(seen, 5);
    let e = w.err(
        "ExecuteStatement",
        json!({"Statement": "SELECT * FROM music", "NextToken": "bm90IGpzb24"}),
    );
    assert_eq!(e.code, "ValidationException");
}

#[test]
fn update_and_delete_need_the_whole_key_and_an_update_makes_nothing() {
    let mut w = world();
    w.put(
        "music",
        json!({"pk": {"S": "a"}, "sk": {"N": "1"}, "plays": {"N": "1"}, "old": {"S": "x"}, "l": {"L": []}}),
    );
    let out = run(
        &mut w,
        "UPDATE music SET plays = plays + ? SET l = list_append(l, [1]) REMOVE old WHERE pk = 'a' AND sk = 1 RETURNING ALL NEW *",
        json!([{"N": "2"}]),
    );
    let item = &out["Items"][0];
    assert_eq!(
        (item["plays"].clone(), item["l"].clone(), item.get("old")),
        (json!({"N": "3"}), json!({"L": [{"N": "1"}]}), None)
    );
    let e = fail(
        &mut w,
        "UPDATE music SET plays = 1 WHERE pk = 'a'",
        json!([]),
    );
    assert_eq!(
        message(&e),
        "Where clause does not contain a mandatory equality on all key attributes"
    );
    let e = fail(
        &mut w,
        "UPDATE music SET plays = 1 WHERE pk = 'nobody' AND sk = 1",
        json!([]),
    );
    assert_eq!(e.code, "ConditionalCheckFailedException");
    let e = fail(
        &mut w,
        "UPDATE music SET plays = 1 WHERE pk = 'a' AND sk = 1 AND plays > 10",
        json!([]),
    );
    assert_eq!(e.code, "ConditionalCheckFailedException");

    let e = fail(
        &mut w,
        "DELETE FROM music WHERE pk = 'a' AND sk = 1 AND plays = 0",
        json!([]),
    );
    assert_eq!(e.code, "ConditionalCheckFailedException");
    let gone = run(
        &mut w,
        "DELETE FROM music WHERE pk = 'a' AND sk = 1 RETURNING ALL OLD *",
        json!([]),
    );
    assert_eq!(gone["Items"][0]["plays"], json!({"N": "3"}));
    assert_eq!(
        run(&mut w, "SELECT * FROM music", json!([]))["Items"],
        json!([])
    );
    // Deleting what is not there is not an error.
    run(
        &mut w,
        "DELETE FROM music WHERE pk = 'a' AND sk = 1",
        json!([]),
    );
}

#[test]
fn set_add_and_set_delete_change_a_set() {
    let mut w = world();
    w.put(
        "music",
        json!({"pk": {"S": "a"}, "sk": {"N": "1"}, "tags": {"SS": ["x"]}}),
    );
    run(
        &mut w,
        "UPDATE music SET tags = set_add(tags, <<'y', 'z'>>) WHERE pk = 'a' AND sk = 1",
        json!([]),
    );
    run(
        &mut w,
        "UPDATE music SET tags = set_delete(tags, <<'x'>>) WHERE pk = 'a' AND sk = 1",
        json!([]),
    );
    let got = run(
        &mut w,
        "SELECT tags FROM music WHERE pk = 'a' AND sk = 1",
        json!([]),
    );
    assert_eq!(got["Items"][0]["tags"], json!({"SS": ["y", "z"]}));
}

#[test]
fn refusals_say_what_and_parameters_must_match() {
    let mut w = world();
    let e = fail(&mut w, "SELECT * FROM music WHERE pk = ?", json!([]));
    assert_eq!(
        message(&e),
        "Number of parameters in request and statement don't match."
    );
    let e = fail(&mut w, "SELECT * FROM music ORDER BY pk", json!([]));
    assert_eq!(
        (e.status, e.code.as_ref(), message(&e)),
        (
            501,
            "NotImplemented",
            "PartiQL ORDER BY is not built in doze-aws."
        )
    );
    let e = fail(&mut w, "SELEKT * FROM music", json!([]));
    assert_eq!(e.code, "ValidationException");
    assert!(message(&e).starts_with("Statement wasn't well formed"));
    let e = fail(&mut w, "SELECT * FROM nothing", json!([]));
    assert_eq!(e.code, "ResourceNotFoundException");
}

#[test]
fn a_batch_answers_for_each_statement() {
    let mut w = world();
    w.put("music", json!({"pk": {"S": "old"}, "sk": {"N": "1"}}));
    let out = w.ok(
        "BatchExecuteStatement",
        json!({"Statements": [
            {"Statement": "INSERT INTO music VALUE {'pk': 'a', 'sk': 1}"},
            {"Statement": "INSERT INTO music VALUE {'pk': 'old', 'sk': 1}"},
            {"Statement": "UPDATE music SET x = 1 WHERE pk = 'b' AND sk = 1"},
        ]}),
    );
    let r = out["Responses"].as_array().unwrap();
    assert_eq!(r[0], json!({"TableName": "music"}));
    assert_eq!(r[1]["Error"]["Code"], "DuplicateItem");
    assert_eq!(r[2]["Error"]["Code"], "ConditionalCheckFailed");
    let e = w.err(
        "BatchExecuteStatement",
        json!({"Statements": [
            {"Statement": "INSERT INTO music VALUE {'pk': 'c', 'sk': 1}"},
            {"Statement": "DELETE FROM music WHERE pk = 'c' AND sk = 1"},
        ]}),
    );
    assert_eq!(
        message(&e),
        "Provided list of item keys contains duplicates"
    );

    let read = w.ok(
        "BatchExecuteStatement",
        json!({"Statements": [
            {"Statement": "SELECT * FROM music WHERE pk = ? AND sk = ?", "Parameters": [{"S": "a"}, {"N": "1"}]},
            {"Statement": "SELECT * FROM music WHERE pk = 'z' AND sk = 1"},
            {"Statement": "SELECT * FROM music WHERE pk = 'a'"},
        ]}),
    );
    let r = read["Responses"].as_array().unwrap();
    assert_eq!(r[0]["Item"]["pk"], json!({"S": "a"}));
    assert_eq!(r[1], json!({"TableName": "music"}));
    assert_eq!(r[2]["Error"]["Code"], "ValidationError");

    let e = w.err(
        "BatchExecuteStatement",
        json!({"Statements": [
            {"Statement": "SELECT * FROM music WHERE pk = 'a' AND sk = 1"},
            {"Statement": "DELETE FROM music WHERE pk = 'a' AND sk = 1"},
        ]}),
    );
    assert_eq!(e.code, "ValidationException");
}

#[test]
fn a_transaction_is_all_or_nothing() {
    let mut w = world();
    w.put("music", json!({"pk": {"S": "taken"}, "sk": {"N": "1"}}));
    let e = w.err(
        "ExecuteTransaction",
        json!({"TransactStatements": [
            {"Statement": "INSERT INTO music VALUE {'pk': 'new', 'sk': 1}"},
            {"Statement": "INSERT INTO music VALUE {'pk': 'taken', 'sk': 1}"},
        ]}),
    );
    assert_eq!(e.code, "TransactionCanceledException");
    assert_eq!(
        message(&e),
        "Transaction cancelled, please refer cancellation reasons for specific reasons [None, DuplicateItem]"
    );
    assert_eq!(
        run(
            &mut w,
            "SELECT * FROM music WHERE pk = 'new' AND sk = 1",
            json!([])
        )["Items"],
        json!([]),
        "nothing of a cancelled transaction is written"
    );

    w.ok(
        "ExecuteTransaction",
        json!({"TransactStatements": [
            {"Statement": "INSERT INTO music VALUE {'pk': 'new', 'sk': 1}"},
            {"Statement": "UPDATE music SET n = 1 WHERE pk = 'taken' AND sk = 1"},
        ]}),
    );
    let read = w.ok(
        "ExecuteTransaction",
        json!({"TransactStatements": [
            {"Statement": "SELECT * FROM music WHERE pk = 'new' AND sk = 1"},
            {"Statement": "SELECT n FROM music WHERE pk = 'taken' AND sk = 1"},
        ]}),
    );
    assert_eq!(read["Responses"][0]["Item"]["pk"], json!({"S": "new"}));
    assert_eq!(read["Responses"][1]["Item"], json!({"n": {"N": "1"}}));
}

#[test]
fn a_select_on_an_index_scans_the_index() {
    let mut w = World::new();
    w.ok(
        "CreateTable",
        json!({"TableName": "tbl", "BillingMode": "PAY_PER_REQUEST",
            "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
            "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}, {"AttributeName": "g", "AttributeType": "S"}],
            "GlobalSecondaryIndexes": [{"IndexName": "by-g", "KeySchema": [{"AttributeName": "g", "KeyType": "HASH"}], "Projection": {"ProjectionType": "ALL"}}]}),
    );
    w.put("tbl", json!({"pk": {"S": "1"}, "g": {"S": "x"}}));
    w.put("tbl", json!({"pk": {"S": "2"}}));
    let got = run(&mut w, r#"SELECT * FROM "tbl"."by-g""#, json!([]));
    assert_eq!(got["Items"].as_array().unwrap().len(), 1);
    let e = fail(&mut w, r#"SELECT * FROM "tbl"."nope""#, json!([]));
    assert_eq!(e.code, "ResourceNotFoundException");
}
