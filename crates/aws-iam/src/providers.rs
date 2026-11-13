//! OpenID Connect and SAML identity providers: kept as given and returned. Nothing is fetched from them and no token of theirs is
//! validated (signatures and thumbprints are not checked: SCOPE.md, "STAY CUT").

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::errors::{exists, limit};
use crate::model::Provider;
use crate::req::{Req, page, secs, tag_list};

const MAX_CLIENT_IDS: usize = 100;

fn no_provider(arn: &str) -> ApiError {
    ApiError::modeled(
        "NoSuchEntityException",
        format!("OpenIDConnect Provider not found for arn {arn}"),
    )
}

fn no_saml(arn: &str) -> ApiError {
    // (memory)
    ApiError::modeled(
        "NoSuchEntityException",
        format!("SAMLProvider {arn} not found"),
    )
}

impl Core {
    fn provider(&self, arn: &str, saml: bool) -> Result<&Provider, ApiError> {
        self.providers
            .get(arn)
            .filter(|p| p.saml == saml)
            .ok_or_else(|| if saml { no_saml(arn) } else { no_provider(arn) })
    }

    pub(crate) fn create_oidc_provider(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let url = r.s("Url").trim_start_matches("https://").to_owned();
        let arn = format!("arn:aws:iam::{}:oidc-provider/{url}", r.account());
        if self.providers.get(&arn).is_some() {
            return Err(exists("Provider", &url));
        }
        let tags: BTreeMap<String, String> = r.tags().into_iter().collect();
        let p = Provider {
            arn: arn.clone(),
            saml: false,
            created_ms: self.now_ms(),
            url,
            client_ids: r.strings("ClientIDList"),
            thumbprints: r.strings("ThumbprintList"),
            metadata: String::new(),
            tags,
        };
        let shown = tag_list(p.tags.iter());
        self.providers.insert(arn.clone(), p);
        Ok(json!({"OpenIDConnectProviderArn": arn, "Tags": shown}))
    }

    pub(crate) fn get_oidc_provider(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let p = self.provider(r.s("OpenIDConnectProviderArn"), false)?;
        Ok(json!({
            "Url": p.url, "ClientIDList": p.client_ids, "ThumbprintList": p.thumbprints,
            "CreateDate": secs(p.created_ms), "Tags": tag_list(p.tags.iter()),
        }))
    }

    pub(crate) fn list_providers(&self, saml: bool) -> Value {
        let items: Vec<Value> = self
            .providers
            .iter()
            .filter(|(_, p)| p.saml == saml)
            .map(|(_, p)| json!({"Arn": p.arn}))
            .collect();
        json!({ if saml { "SAMLProviderList" } else { "OpenIDConnectProviderList" }: items })
    }

    pub(crate) fn delete_provider(&mut self, r: &Req<'_>, saml: bool) -> Result<Value, ApiError> {
        let arn = r.s(if saml {
            "SAMLProviderArn"
        } else {
            "OpenIDConnectProviderArn"
        });
        self.provider(arn, saml)?;
        self.providers.remove(arn);
        Ok(Value::Null)
    }

    pub(crate) fn add_client_id(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let arn = r.s("OpenIDConnectProviderArn");
        self.provider(arn, false)?;
        if let Some(p) = self.providers.get_mut(arn) {
            if p.client_ids.len() >= MAX_CLIENT_IDS {
                return Err(limit(
                    "Cannot exceed quota for ClientIdsPerOpenIdConnectProvider: 100",
                ));
            }
            if !p.client_ids.iter().any(|c| c == r.s("ClientID")) {
                p.client_ids.push(r.s("ClientID").to_owned());
            }
        }
        Ok(Value::Null)
    }

    pub(crate) fn remove_client_id(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let arn = r.s("OpenIDConnectProviderArn");
        self.provider(arn, false)?;
        if let Some(p) = self.providers.get_mut(arn) {
            p.client_ids.retain(|c| c != r.s("ClientID"));
        }
        Ok(Value::Null)
    }

    pub(crate) fn update_thumbprint(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let arn = r.s("OpenIDConnectProviderArn");
        self.provider(arn, false)?;
        let list = r.strings("ThumbprintList");
        if let Some(p) = self.providers.get_mut(arn) {
            p.thumbprints = list;
        }
        Ok(Value::Null)
    }

    pub(crate) fn create_saml_provider(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let arn = format!("arn:aws:iam::{}:saml-provider/{}", r.account(), r.s("Name"));
        if self.providers.get(&arn).is_some() {
            return Err(exists("Provider", r.s("Name")));
        }
        let p = Provider {
            arn: arn.clone(),
            saml: true,
            created_ms: self.now_ms(),
            url: String::new(),
            client_ids: Vec::new(),
            thumbprints: Vec::new(),
            metadata: r.s("SAMLMetadataDocument").to_owned(),
            tags: r.tags().into_iter().collect(),
        };
        let shown = tag_list(p.tags.iter());
        self.providers.insert(arn.clone(), p);
        Ok(json!({"SAMLProviderArn": arn, "Tags": shown}))
    }

    pub(crate) fn get_saml_provider(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let p = self.provider(r.s("SAMLProviderArn"), true)?;
        Ok(
            json!({"SAMLMetadataDocument": p.metadata, "CreateDate": secs(p.created_ms), "Tags": tag_list(p.tags.iter())}),
        )
    }

    pub(crate) fn update_saml_provider(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let arn = r.s("SAMLProviderArn");
        self.provider(arn, true)?;
        if let Some(p) = self.providers.get_mut(arn)
            && let Some(doc) = r.opt("SAMLMetadataDocument")
        {
            doc.clone_into(&mut p.metadata);
        }
        Ok(json!({"SAMLProviderArn": arn}))
    }

    /// Tag, untag and list tags of a provider; `arn_key` is the member that names it.
    pub(crate) fn tag_provider(
        &mut self,
        r: &Req<'_>,
        arn_key: &str,
        saml: bool,
    ) -> Result<Value, ApiError> {
        let arn = r.s(arn_key);
        self.provider(arn, saml)?;
        let new = r.tags();
        if let Some(p) = self.providers.get_mut(arn) {
            p.tags.extend(new);
        }
        Ok(Value::Null)
    }

    pub(crate) fn untag_provider(
        &mut self,
        r: &Req<'_>,
        arn_key: &str,
        saml: bool,
    ) -> Result<Value, ApiError> {
        let arn = r.s(arn_key);
        self.provider(arn, saml)?;
        let keys = r.strings("TagKeys");
        if let Some(p) = self.providers.get_mut(arn) {
            for k in keys {
                p.tags.remove(&k);
            }
        }
        Ok(Value::Null)
    }

    pub(crate) fn list_provider_tags(
        &self,
        r: &Req<'_>,
        arn_key: &str,
        saml: bool,
    ) -> Result<Value, ApiError> {
        let p = self.provider(r.s(arn_key), saml)?;
        let Value::Array(items) = tag_list(p.tags.iter()) else {
            return Ok(Value::Null);
        };
        page(r, items, "Tags")
    }
}
