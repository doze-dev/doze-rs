#![no_main]
//! Any token, against a few users: never a panic, and a login only for a token that names the user and its
//! password.

use kafka_auth::{Credentials, plain};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let users = ["alice", "bob", "admin"].map(|u| (u.to_owned(), format!("{u}-secret")));
    let Ok(credentials) = Credentials::new(users) else {
        return;
    };
    match plain::check(data, &credentials) {
        Ok(principal) => {
            let name = principal.as_str().strip_prefix("User:").unwrap_or_default();
            let secret = format!("{name}-secret");
            let mut parts = data.splitn(4, |&b| b == 0);
            let (_, user, password) = (parts.next(), parts.next(), parts.next());
            assert_eq!(user, Some(name.as_bytes()));
            assert_eq!(password, Some(secret.as_bytes()));
        }
        Err(refusal) => {
            let _ = refusal.to_string();
        }
    }
});
