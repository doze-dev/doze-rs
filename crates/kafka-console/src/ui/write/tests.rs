use super::*;
use crate::model::Tok;
use crate::refusal::refusal;

fn hostile() -> String {
    "<script>alert(1)</script>\"'&".to_owned()
}

#[test]
fn the_steps_bar_marks_the_step_it_is_on_and_checks_the_ones_before() {
    let one = steps(Step::Name).into_string();
    assert!(
        one.contains(r#"<span class="s on"><i>1</i>Name and partitions</span>"#),
        "{one}"
    );
    assert!(
        one.contains(r#"<span class="s "><i>2</i>Configuration</span>"#),
        "{one}"
    );
    assert_eq!(one.matches(r#"class="bar""#).count(), 2);
    let three = steps(Step::Review).into_string();
    assert_eq!(three.matches("class=\"s done\"").count(), 2, "{three}");
    assert!(
        three.contains(r#"<span class="s on"><i>3</i>Review and create</span>"#),
        "{three}"
    );
    let two = steps(Step::Config).into_string();
    assert_eq!(two.matches("class=\"s done\"").count(), 1);
}

#[test]
fn teaching_has_the_five_lines_of_the_boards_in_order() {
    let t = Teach {
        says: "the number of partitions".to_owned(),
        default: vec![Span::mono("1")],
        range: vec![Span::text("1 to "), Span::strong("1,000")],
        takes_effect: vec![Span::text("at once")],
        here: vec![Span::text("order is kept inside a partition")],
    };
    let m = teach(&t).into_string();
    let mut at = 0;
    for want in ["Kafka says", "Default", "Range", "Takes effect", "Here"] {
        at += m[at..]
            .find(&format!("<b>{want}</b>"))
            .unwrap_or_else(|| panic!("{want} in {m}"));
    }
    assert!(m.starts_with(r#"<div class="teach" data-extra="teaching">"#));
    assert!(m.contains(r#"<span class="mono">1</span>"#) && m.contains("<b>1,000</b>"));
}

#[test]
fn a_refusal_shows_kafkas_code_and_the_brokers_sentence_and_escapes_both() {
    let mut r = refusal(36, Some("Topic 'orders.retry' already exists."));
    r.would_pass = Some(vec![Span::text("another name")]);
    let m = refused(&r).into_string();
    assert!(
        m.contains("<b>Refused · TOPIC_ALREADY_EXISTS (36)</b>"),
        "{m}"
    );
    assert!(
        m.contains("Topic &#39;orders.retry&#39; already exists.")
            || m.contains("Topic 'orders.retry' already exists."),
        "{m}"
    );
    assert!(m.contains("<b>What would pass</b> another name"), "{m}");
    assert!(m.starts_with(r#"<div class="callout refused" data-extra="refusal">"#));
    let mut bad = refusal(17, Some(&hostile()));
    bad.rule = Some(vec![Span::mono(hostile())]);
    bad.would_pass = Some(vec![Span::strong(hostile())]);
    bad.name = hostile();
    let m = refused(&bad).into_string();
    assert!(!m.contains("<script>") && !m.contains("</script>"), "{m}");
    assert!(m.contains("&lt;script&gt;alert(1)&lt;/script&gt;"), "{m}");
}

#[test]
fn a_text_input_carries_its_id_name_value_and_its_error_state() {
    let m = text_input(
        "n",
        "name",
        "orders.retry",
        true,
        Some("of 1 to 1,000"),
        false,
    )
    .into_string();
    assert_eq!(
        m,
        r#"<div class="input mono"><input id="n" name="name" type="text" value="orders.retry" autocomplete="off" spellcheck="false"><span class="unit">of 1 to 1,000</span></div>"#
    );
    let e = text_input("n", "name", &hostile(), false, None, true).into_string();
    assert!(
        e.starts_with(r#"<div class="input err">"#) && e.contains(r#"aria-invalid="true""#),
        "{e}"
    );
    assert!(!e.contains("<script>") && e.contains("&quot;"), "{e}");
    assert_eq!(
        field_error("bad <b>").into_string(),
        r#"<div class="help err">bad &lt;b&gt;</div>"#
    );
}

#[test]
fn the_call_is_a_titled_block_with_its_note() {
    let lines: Lines = vec![vec![
        (Tok::Keyword, "Produce".to_owned()),
        (Tok::Plain, " v9".to_owned()),
    ]];
    let m = apicall("The call", &lines, Some("note")).into_string();
    assert!(
        m.starts_with(r#"<div class="apicall" data-extra="api-preview"><div class="hd">"#),
        "{m}"
    );
    assert!(
        m.contains(r#"<pre class="code"><span class="k">Produce</span> v9</pre>"#),
        "{m}"
    );
    assert!(m.contains(r#"<p class="help" style="margin:12px 0 0">note</p>"#));
    assert!(
        !apicall("x", &lines, None)
            .into_string()
            .contains("<p class")
    );
    let t = terminal(
        "cmd",
        &["kafka-topics.sh \\".to_owned(), "  --create".to_owned()],
    )
    .into_string();
    assert!(
        t.contains(r#"<pre class="code" id="cmd">"#)
            && t.contains("kafka-topics.sh \\\n  --create"),
        "{t}"
    );
    assert!(
        t.contains(r#"data-copy-from="cmd""#) && t.contains("hidden"),
        "{t}"
    );
}
