use super::*;

const HOST: Option<&str> = Some("127.0.0.1:9093");

#[test]
fn a_read_needs_no_proof() {
    for m in ["GET", "get", "HEAD"] {
        assert_eq!(check(m, None, None, None), Ok(()), "{m}");
        assert_eq!(
            check(m, Some("http://evil.example"), Some("cross-site"), HOST),
            Ok(())
        );
    }
}

#[test]
fn the_browsers_own_word_decides_when_it_gives_one() {
    for (site, want) in [
        ("same-origin", Ok(())),
        ("SAME-ORIGIN", Ok(())),
        (" same-origin ", Ok(())),
        ("same-site", Err(Refusal::CrossSite)),
        ("cross-site", Err(Refusal::CrossSite)),
        ("none", Err(Refusal::CrossSite)),
        ("anything", Err(Refusal::CrossSite)),
    ] {
        assert_eq!(check("POST", None, Some(site), HOST), want, "{site}");
        // Even an Origin that matches does not rescue a request the browser calls cross-site.
        assert_eq!(
            check("POST", Some("http://127.0.0.1:9093"), Some(site), HOST),
            want,
            "{site}"
        );
    }
}

#[test]
fn without_the_browsers_word_the_origin_must_equal_the_host() {
    let post = |origin: Option<&str>, host: Option<&str>| check("POST", origin, None, host);
    assert_eq!(post(Some("http://127.0.0.1:9093"), HOST), Ok(()));
    assert_eq!(post(Some("https://127.0.0.1:9093"), HOST), Ok(()));
    assert_eq!(
        post(Some("HTTP://127.0.0.1:9093"), HOST),
        Err(Refusal::BadOrigin)
    );
    assert_eq!(
        post(Some("http://LOCALHOST:9093"), Some("localhost:9093")),
        Ok(())
    );
    assert_eq!(
        post(Some("http://localhost:9093"), Some("LocalHost:9093")),
        Ok(())
    );
    assert_eq!(post(Some("http://[::1]:9093"), Some("[::1]:9093")), Ok(()));
    assert_eq!(
        post(Some("http://127.0.0.1:9093"), Some("127.0.0.1")),
        Err(Refusal::OtherOrigin)
    );
    assert_eq!(
        post(Some("http://127.0.0.1"), HOST),
        Err(Refusal::OtherOrigin)
    );
    assert_eq!(
        post(Some("http://evil.example"), HOST),
        Err(Refusal::OtherOrigin)
    );
    assert_eq!(
        post(Some("http://127.0.0.1:9094"), HOST),
        Err(Refusal::OtherOrigin)
    );
    assert_eq!(
        post(Some("http://127.0.0.1:9093"), None),
        Err(Refusal::NoHost)
    );
    assert_eq!(
        post(Some("http://127.0.0.1:9093"), Some("")),
        Err(Refusal::NoHost)
    );
}

#[test]
fn what_is_not_an_origin_is_refused() {
    for bad in [
        "null",
        "",
        "http://",
        "ftp://127.0.0.1:9093",
        "127.0.0.1:9093",
        "http://127.0.0.1:9093/",
        "http://127.0.0.1:9093/x",
        "http://user@127.0.0.1:9093",
        "http://127.0.0.1:9093?x",
        "http://127.0.0.1:9093 ",
        "http://127.0.0.1 :9093",
        "http://127.0.0.1:9093\r\nX: y",
        "http://127.0.0.1:9093;evil",
        "file://",
    ] {
        let got = check("POST", Some(bad), None, HOST);
        // A trailing space is trimmed, so that one is the origin itself.
        if bad.trim() == "http://127.0.0.1:9093" {
            assert_eq!(got, Ok(()), "{bad:?}");
        } else {
            assert_eq!(got, Err(Refusal::BadOrigin), "{bad:?}");
        }
    }
}

#[test]
fn no_header_at_all_is_no_proof_whatever_the_method() {
    for m in [
        "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "TRACE", "post", "", "PURGE",
    ] {
        assert_eq!(check(m, None, None, HOST), Err(Refusal::NoProof), "{m:?}");
        assert_eq!(
            check(m, None, Some(""), HOST),
            Err(Refusal::NoProof),
            "{m:?}"
        );
    }
}

#[test]
fn every_refusal_has_words_that_say_what_to_do_or_what_happened() {
    for r in [
        Refusal::CrossSite,
        Refusal::NoProof,
        Refusal::BadOrigin,
        Refusal::OtherOrigin,
        Refusal::NoHost,
    ] {
        assert!(r.words().ends_with('.') && r.words().len() > 40, "{r:?}");
    }
}
