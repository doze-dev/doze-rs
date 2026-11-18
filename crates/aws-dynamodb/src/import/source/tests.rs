#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;

#[test]
fn csv_follows_rfc_4180_and_types_the_keys() {
    let text = "pk,n,note\r\n\"a,b\",7,\"say \"\"hi\"\"\nthere\"\nc,x,\n";
    let mut out = Read::default();
    csv(
        text,
        ',',
        None,
        &[("pk".into(), 'S'), ("n".into(), 'N')],
        &mut out,
    );
    assert_eq!(out.items.len(), 1);
    assert_eq!(out.errors, 1, "x is not a number");
    let item = &out.items[0];
    assert_eq!(item["pk"], Value::S("a,b".into()));
    assert_eq!(item["n"], Value::N(Number::from_i64(7)));
    assert_eq!(item["note"], Value::S("say \"hi\"\nthere".into()));
    let mut given = Read::default();
    csv(
        "x;1\n",
        ';',
        Some(&["pk".into(), "v".into()]),
        &[],
        &mut given,
    );
    assert_eq!(given.items[0]["v"], Value::S("1".into()));
}

#[test]
fn json_lines_and_ion_read_and_count_what_does_not() {
    let job = Job {
        arn: String::new(),
        caller: crate::work::service_caller("arn:aws:s3:us-east-1:000000000000:x"),
        bucket: String::new(),
        prefix: String::new(),
        format: Format::Json,
        gzip: false,
        keys: Vec::new(),
    };
    let mut out = Read::default();
    parse(
        "{\"Item\":{\"pk\":{\"S\":\"a\"}}}\n\nnot json\n{\"Item\":{\"pk\":{\"Q\":1}}}\n",
        &job,
        &mut out,
    );
    assert_eq!((out.items.len(), out.errors), (1, 2));
    let mut ion_out = Read::default();
    ion(
        "$ion_1_0\n[{Item:{pk:\"a\"}},{Item:{pk:\"b\", n:6d2}}]\n$ion_1_0 {Item:{pk:\"c\"}}\n$ion_1_0 {Other:1}",
        &mut ion_out,
    );
    assert_eq!((ion_out.items.len(), ion_out.errors), (3, 1));
    assert_eq!(ion_out.items[1]["n"], Value::N(Number::from_i64(600)));
}
