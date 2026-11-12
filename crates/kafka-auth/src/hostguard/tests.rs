//! The cases of the Go edition's guard, translated, and the shapes a DNS-rebinding page presents.

use super::*;

fn extra(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

fn refused(headers: &[&str], extra: &[String]) {
    for h in headers {
        assert!(!allowed(h, extra), "{h:?} should be refused");
    }
}

fn accepted(headers: &[&str], extra: &[String]) {
    for h in headers {
        assert!(allowed(h, extra), "{h:?} should be allowed");
    }
}

#[test]
fn ip_literals_localhost_and_names_under_it_are_allowed() {
    accepted(
        &[
            "127.0.0.1:8081",
            "127.0.0.1",
            "[::1]:8081",
            "[::1]",
            "[2001:db8::7]:80",
            "192.168.1.20:8081",
            "10.0.0.5",
            "localhost:8081",
            "LOCALHOST",
            "localhost.",
            "app.localhost:8081",
            "A.B.Localhost.",
            "localhost:",
            "[::1]:0",
            "127.0.0.1:65535",
        ],
        &[],
    );
}

#[test]
fn the_shapes_a_rebinding_page_presents_are_refused() {
    refused(
        &[
            "evil.example",
            "evil.example:8081",
            "EVIL.EXAMPLE.",
            "127.0.0.1.evil.example",
            "localhost.evil.example",
            "localhost.evil.example:80",
            "xlocalhost",
            "notlocalhost",
            "evil-localhost",
            "localhost:pw@evil.example",
            "evil.example:8081@localhost",
            "a@localhost",
            "a@127.0.0.1",
            "localhost:evil.example",
            "127.0.0.1:80 evil.example",
            "localhost:80a",
            "localhost: 80",
            "localhost:-1",
            "host.docker.internal:8081",
            // The .doze zone has no special case here.
            "kafka.doze",
            "doze",
        ],
        &[],
    );
}

#[test]
fn an_ipv6_literal_is_written_in_brackets_and_only_an_ipv6_literal_is() {
    accepted(&["[::1]", "[::ffff:127.0.0.1]:9"], &[]);
    refused(
        &[
            "::1",
            "::1:8081",
            "fe80::1",
            "[::1",
            "::1]",
            "[[::1]]",
            "]::1[",
            "[::1]x",
            "[::1]:8a",
            "[::1]@evil.example",
            "[evil]:80",
            "[localhost]",
            "[127.0.0.1]",
            "[fe80::1%eth0]",
            "[]",
            "[",
            "]",
        ],
        &[],
    );
    // Not even a listed name gets in through brackets.
    refused(
        &[
            "[dev.example.com]",
            "[dev.example.com]:1",
            "]dev.example.com",
        ],
        &extra(&["dev.example.com", "[", "]", ":"]),
    );
}

#[test]
fn a_port_is_only_trailing_digits() {
    accepted(
        &["localhost:0", "localhost:80", "localhost:99999999999"],
        &[],
    );
    refused(
        &[
            "localhost:8081:80",
            "localhost::80",
            ":80",
            "localhost:+1",
            "localhost:8 1",
        ],
        &[],
    );
}

#[test]
fn a_trailing_dot_is_the_same_name_but_one_is_all_and_an_address_has_none() {
    accepted(&["localhost.", "a.localhost.:1"], &[]);
    accepted(&["Dev.Example.COM."], &extra(&["dev.example.com"]));
    accepted(&["dev.example.com"], &extra(&["dev.example.com."]));
    refused(
        &[
            "localhost..",
            "localhost..:80",
            ".",
            ".:80",
            ".localhost",
            "a..localhost",
        ],
        &[],
    );
    // An address with a dot after it is not an address, and is no listed name.
    refused(&["127.0.0.1.", "127.0.0.1.:80"], &[]);
}

#[test]
fn an_empty_header_is_never_allowed() {
    refused(&["", ":", ":80"], &[]);
    refused(&["", ":80", "."], &extra(&["*", ""]));
}

#[test]
fn a_name_is_letters_digits_hyphens_and_underscores_in_labels_that_are_not_empty_or_too_long() {
    refused(
        &[
            "a b.localhost",
            "a/b.localhost",
            "a\u{0}.localhost",
            "a\r\nb.localhost",
            "a#.localhost",
            "a?.localhost",
            "a\\.localhost",
            "%41.localhost",
        ],
        &[],
    );
    accepted(&["my_app-1.localhost"], &[]);
    let label = "a".repeat(63);
    accepted(&[&format!("{label}.localhost")], &[]);
    refused(&[&format!("{}.localhost", "a".repeat(64))], &[]);
    // A very long host: refused, and listing it does not change that.
    let long = format!("{}.localhost", ["a"; 200].join("."));
    refused(
        &[&long, &"a".repeat(100_000)],
        &extra(&[&"a".repeat(100_000)]),
    );
}

#[test]
fn non_ascii_names_are_refused_because_a_browser_sends_punycode() {
    refused(
        &[
            "localhost\u{202e}",
            "ex\u{e4}mple.localhost",
            "\u{ff4c}ocalhost",
            "\u{212a}.localhost",
            "\u{1f4a9}",
        ],
        &[],
    );
    refused(&["b\u{fc}cher.example"], &extra(&["b\u{fc}cher.example"]));
    accepted(
        &["xn--bcher-kva.example"],
        &extra(&["xn--bcher-kva.example"]),
    );
}

#[test]
fn the_list_adds_exact_names() {
    let list = extra(&["host.docker.internal", " Dev.Example.com. "]);
    accepted(
        &[
            "host.docker.internal:8081",
            "HOST.DOCKER.INTERNAL",
            "host.docker.internal.",
            "dev.example.com:9093",
            "127.0.0.1:1",
        ],
        &list,
    );
    refused(
        &[
            "other.example.com",
            "x.host.docker.internal",
            "host.docker.internal.evil.example",
            "host.docker.internal:8081@evil.example",
            "dev.example.com.evil.example",
            "*",
        ],
        &list,
    );
    // A pattern is not a feature: only the whole "*" means anything.
    refused(&["a.example.com"], &extra(&["*.example.com"]));
    // An entry that holds a port is not a name.
    refused(&["dev.example.com:80"], &extra(&["dev.example.com:80"]));
}

#[test]
fn a_star_in_the_list_turns_the_check_off_but_an_empty_header_still_fails() {
    let list = extra(&["a.example", " * "]);
    accepted(
        &[
            "anything.example:8081",
            "x]y",
            "[evil]",
            "a@b",
            "\u{e4}.example",
        ],
        &list,
    );
    refused(&["", ":80"], &list);
}

#[test]
fn a_header_in_any_case_is_judged_alike() {
    for h in ["LocalHost:80", "lOcAlHoSt", "APP.LOCALHOST"] {
        assert!(allowed(h, &[]), "{h}");
    }
    assert!(allowed("EVIL.example", &extra(&["evil.EXAMPLE"])));
}
