//! The shell checks: the trap of AWS variables that send calls elsewhere.

use super::*;

fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn the_shell_trap_is_found_and_the_fix_named() {
    let i = Instance {
        endpoint: "http://127.0.0.1:4566".into(),
        region: "us-east-1".into(),
        ..Instance::default()
    };
    let none = shell_checks(&env(&[]), Some(&i));
    assert_eq!(none[0].verdict, Verdict::Warn);
    assert!(none[0].fix.as_deref().unwrap().contains("doze-aws env"));
    let elsewhere = shell_checks(
        &env(&[
            ("AWS_ENDPOINT_URL", "http://localhost:4567"),
            ("AWS_ENDPOINT_URL_S3", "x"),
        ]),
        Some(&i),
    );
    assert!(
        elsewhere[0].detail.contains("not this doze-aws"),
        "{elsewhere:?}"
    );
    assert!(
        elsewhere[1].detail.contains("AWS_ENDPOINT_URL_S3"),
        "{elsewhere:?}"
    );
    let good = shell_checks(
        &env(&[
            ("AWS_ENDPOINT_URL", "http://127.0.0.1:4566/"),
            ("AWS_REGION", "us-east-1"),
        ]),
        Some(&i),
    );
    assert!(good.iter().all(|c| c.verdict == Verdict::Ok), "{good:?}");
}
