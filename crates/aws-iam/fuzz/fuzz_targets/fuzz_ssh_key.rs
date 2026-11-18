#![no_main]
//! Any text as an SSH public key (ssh-rsa or PEM): no panic.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        aws_iam::fuzz::ssh_key(text);
    }
});
