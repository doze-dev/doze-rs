//! Key specs and key usages, and what each can do.

use serde::{Deserialize, Serialize};

/// What a key is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Spec {
    Symmetric,
    Hmac224,
    Hmac256,
    Hmac384,
    Hmac512,
    EccP256,
    EccP384,
    EccP521,
    EccK1,
    Ed25519,
    Rsa2048,
    Rsa3072,
    Rsa4096,
}

/// What a key is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Usage {
    EncryptDecrypt,
    SignVerify,
    GenerateVerifyMac,
}

impl Spec {
    pub(crate) const ALL: [Self; 13] = [
        Self::Symmetric,
        Self::Hmac224,
        Self::Hmac256,
        Self::Hmac384,
        Self::Hmac512,
        Self::EccP256,
        Self::EccP384,
        Self::EccP521,
        Self::EccK1,
        Self::Ed25519,
        Self::Rsa2048,
        Self::Rsa3072,
        Self::Rsa4096,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Symmetric => "SYMMETRIC_DEFAULT",
            Self::Hmac224 => "HMAC_224",
            Self::Hmac256 => "HMAC_256",
            Self::Hmac384 => "HMAC_384",
            Self::Hmac512 => "HMAC_512",
            Self::EccP256 => "ECC_NIST_P256",
            Self::EccP384 => "ECC_NIST_P384",
            Self::EccP521 => "ECC_NIST_P521",
            Self::EccK1 => "ECC_SECG_P256K1",
            Self::Ed25519 => "ECC_NIST_EDWARDS25519",
            Self::Rsa2048 => "RSA_2048",
            Self::Rsa3072 => "RSA_3072",
            Self::Rsa4096 => "RSA_4096",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }

    /// The usage a key of this spec has when the request names none: only a symmetric key has a default.
    pub(crate) const fn default_usage(self) -> Option<Usage> {
        match self {
            Self::Symmetric => Some(Usage::EncryptDecrypt),
            _ => None,
        }
    }

    pub(crate) const fn allows(self, usage: Usage) -> bool {
        matches!(
            (self, usage),
            (Self::Symmetric, Usage::EncryptDecrypt)
                | (
                    Self::Hmac224 | Self::Hmac256 | Self::Hmac384 | Self::Hmac512,
                    Usage::GenerateVerifyMac
                )
                | (
                    Self::EccP256 | Self::EccP384 | Self::EccP521 | Self::EccK1 | Self::Ed25519,
                    Usage::SignVerify
                )
                | (
                    Self::Rsa2048 | Self::Rsa3072 | Self::Rsa4096,
                    Usage::EncryptDecrypt | Usage::SignVerify
                )
        )
    }

    pub(crate) const fn is_rsa(self) -> bool {
        matches!(self, Self::Rsa2048 | Self::Rsa3072 | Self::Rsa4096)
    }

    /// The modulus size of an RSA spec.
    pub(crate) const fn rsa_bits(self) -> Option<usize> {
        match self {
            Self::Rsa2048 => Some(2048),
            Self::Rsa3072 => Some(3072),
            Self::Rsa4096 => Some(4096),
            _ => None,
        }
    }

    pub(crate) const fn is_hmac(self) -> bool {
        matches!(
            self,
            Self::Hmac224 | Self::Hmac256 | Self::Hmac384 | Self::Hmac512
        )
    }

    /// The bytes of material a key of this spec has (the private scalar, the seed, the HMAC key, the AES key).
    pub(crate) const fn material_len(self) -> usize {
        match self {
            Self::Symmetric | Self::EccP256 | Self::EccK1 | Self::Ed25519 | Self::Hmac256 => 32,
            Self::Hmac224 => 28,
            Self::Hmac384 | Self::EccP384 => 48,
            Self::Hmac512 => 64,
            Self::EccP521 => 66,
            // RSA material is a PKCS #8 document of no fixed size.
            Self::Rsa2048 | Self::Rsa3072 | Self::Rsa4096 => 0,
        }
    }

    pub(crate) fn encryption_algorithms(self, usage: Usage) -> &'static [&'static str] {
        match (self, usage) {
            (Self::Symmetric, _) => &["SYMMETRIC_DEFAULT"],
            (s, Usage::EncryptDecrypt) if s.is_rsa() => &["RSAES_OAEP_SHA_1", "RSAES_OAEP_SHA_256"],
            _ => &[],
        }
    }

    pub(crate) fn signing_algorithms(self, usage: Usage) -> &'static [&'static str] {
        if self.is_rsa() {
            return if usage == Usage::SignVerify {
                &RSA_SIGNING
            } else {
                &[]
            };
        }
        match self {
            Self::EccP256 | Self::EccK1 => &["ECDSA_SHA_256"],
            Self::EccP384 => &["ECDSA_SHA_384"],
            Self::EccP521 => &["ECDSA_SHA_512"],
            Self::Ed25519 => &["ED25519_SHA_512"],
            _ => &[],
        }
    }

    pub(crate) fn mac_algorithms(self) -> &'static [&'static str] {
        match self {
            Self::Hmac224 => &["HMAC_SHA_224"],
            Self::Hmac256 => &["HMAC_SHA_256"],
            Self::Hmac384 => &["HMAC_SHA_384"],
            Self::Hmac512 => &["HMAC_SHA_512"],
            _ => &[],
        }
    }
}

impl Usage {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::EncryptDecrypt => "ENCRYPT_DECRYPT",
            Self::SignVerify => "SIGN_VERIFY",
            Self::GenerateVerifyMac => "GENERATE_VERIFY_MAC",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        [
            Self::EncryptDecrypt,
            Self::SignVerify,
            Self::GenerateVerifyMac,
        ]
        .into_iter()
        .find(|u| u.name() == name)
    }
}

/// What an RSA signing key can do: PSS and PKCS #1 v1.5, each with SHA-256, SHA-384 and SHA-512.
const RSA_SIGNING: [&str; 6] = [
    "RSASSA_PSS_SHA_256",
    "RSASSA_PSS_SHA_384",
    "RSASSA_PSS_SHA_512",
    "RSASSA_PKCS1_V1_5_SHA_256",
    "RSASSA_PKCS1_V1_5_SHA_384",
    "RSASSA_PKCS1_V1_5_SHA_512",
];
