//! The operating system's trusted roots, read once on the first `https://` call (off the runtime's thread).
//!
//! **Linux and Windows:** `rustls-native-certs` (the bundle `openssl-probe` finds or `SSL_CERT_FILE`; the Windows store).
//!
//! **macOS:** the keychains are listed with Apple's own `/usr/bin/security find-certificate -a -p`, rather than through
//! Security.framework, because linking that framework (and CoreFoundation) cost doze-aws about 0.6 MB of its own memory and 3.8 MB
//! of resident pages at idle, whether or not it ever calls out (`xtask/weight-limits.txt`, tls2). Three keychains are read: the
//! system roots (`/System/Library/Keychains/SystemRootCertificates.keychain`), the system keychain (`/Library/Keychains/System.keychain`,
//! where MDM and corporate proxies install their CAs) and the user's login keychain (where `mkcert -install` and a hand-trusted local
//! CA go). Listing certificates needs no password and shows no prompt; private keys are not touched.
//!
//! **Known difference on macOS:** keychain trust settings are not read. Every certificate in those keychains is a root, so one
//! marked "Never Trust" is trusted, and one that sits in the login keychain without being marked trusted is trusted too.
//! `--ca-file` is the precise alternative.

use rustls::pki_types::CertificateDer;

/// The roots found, and what could not be read.
#[cfg(not(target_os = "macos"))]
pub(crate) fn load() -> (Vec<CertificateDer<'static>>, Vec<String>) {
    let found = rustls_native_certs::load_native_certs();
    let errors = found.errors.iter().map(ToString::to_string).collect();
    (found.certs, errors)
}

/// The keychains read, in order.
#[cfg(target_os = "macos")]
fn keychains() -> Vec<std::path::PathBuf> {
    let mut out = vec![
        "/System/Library/Keychains/SystemRootCertificates.keychain".into(),
        "/Library/Keychains/System.keychain".into(),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        let login = std::path::Path::new(&home).join("Library/Keychains/login.keychain-db");
        if login.exists() {
            out.push(login);
        }
    }
    out
}

/// The roots found, and what could not be read.
#[cfg(target_os = "macos")]
pub(crate) fn load() -> (Vec<CertificateDer<'static>>, Vec<String>) {
    use rustls::pki_types::pem::PemObject;
    let mut certs = Vec::new();
    let mut errors = Vec::new();
    for keychain in keychains() {
        let shown = keychain.display().to_string();
        let out = std::process::Command::new("/usr/bin/security")
            .args(["find-certificate", "-a", "-p"])
            .arg(&keychain)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
        match out {
            Ok(o) if o.status.success() => {
                certs.extend(CertificateDer::pem_slice_iter(&o.stdout).filter_map(Result::ok));
            }
            Ok(o) => errors.push(format!("security find-certificate {shown}: {}", o.status)),
            Err(e) => errors.push(format!("/usr/bin/security: {e}")),
        }
    }
    (certs, errors)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_os_store_has_roots() {
        let (certs, errors) = super::load();
        assert!(
            certs.len() > 50,
            "{} roots, errors: {errors:?}",
            certs.len()
        );
    }
}
