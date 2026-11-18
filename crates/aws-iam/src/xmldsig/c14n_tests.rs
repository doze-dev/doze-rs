#![allow(clippy::unwrap_used)]

use super::super::reader::parse;
use super::super::tree::{Element, Limits};
use super::*;

const LIMITS: Limits = Limits {
    bytes: 100_000,
    depth: 32,
    elements: 1_000,
};

/// The path from the document element to the first element named `local`, depth first.
fn path_to<'a>(e: &'a Element, local: &str, path: &mut Vec<&'a Element>) -> bool {
    path.push(e);
    if e.local() == local || e.elements().any(|c| path_to(c, local, path)) {
        return true;
    }
    path.pop();
    false
}

/// The canonical form of the first element named `local` in `xml`.
fn c14n(xml: &str, local: &str, inclusive: &[&str]) -> String {
    let root = parse(xml, LIMITS).unwrap();
    let mut path = Vec::new();
    assert!(path_to(&root, local, &mut path), "{local} in {xml}");
    let inclusive: Vec<String> = inclusive.iter().map(|s| (*s).to_owned()).collect();
    canonicalize(&path, None, &inclusive)
}

#[test]
fn the_specs_two_documents_give_one_canonical_subtree() {
    // Exclusive XML Canonicalization 1.0, section 2.2: the same `n1:elem2` in two contexts.
    let first = r#"<n0:local xmlns:n0="foo:bar" xmlns:n3="ftp://example.org">
  <n1:elem2 xmlns:n1="http://example.net" xml:lang="en">
    <n3:stuff xmlns:n3="ftp://example.org"/>
  </n1:elem2>
</n0:local>"#;
    let second = r#"<n2:pdu xmlns:n1="http://example.com" xmlns:n2="http://foo.example" xml:lang="fr" xml:space="retain">
  <n1:elem2 xmlns:n1="http://example.net" xml:lang="en">
    <n3:stuff xmlns:n3="ftp://example.org"/>
  </n1:elem2>
</n2:pdu>"#;
    let expected = "<n1:elem2 xmlns:n1=\"http://example.net\" xml:lang=\"en\">\n    \
                    <n3:stuff xmlns:n3=\"ftp://example.org\"></n3:stuff>\n  </n1:elem2>";
    assert_eq!(c14n(first, "elem2", &[]), expected);
    assert_eq!(c14n(second, "elem2", &[]), expected);
}

#[test]
fn a_namespace_is_pushed_down_to_where_it_is_used() {
    let xml = r#"<r xmlns:p="urn:p" xmlns:unused="urn:u"><p:a><p:b/></p:a><c/></r>"#;
    assert_eq!(
        c14n(xml, "r", &[]),
        r#"<r><p:a xmlns:p="urn:p"><p:b></p:b></p:a><c></c></r>"#
    );
}

#[test]
fn a_prefix_rebound_to_another_uri_is_declared_again() {
    let xml = r#"<p:a xmlns:p="urn:1"><p:b xmlns:p="urn:2"><p:c xmlns:p="urn:2"/></p:b></p:a>"#;
    assert_eq!(
        c14n(xml, "a", &[]),
        r#"<p:a xmlns:p="urn:1"><p:b xmlns:p="urn:2"><p:c></p:c></p:b></p:a>"#
    );
}

