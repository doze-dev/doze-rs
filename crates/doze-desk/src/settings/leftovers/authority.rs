//! Taking doze's certificate authority out of this Mac's trust: this home's roots only, named by fingerprint, never by
//! the common name every doze home's root shares ("doze local CA"; clearing by name once untrusted another home's root
//! in use). Its trust settings go first, as the person, through macOS's own dialog (root under the app's prompt
//! cannot show it); then root deletes the certificates by hash. A cancelled dialog stops before anything is deleted.

use std::path::{Path, PathBuf};

use crate::settings::quote;

const KEYCHAIN: &str = "/Library/Keychains/System.keychain";

/// This home's roots: the current one, and every rotated-out one it kept (`previous/<fp>.crt`, named in
/// `previous.json`). Only fingerprints that are SHA-256 hex reach a script.
fn previous(ca: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(ca.join("previous.json")).unwrap_or_default();
    let fps: Vec<String> = serde_json::from_str(&text).unwrap_or_default();
    fps.into_iter().filter(|fp| valid(fp)).collect()
}

fn valid(fp: &str) -> bool {
    fp.len() == 64 && fp.bytes().all(|b| b.is_ascii_hexdigit())
}

fn files(ca: &Path) -> Vec<PathBuf> {
    std::iter::once(ca.join("root.crt"))
        .chain(
            previous(ca)
                .into_iter()
                .map(|fp| ca.join("previous").join(format!("{fp}.crt"))),
        )
        .filter(|f| f.is_file())
        .collect()
}

/// One root's admin trust setting removed by its file, as the person: only while the keychain trusts it, and only
/// "not found" let pass, so a cancelled or refused dialog stops the script (the same lines as doze-names'
/// `mac_remove_trust`, which a test holds them to).
pub(super) fn remove_trust(file: &str) -> String {
    let q = quote(file);
    format!(
        "if security verify-cert -L -l -q -p basic -c {q} >/dev/null 2>&1; then\n  \
         said=$(security remove-trusted-cert -d {q} 2>&1) || case \"$said\" in\n    \
         *'could not be found'*) ;;\n    *) printf '%s\\n' \"$said\" >&2; exit 1 ;;\n  esac\nfi"
    )
}

/// The person's part: every root file of this home, its trust setting removed.
pub(super) fn untrust_as_user(ca: &Path) -> String {
    files(ca)
        .iter()
        .map(|f| remove_trust(&f.to_string_lossy()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The SHA-256 of no bytes at all: what the fingerprint pipeline gives when openssl wrote nothing.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Root's part: this home's roots out of the System keychain, by SHA-256. The current root's is worked out from its
/// file as the script runs (the app keeps no hash of it), only once openssl has read it as a certificate, and never
/// the hash of nothing; rotated-out ones are named in `previous.json`.
pub(super) fn untrust_script(ca: &Path) -> String {
    let root = quote(&ca.join("root.crt").to_string_lossy());
    let refuse = "{ echo 'doze: cannot read the fingerprint of the root; nothing was deleted' >&2; exit 1; }";
    let mut lines = vec![
        format!("if [ -f {root} ]; then"),
        format!("  openssl x509 -in {root} -noout >/dev/null 2>&1 || {refuse}"),
        format!("  fp=$(openssl x509 -in {root} -outform der | shasum -a 256 | cut -d ' ' -f 1)"),
        format!("  case \"$fp\" in ''|{EMPTY_SHA256}) {refuse} ;; esac"),
        format!("  [ \"${{#fp}}\" = 64 ] || {refuse}"),
        format!("  security delete-certificate -t -Z \"$fp\" {KEYCHAIN} 2>/dev/null || true"),
        "fi".into(),
    ];
    for fp in previous(ca) {
        lines.push(format!(
            "security delete-certificate -t -Z {} {KEYCHAIN} 2>/dev/null || true",
            quote(&fp)
        ));
    }
    lines.join("\n")
}
