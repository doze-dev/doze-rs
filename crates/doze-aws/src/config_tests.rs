//! Precedence (flag > variable > file > default), where each value came from, paths anchored to their source, and refusals.

use super::*;

fn given(pairs: &[(&'static str, &str, Source)]) -> Given {
    pairs
        .iter()
        .map(|(k, v, s)| (*k, (v.split(',').map(str::to_owned).collect(), s.clone())))
        .collect()
}

fn dir_with(file: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(DEFAULT_FILE), file).unwrap();
    dir
}

#[test]
fn a_flag_beats_a_variable_beats_the_file_beats_the_default() {
    let dir = dir_with("region = \"eu-west-1\"\naccount = \"111122223333\"\nidle-secs = 9\n");
    let g = given(&[
        ("region", "ap-south-1", Source::Flag),
        ("account", "222233334444", Source::Env),
    ]);
    let cfg = Config::load(&g, None, dir.path()).unwrap();
    assert_eq!(cfg.text("region"), "ap-south-1");
    assert_eq!(cfg.text("account"), "222233334444");
    assert_eq!(cfg.count("idle-secs"), 9);
    assert_eq!(cfg.get("idle-secs").unwrap().source, Source::File(3));
    assert_eq!(cfg.text("listen"), "127.0.0.1:4566");
    assert_eq!(cfg.get("listen").unwrap().source, Source::Default);
    let over: Vec<&str> = cfg.overrules().map(|s| s.key.name).collect();
    assert_eq!(over, ["region", "account"]);
}

#[test]
fn a_data_dir_from_the_file_is_beside_the_file_and_from_a_flag_beside_the_shell() {
    let dir = dir_with("data-dir = \"state\"\n");
    let elsewhere = tempfile::tempdir().unwrap();
    let file = dir.path().join(DEFAULT_FILE);
    let cfg = Config::load(&Given::new(), Some(&file), elsewhere.path()).unwrap();
    assert_eq!(cfg.data_dir(), dir.path().join("state"));
    let g = given(&[("data-dir", "./mine", Source::Flag)]);
    let cfg = Config::load(&g, Some(&file), elsewhere.path()).unwrap();
    assert_eq!(cfg.data_dir(), elsewhere.path().join("mine"));
}

#[test]
fn no_file_is_fine_and_a_named_one_must_exist() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config::load(&Given::new(), None, dir.path()).unwrap();
    assert!(cfg.file.is_none());
    assert_eq!(cfg.data_dir(), dir.path().join("doze-aws-data"));
    let e = Config::load(&Given::new(), Some(Path::new("nope.toml")), dir.path()).unwrap_err();
    assert!(e.contains("nope.toml"), "{e}");
}

#[test]
fn bad_values_are_refused_naming_where_they_came_from() {
    let dir = dir_with("\nregion = \"Mars\"\n");
    let e = Config::load(&Given::new(), None, dir.path()).unwrap_err();
    assert!(
        e.contains("line 2 (region)") && e.contains("not a region"),
        "{e}"
    );
    for (key, value, needle) in [
        ("listen", "localhost", "not an address"),
        ("account", "12", "twelve digits"),
        ("iam", "loud", "one of off, soft, enforce"),
        ("cloudwatch-retention-hours", "0", "at least 1"),
        ("services", "sqs,dynamdb", "did you mean `dynamodb`"),
    ] {
        let g = given(&[(key, value, Source::Flag)]);
        let e = Config::load(&g, None, &std::env::temp_dir()).unwrap_err();
        assert!(
            e.contains(&format!("--{key}")) && e.contains(needle),
            "{key}: {e}"
        );
    }
}

#[test]
fn an_unknown_key_in_the_file_is_refused_with_the_nearest_one() {
    let dir = dir_with("lambda-idle = 3\n");
    let e = Config::load(&Given::new(), None, dir.path()).unwrap_err();
    assert!(
        e.contains("`lambda-idle` is not a setting (did you mean `lambda-idle-secs`?)"),
        "{e}"
    );
}

#[test]
fn config_print_reads_back_to_the_same_values() {
    let dir = dir_with("services = [\"sqs\", \"s3\"]\nlambda-quiet = true\n");
    let g = given(&[("region", "eu-west-2", Source::Env)]);
    let first = Config::load(&g, None, dir.path()).unwrap();
    let printed = crate::show::config(&first);
    assert!(printed.contains("region = \"eu-west-2\""), "{printed}");
    assert!(printed.contains("# from DOZE_AWS_REGION"), "{printed}");
    assert!(
        printed.contains("# from ./doze-aws.toml, line 1 (services)")
            || printed.contains("line 1 (services)"),
        "{printed}"
    );
    let again = dir_with(&printed);
    let second = Config::load(&Given::new(), None, again.path()).unwrap();
    for s in first.all() {
        assert_eq!(
            second.get(s.key.name).unwrap().value,
            s.value,
            "{}",
            s.key.name
        );
    }
}
