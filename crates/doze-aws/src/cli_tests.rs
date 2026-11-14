//! Dispatch: a bare flag serves, a command runs, a typo is an error (never a server), and settings come from both levels.

use super::*;

fn parse(args: &[&str]) -> Result<(Cli, ArgMatches), clap::Error> {
    let m =
        command().try_get_matches_from(std::iter::once("doze-aws").chain(args.iter().copied()))?;
    let cli = <Cli as clap::FromArgMatches>::from_arg_matches(&m)?;
    Ok((cli, m))
}

#[test]
fn no_command_or_a_bare_flag_serves() {
    let (cli, m) = parse(&[]).unwrap();
    assert!(cli.command.is_none());
    let (cli, m2) = parse(&["--listen", "127.0.0.1:0", "--services", "sqs,s3"]).unwrap();
    assert!(cli.command.is_none());
    let (g, _) = given(&m2, None);
    assert_eq!(g["listen"], (vec!["127.0.0.1:0".to_owned()], Source::Flag));
    assert_eq!(g["services"].0, ["sqs", "s3"]);
    assert!(given(&m, None).0.is_empty());
}

#[test]
fn a_typo_is_an_error_that_suggests_the_command() {
    let e = parse(&["aply"]).unwrap_err();
    assert_eq!(e.kind(), clap::error::ErrorKind::InvalidSubcommand);
    assert!(e.to_string().contains("apply"), "{e}");
    let e = parse(&["--lisen", "x"]).unwrap_err();
    assert!(e.to_string().contains("--listen"), "{e}");
}

#[test]
fn version_has_its_flags_and_its_command() {
    assert!(parse(&["--version"]).unwrap().0.version);
    assert!(parse(&["-V"]).unwrap().0.version);
    assert!(matches!(
        parse(&["version"]).unwrap().0.command,
        Some(Command::Version)
    ));
}

#[test]
fn commands_take_their_own_arguments_and_the_settings() {
    let (cli, m) = parse(&[
        "--region",
        "eu-west-1",
        "apply",
        "t.yaml",
        "--param",
        "A=1",
        "--stack",
        "s",
        "--data-dir",
        "d",
    ])
    .unwrap();
    let Some(Command::Apply(a)) = cli.command else {
        panic!("not apply")
    };
    assert_eq!(
        (a.template.unwrap(), a.stack.unwrap(), a.params),
        (
            PathBuf::from("t.yaml"),
            "s".to_owned(),
            vec!["A=1".to_owned()]
        )
    );
    let (g, _) = given(&m, m.subcommand().map(|(_, s)| s));
    assert_eq!(g["region"].0, ["eu-west-1"]);
    assert_eq!(g["data-dir"].0, ["d"]);
    let (cli, _) = parse(&["config", "print", "--lambda-quiet"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Config(ConfigCommand::Print))
    ));
    assert!(parse(&["config"]).is_err(), "config needs print");
    let (cli, _) = parse(&["env", "--shell", "fish"]).unwrap();
    let Some(Command::Env(e)) = cli.command else {
        panic!("not env")
    };
    assert_eq!(e.shell.as_deref(), Some("fish"));
}

#[test]
fn a_switch_given_is_true_and_the_account_has_its_go_spelling() {
    let (_, m) = parse(&["--lambda-quiet", "--account-id", "111122223333"]).unwrap();
    let (g, _) = given(&m, None);
    assert_eq!(g["lambda-quiet"].0, ["true"]);
    assert_eq!(g["account"].0, ["111122223333"]);
    command().debug_assert();
}
