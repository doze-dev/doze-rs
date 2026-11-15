//! `AssumeRoleWithSAML`'s trust in an assertion, as AWS gives it: the `PrincipalArn` must name a SAML provider of the account; the
//! response must carry an XML signature (of the `Response`, the `Assertion`, or both) that verifies under a certificate in the
//! provider's metadata document ([`crate::xmldsig`]); and the assertion's issuer must be the metadata's `entityID`. Only the signed
//! assertion is read. Nothing is accepted unsigned: a local identity provider registers its own metadata (its signing certificate)
//! with `CreateSAMLProvider`, exactly as with AWS.
//!
//! (memory) The refusals are the model's `InvalidIdentityTokenException`; the sentences are doze's, built around what failed, as no
//! refusal of AWS's has been recorded.

use aws_wire::ApiError;

use crate::core::Core;
use crate::saml::{self, Assertion};

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::modeled("InvalidIdentityTokenException", message)
}

impl Core {
    /// The assertion of a base64 SAML response, once its signature has verified under the certificate of SAML provider
    /// `provider_arn`.
    pub(crate) fn signed_assertion(
        &self,
        provider_arn: &str,
        base64: &str,
    ) -> Result<Assertion, ApiError> {
        let provider = self
            .providers
            .get(provider_arn)
            .filter(|p| p.saml)
            .ok_or_else(|| {
                invalid(format!(
                    "The SAML provider {provider_arn} does not exist in this account."
                ))
            })?;
        let metadata = crate::xmldsig::signing_certs(&provider.metadata).map_err(|e| {
            invalid(format!(
                "The metadata document of SAML provider {provider_arn} gives no signing certificate to check the assertion with: {}",
                e.message()
            ))
        })?;
        let xml = saml::decode(base64).ok_or_else(|| {
            invalid("The SAML assertion could not be read: it must be base64 of a SAML response.")
        })?;
        let signed = crate::xmldsig::verify(&xml, &metadata.certs).map_err(|e| {
            invalid(format!(
                "The SAML response's signature could not be verified with the certificate of {provider_arn}: {}",
                e.message()
            ))
        })?;
        let assertion = saml::read(&signed)
            .ok_or_else(|| invalid("The signed SAML assertion could not be read."))?;
        if assertion.issuer != metadata.entity_id {
            return Err(invalid(format!(
                "The assertion's issuer {} is not the entity {} that SAML provider {provider_arn} describes.",
                assertion.issuer, metadata.entity_id
            )));
        }
        Ok(assertion)
    }
}
