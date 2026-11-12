//! The binary with a users file, the environment variable and the credential API: a user made over the wire survives the
//! process, and the bootstrap passwords are in no log line.

use std::os::unix::fs::PermissionsExt;

use super::*;
use kafka_auth::scram::Kind;
use kafka_auth::scram::client::Client;
use kafka_protocol::messages::alter_user_scram_credentials_request::ScramCredentialUpsertion;
use kafka_protocol::messages::{
    AlterUserScramCredentialsRequest, AlterUserScramCredentialsResponse, SaslAuthenticateRequest,
    SaslAuthenticateResponse, SaslHandshakeRequest, SaslHandshakeResponse,
};

/// A SCRAM-SHA-256 login as `user`: the error code of the first and of the second token.
fn log_in(s: &mut TcpStream, user: &str, password: &str) -> (i16, i16) {
    let mut hs = SaslHandshakeRequest::default();
    hs.mechanism = StrBytes::from_static_str("SCRAM-SHA-256");
    let said: SaslHandshakeResponse = call(s, ApiKey::SaslHandshake, 1, &hs);
    assert_eq!(said.error_code, 0);
    let me = Client::new(Kind::Sha256, user, password, "process-nonce.");
    let mut auth = SaslAuthenticateRequest::default();
    auth.auth_bytes = Bytes::from(me.first());
    let first: SaslAuthenticateResponse = call(s, ApiKey::SaslAuthenticate, 2, &auth);
    if first.error_code != 0 {
        return (first.error_code, 0);
    }
    auth.auth_bytes = Bytes::from(me.last(&first.auth_bytes, None, "").message);
    let done: SaslAuthenticateResponse = call(s, ApiKey::SaslAuthenticate, 2, &auth);
    (0, done.error_code)
}

/// The request that makes `user`: the client sends the salted password, the broker never sees the password.
fn make_user(user: &str, password: &str) -> AlterUserScramCredentialsRequest {
    let salt = format!("salt-of-{user}").into_bytes();
    let iterations = std::num::NonZeroU32::new(4096).unwrap();
    let mut up = ScramCredentialUpsertion::default();
    up.name = StrBytes::from_string(user.to_owned());
    up.mechanism = 1;
    up.iterations = 4096;
    up.salted_password =
        Bytes::from(Kind::Sha256.salted_password(password.as_bytes(), &salt, iterations));
    up.salt = Bytes::from(salt);
    let mut req = AlterUserScramCredentialsRequest::default();
    req.upsertions.push(up);
    req
}

fn file(dir: &Path, text: &str, mode: u32) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("users");
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    path
}

#[test]
fn users_from_a_file_and_the_environment_log_in_and_one_made_over_the_api_outlives_the_process() {
    let data = scratch("users-api");
    let users = scratch("users-api-file");
    let path = file(&users, "# bootstrap\nadmin:admin-file-planted\n", 0o600);
    let flags = [
        "--sasl-mechanisms",
        "SCRAM-SHA-256",
        "--sasl-users-file",
        path.to_str().unwrap(),
        "--acl-default",
        "deny",
        "--acl-super-user",
        "User:admin",
        "--log-level",
        "trace",
    ];
    let env = [("DOZE_KAFKA_SASL_USERS", "bob:bob-env-planted")];
    let mut broker = start_with_env(&data, &flags, &env);
    assert_eq!(
        log_in(&mut broker.connect(), "bob", "bob-env-planted"),
        (0, 0)
    );
    let mut admin = broker.connect();
    assert_eq!(log_in(&mut admin, "admin", "admin-file-planted"), (0, 0));
    let req = make_user("carol", "carol-api-planted");
    let made: AlterUserScramCredentialsResponse =
        call(&mut admin, ApiKey::AlterUserScramCredentials, 0, &req);
    assert_eq!(
        (made.results[0].user.as_str(), made.results[0].error_code),
        ("carol", 0)
    );
    assert_eq!(
        log_in(&mut broker.connect(), "carol", "carol-api-planted"),
        (0, 0)
    );
    let status = broker.terminate();
    assert!(status.success(), "{status:?}");

    // Started again: the configured users come from the file and the variable again, and carol from the database.
    let again = start_with_env(&data, &flags, &env);
    for (user, password) in [
        ("bob", "bob-env-planted"),
        ("admin", "admin-file-planted"),
        ("carol", "carol-api-planted"),
    ] {
        assert_eq!(
            log_in(&mut again.connect(), user, password),
            (0, 0),
            "{user}"
        );
    }
    assert_eq!(log_in(&mut again.connect(), "carol", "other").1, 58);
    std::thread::sleep(Duration::from_millis(300));
    for broker in [&broker, &again] {
        let log = broker.log.lock().unwrap().clone();
        assert!(log.contains("SASL users"), "{log}");
        assert!(!log.contains("planted"), "a secret is in the log:\n{log}");
    }
    drop((broker, again));
    let _ = std::fs::remove_dir_all(&data);
    let _ = std::fs::remove_dir_all(&users);
}

