//! The two certificate shapes the authority makes, as Go's are: a root (CA, path length 0, cert-sign and CRL-sign)
//! and a leaf (digital signature, server auth, its DNS names, signed by the root).

use x509_cert::builder::profile::BuilderProfile;
use x509_cert::certificate::TbsCertificate;
use x509_cert::der::asn1::{Ia5String, ObjectIdentifier};
use x509_cert::der::referenced::OwnedToRef;
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::ext::pkix::{
    AuthorityKeyIdentifier, BasicConstraints, ExtendedKeyUsage, KeyUsage, KeyUsages,
    SubjectAltName, SubjectKeyIdentifier,
};
use x509_cert::ext::{Extension, ToExtension};
use x509_cert::name::Name;
use x509_cert::spki::{SubjectPublicKeyInfoOwned, SubjectPublicKeyInfoRef};

/// id-kp-serverAuth (RFC 5280).
const SERVER_AUTH: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.3.1");

/// Which certificate, and what it needs.
pub(crate) enum Profile {
    Root {
        subject: Name,
    },
    Leaf {
        subject: Name,
        issuer: Name,
        dns: Vec<String>,
        issuer_spki: SubjectPublicKeyInfoOwned,
    },
}

impl Profile {
    pub(crate) fn root(subject: Name) -> Profile {
        Profile::Root { subject }
    }

    pub(crate) fn leaf(
        subject: Name,
        issuer: Name,
        dns: Vec<String>,
        issuer_spki: SubjectPublicKeyInfoOwned,
    ) -> Profile {
        Profile::Leaf {
            subject,
            issuer,
            dns,
            issuer_spki,
        }
    }
}

impl BuilderProfile for Profile {
    fn get_issuer(&self, subject: &Name) -> Name {
        match self {
            Profile::Root { .. } => subject.clone(),
            Profile::Leaf { issuer, .. } => issuer.clone(),
        }
    }

    fn get_subject(&self) -> Name {
        match self {
            Profile::Root { subject } | Profile::Leaf { subject, .. } => subject.clone(),
        }
    }

    fn build_extensions(
        &self,
        spk: SubjectPublicKeyInfoRef<'_>,
        _issuer_spk: SubjectPublicKeyInfoRef<'_>,
        tbs: &TbsCertificate,
    ) -> x509_cert::builder::Result<Vec<Extension>> {
        let mut out: Vec<Extension> = Vec::new();
        let subject = tbs.subject();
        match self {
            Profile::Root { .. } => {
                let ski = SubjectKeyIdentifier::try_from(spk)?;
                out.push(ski.to_extension(subject, &out)?);
                let bc = BasicConstraints {
                    ca: true,
                    path_len_constraint: Some(0),
                };
                out.push(bc.to_extension(subject, &out)?);
                out.push(
                    KeyUsage(KeyUsages::KeyCertSign | KeyUsages::CRLSign)
                        .to_extension(subject, &out)?,
                );
            }
            Profile::Leaf {
                dns, issuer_spki, ..
            } => {
                let issuer_ski = SubjectKeyIdentifier::try_from(issuer_spki.owned_to_ref())?;
                let aki = AuthorityKeyIdentifier {
                    key_identifier: Some(issuer_ski.0),
                    ..Default::default()
                };
                out.push(aki.to_extension(subject, &out)?);
                out.push(KeyUsage(KeyUsages::DigitalSignature.into()).to_extension(subject, &out)?);
                out.push(ExtendedKeyUsage(vec![SERVER_AUTH]).to_extension(subject, &out)?);
                let names = dns
                    .iter()
                    .map(|d| Ia5String::new(d).map(GeneralName::DnsName))
                    .collect::<Result<Vec<_>, _>>()?;
                out.push(SubjectAltName(names).to_extension(subject, &out)?);
            }
        }
        Ok(out)
    }
}

/// Today as `YYYY-MM-DD` (UTC), for the root's name: a person who rotates sees which root is which.
pub(crate) fn today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400;
    let (y, m, d) = civil_from_days(i64::try_from(days).unwrap_or(0));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// This machine's name for the root's subject (Go: `os.Hostname`, without `.local`).
pub(crate) fn hostname() -> String {
    let from_env = ["HOSTNAME", "COMPUTERNAME"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|h| !h.is_empty()));
    let name = from_env.or_else(|| {
        let out = std::process::Command::new("hostname").output().ok()?;
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|h| !h.is_empty())
    });
    name.map(|h| h.trim_end_matches(".local").to_string())
        .unwrap_or_else(|| "this machine".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_405), (2025, 11, 13));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }
}