#[test]
fn the_default_namespace_is_undeclared_only_where_an_ancestor_rendered_one() {
    let xml = r#"<a xmlns="urn:a"><b xmlns=""><c/></b></a>"#;
    assert_eq!(
        c14n(xml, "a", &[]),
        r#"<a xmlns="urn:a"><b xmlns=""><c></c></b></a>"#
    );
    assert_eq!(c14n(xml, "b", &[]), "<b><c></c></b>");
    assert_eq!(c14n(r#"<a xmlns=""/>"#, "a", &[]), "<a></a>");
}

#[test]
fn attributes_sort_by_namespace_uri_then_local_name() {
    // `b` is bound to the URI that sorts first, so `b:x` comes before `a:y`; unqualified attributes come first of all.
    let xml = r#"<e xmlns:a="urn:2" xmlns:b="urn:1" z="1" a:y="2" b:x="3" m="4"/>"#;
    assert_eq!(
        c14n(xml, "e", &[]),
        r#"<e xmlns:a="urn:2" xmlns:b="urn:1" m="4" z="1" b:x="3" a:y="2"></e>"#
    );
}

#[test]
fn text_and_attributes_are_escaped_as_the_spec_says() {
    let xml = "<a b=\"&lt;&gt;&amp;&quot;&#9;&#10;&#13;'\">&lt;&gt;&amp;&#13;\"'</a>";
    assert_eq!(
        c14n(xml, "a", &[]),
        "<a b=\"&lt;>&amp;&quot;&#x9;&#xA;&#xD;'\">&lt;&gt;&amp;&#xD;\"'</a>"
    );
}

#[test]
fn line_ends_and_attribute_whitespace_are_normalized() {
    let xml = "<a b=\"one\ttwo\r\nthree\">x\r\ny\rz</a>";
    assert_eq!(c14n(xml, "a", &[]), "<a b=\"one two three\">x\ny\nz</a>");
}

#[test]
fn comments_go_and_processing_instructions_and_cdata_stay_as_text() {
    let xml =
        "<?xml version=\"1.0\"?><a><!-- gone --><?pi  some data?><?bare?><![CDATA[<x>&]]></a>";
    assert_eq!(
        c14n(xml, "a", &[]),
        "<a><?pi some data?><?bare?>&lt;x&gt;&amp;</a>"
    );
}

#[test]
fn the_prefix_list_declares_prefixes_used_only_in_content() {
    let xml = r#"<r xmlns="urn:d" xmlns:xs="urn:xs" xmlns:xsi="urn:xsi"><v xsi:type="xs:string">t</v></r>"#;
    assert_eq!(
        c14n(xml, "v", &[]),
        r#"<v xmlns="urn:d" xmlns:xsi="urn:xsi" xsi:type="xs:string">t</v>"#
    );
    assert_eq!(
        c14n(xml, "v", &["xs"]),
        r#"<v xmlns="urn:d" xmlns:xs="urn:xs" xmlns:xsi="urn:xsi" xsi:type="xs:string">t</v>"#
    );
    // A listed prefix not in scope is not declared; `#default` names the default namespace.
    let plain = r#"<r xmlns:p="urn:p"><p:v/></r>"#;
    assert_eq!(
        c14n(plain, "v", &["nope", "#default"]),
        r#"<p:v xmlns:p="urn:p"></p:v>"#
    );
    let defaulted = r#"<r xmlns="urn:d" xmlns:p="urn:p"><p:v/></r>"#;
    assert_eq!(
        c14n(defaulted, "v", &["#default"]),
        r#"<p:v xmlns="urn:d" xmlns:p="urn:p"></p:v>"#
    );
}

#[test]
fn the_xml_prefix_is_never_declared() {
    assert_eq!(
        c14n(
            r#"<a xml:lang="en"><b xml:space="preserve"/></a>"#,
            "a",
            &[]
        ),
        r#"<a xml:lang="en"><b xml:space="preserve"></b></a>"#
    );
}

#[test]
fn an_omitted_subtree_is_left_out() {
    let root = parse("<a><s><t/></s>x<b/></a>", LIMITS).unwrap();
    let s = root.elements().next().unwrap();
    assert_eq!(canonicalize(&[&root], Some(s), &[]), "<a>x<b></b></a>");
}

#[test]
fn the_reader_refuses_what_is_not_well_formed() {
    for bad in [
        "<a><b></a>",
        "<a>",
        "<a></a><b></b>",
        "<p:a/>",
        "<a b=\"1\" b=\"2\"/>",
        r#"<a xmlns:p="urn:p" xmlns:q="urn:p" p:x="1" q:x="2"/>"#,
        "<a b=\"<\"/>",
        "<a b=\"1\"c=\"2\"/>",
        "<a>&unknown;</a>",
        "<a>&#0;</a>",
        r#"<!DOCTYPE a [<!ENTITY x "y">]><a>&x;</a>"#,
        "text<a/>",
        r#"<a xmlns:p=""/>"#,
    ] {
        assert!(parse(bad, LIMITS).is_err(), "{bad}");
    }
    let deep = "<a>".repeat(40) + &"</a>".repeat(40);
    assert!(parse(&deep, LIMITS).is_err());
}

#[test]
fn a_greater_than_inside_an_attribute_is_not_the_tags_end() {
    assert_eq!(c14n(r#"<a b="x>y"/>"#, "a", &[]), r#"<a b="x>y"></a>"#);
}