#[test]
fn a_users_file_others_can_read_is_warned_about_without_its_content() {
    let data = scratch("users-mode");
    let users = scratch("users-mode-file");
    let path = file(&users, "alice:alice-mode-planted\n", 0o644);
    let broker = start_with(
        &data,
        &[
            "--sasl-mechanisms",
            "SCRAM-SHA-256",
            "--sasl-users-file",
            path.to_str().unwrap(),
            "--log-level",
            "trace",
        ],
    );
    let log = broker.log.lock().unwrap().clone();
    assert!(log.contains("readable by group or others"), "{log}");
    assert!(log.contains("chmod 600"), "{log}");
    assert!(!log.contains("planted"), "{log}");
    drop(broker);
    let _ = std::fs::remove_dir_all(&data);
    let _ = std::fs::remove_dir_all(&users);
}

/// The refusal of `flags` on `data`, in words.
fn refused(data: &Path, flags: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
        .args(["--listen", "127.0.0.1:0", "--data-dir"])
        .arg(data)
        .args(flags)
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!output.status.success(), "{said}");
    said
}

#[test]
fn a_secured_broker_with_no_user_anywhere_is_refused_and_one_with_stored_users_starts() {
    // SEC-176: there is no flag to start with nobody able to log in.
    let data = scratch("users-empty");
    let secured = ["--sasl-mechanisms", "SCRAM-SHA-256"];
    let said = refused(&data, &secured);
    assert!(said.contains("no credentials are configured"), "{said}");
    assert!(said.contains("--sasl-user USER:PASSWORD"), "{said}");
    assert!(!said.contains("allow-empty"), "{said}");
    assert!(
        !data.join("meta.redb").exists(),
        "nothing was created: {said}"
    );
    assert!(!refused(&data, &["--sasl-allow-empty-users"]).contains("no credentials"));
    // A broker without SASL makes a user over the API (SEC-D51) and then the database holds one.
    let mut open = start_with(&data, &["--acl-default", "allow"]);
    let req = make_user("carol", "carol-stored-planted");
    let made: AlterUserScramCredentialsResponse = call(
        &mut open.connect(),
        ApiKey::AlterUserScramCredentials,
        0,
        &req,
    );
    assert_eq!(made.results[0].error_code, 0);
    assert!(open.terminate().success());
    // PLAIN cannot use a stored credential, so that start is refused once the database is read.
    let said = refused(&data, &["--sasl-mechanisms", "PLAIN"]);
    assert!(said.contains("no credentials are configured"), "{said}");
    // SCRAM can: the stored user logs in with no user on the command line.
    let mut broker = start_with(&data, &secured);
    assert_eq!(
        log_in(&mut broker.connect(), "carol", "carol-stored-planted"),
        (0, 0)
    );
    assert!(broker.terminate().success());
    let _ = std::fs::remove_dir_all(&data);
    // A database that holds no user at all is refused after it is opened.
    let empty = scratch("users-empty-db");
    let mut open = start_with(&empty, &["--acl-default", "allow"]);
    assert!(open.terminate().success());
    let said = refused(&empty, &secured);
    assert!(said.contains("no credentials are configured"), "{said}");
    let _ = std::fs::remove_dir_all(&empty);
}

#[test]
fn the_metadata_database_is_owner_only_and_an_open_one_from_an_earlier_build_is_narrowed() {
    // SEC-193: the file holds the SCRAM salts and keys.
    let data = scratch("db-mode");
    let path = data.join("meta.redb");
    let mode = || std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    let mut first = start_with(&data, &[]);
    assert_eq!(mode(), 0o600);
    assert!(first.terminate().success());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let mut again = start_with(&data, &[]);
    assert_eq!(mode(), 0o600);
    assert!(again.terminate().success());
    let log = again.log.lock().unwrap().clone();
    assert!(log.contains("now readable by its owner only"), "{log}");
    let _ = std::fs::remove_dir_all(&data);
}
