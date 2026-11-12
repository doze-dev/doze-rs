//! Mobile push: platform applications and the device endpoints registered with them. They are **stored and nothing is sent** (decision D1):
//! a message published to an endpoint is accepted and gets an id. The wording of the refusals is from memory except where marked.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::arn;
use crate::core::Core;
use crate::model::{Application, Endpoint};
use crate::req::{Req, invalid, invalid_because, not_found};
use crate::topics::page;

const PLATFORMS: &[&str] = &[
    "ADM",
    "APNS",
    "APNS_SANDBOX",
    "BAIDU",
    "GCM",
    "MPNS",
    "WNS",
    "MACOS",
    "MACOS_SANDBOX",
];

/// The attributes an application may have.
const APP_ATTRS: &[&str] = &[
    "PlatformCredential",
    "PlatformPrincipal",
    "EventEndpointCreated",
    "EventEndpointDeleted",
    "EventEndpointUpdated",
    "EventDeliveryFailure",
    "SuccessFeedbackRoleArn",
    "FailureFeedbackRoleArn",
    "SuccessFeedbackSampleRate",
    "ApplePlatformTeamID",
    "ApplePlatformBundleID",
    "AuthenticationMethod",
];

fn no_application() -> ApiError {
    not_found("PlatformApplication does not exist")
}

fn no_endpoint() -> ApiError {
    not_found("Endpoint does not exist")
}

fn check_app_attrs(attrs: &BTreeMap<String, String>) -> Result<(), ApiError> {
    match attrs.keys().find(|k| !APP_ATTRS.contains(&k.as_str())) {
        Some(k) => Err(invalid_because(
            "Attributes",
            format!("Invalid attribute: {k}"),
        )),
        None => Ok(()),
    }
}

fn check_endpoint_attrs(attrs: &BTreeMap<String, String>) -> Result<(), ApiError> {
    for (k, v) in attrs {
        let ok = match k.as_str() {
            "Enabled" => matches!(v.as_str(), "true" | "false"),
            "CustomUserData" | "Token" => true,
            _ => {
                return Err(invalid_because(
                    "Attributes",
                    format!("Invalid attribute: {k}"),
                ));
            }
        };
        if !ok {
            return Err(invalid_because(
                "Attributes",
                "Enabled must be true or false",
            ));
        }
    }
    Ok(())
}

impl Core {
    /// `CreatePlatformApplication`: the same name and platform again is the same application, with the attributes laid over it.
    pub(crate) fn create_platform_application(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (name, platform) = (r.s("Name"), r.s("Platform"));
        let attrs = r.map("Attributes");
        if !PLATFORMS.contains(&platform) {
            return Err(invalid_because(
                "Platform",
                format!("Unsupported platform: {platform}"),
            ));
        }
        if name.is_empty() || name.len() > 256 {
            return Err(invalid("Name"));
        }
        check_app_attrs(&attrs)?;
        let arn = arn::application(r.region(), r.account(), platform, name);
        if let Some(existing) = self.apps.get_mut(&arn) {
            existing.attrs.extend(attrs);
            return Ok(json!({"PlatformApplicationArn": arn}));
        }
        if !attrs.contains_key("PlatformCredential") {
            return Err(invalid_because(
                "Attributes",
                "PlatformCredential is required",
            ));
        }
        let app = Application {
            arn: arn.clone(),
            name: name.to_owned(),
            platform: platform.to_owned(),
            attrs,
        };
        self.apps.insert(arn.clone(), app);
        Ok(json!({"PlatformApplicationArn": arn}))
    }

    fn application(&self, text: &str, r: &Req<'_>) -> Result<&Application, ApiError> {
        let parts = arn::split(text, "PlatformApplicationArn")?;
        if parts.region != r.region() {
            return Err(no_application());
        }
        self.apps.get(text).ok_or_else(no_application)
    }

    /// What an application shows: its attributes without the credential, and that it is enabled.
    fn application_view(app: &Application) -> BTreeMap<String, String> {
        let mut out: BTreeMap<String, String> = app
            .attrs
            .iter()
            .filter(|(k, _)| k.as_str() != "PlatformCredential")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        out.entry("Enabled".into()).or_insert_with(|| "true".into());
        out
    }

    /// `GetPlatformApplicationAttributes`.
    pub(crate) fn get_application_attributes(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let app = self.application(r.s("PlatformApplicationArn"), r)?;
        Ok(json!({"Attributes": Self::application_view(app)}))
    }

    /// `SetPlatformApplicationAttributes`.
    pub(crate) fn set_application_attributes(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let key = self
            .application(r.s("PlatformApplicationArn"), r)?
            .arn
            .clone();
        let attrs = r.map("Attributes");
        check_app_attrs(&attrs)?;
        if let Some(app) = self.apps.get_mut(&key) {
            app.attrs.extend(attrs);
        }
        Ok(Value::Null)
    }

    /// `ListPlatformApplications`.
    pub(crate) fn list_platform_applications(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let all: Vec<Value> = self
            .apps
            .iter()
            .map(|(_, a)| json!({"PlatformApplicationArn": a.arn, "Attributes": Self::application_view(a)}))
            .collect();
        let (shown, next) = page(&all, r.opt("NextToken"))?;
        Ok(json!({"PlatformApplications": shown, "NextToken": next}))
    }

    /// `DeletePlatformApplication`: its endpoints go with it; one that is not there is not an error.
    pub(crate) fn delete_platform_application(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let text = r.s("PlatformApplicationArn");
        arn::split(text, "PlatformApplicationArn")?;
        let gone: Vec<String> = self
            .endpoints
            .iter()
            .filter(|(_, e)| e.application == text)
            .map(|(k, _)| k.to_owned())
            .collect();
        for key in gone {
            self.endpoints.remove(&key);
        }
        self.apps.remove(text);
        Ok(Value::Null)
    }

    /// `CreatePlatformEndpoint`: the same token again is the same endpoint, if the attributes agree (the sentence is AWS's, from memory).
    pub(crate) fn create_platform_endpoint(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let app = self.application(r.s("PlatformApplicationArn"), r)?.clone();
        let (token, mut attrs) = (r.s("Token"), r.map("Attributes"));
        if token.is_empty() {
            return Err(invalid("Token"));
        }
        if let Some(data) = r.opt("CustomUserData") {
            attrs.insert("CustomUserData".into(), data.to_owned());
        }
        check_endpoint_attrs(&attrs)?;
        if let Some(same) = self
            .endpoints
            .iter()
            .map(|(_, e)| e)
            .find(|e| e.application == app.arn && e.token == token)
        {
            let shown = Self::endpoint_view(same);
            return if attrs.iter().all(|(k, v)| shown.get(k) == Some(v)) {
                Ok(json!({"EndpointArn": same.arn}))
            } else {
                Err(invalid_because(
                    "Token",
                    format!(
                        "Endpoint {} already exists with the same Token, but different attributes.",
                        same.arn
                    ),
                ))
            };
        }
        let id = aws_seal::uuid_v4().unwrap_or_default();
        let arn = arn::endpoint(r.region(), r.account(), &app.platform, &app.name, &id);
        let endpoint = Endpoint {
            arn: arn.clone(),
            application: app.arn,
            token: token.to_owned(),
            attrs,
        };
        self.endpoints.insert(arn.clone(), endpoint);
        Ok(json!({"EndpointArn": arn}))
    }

    fn endpoint_view(e: &Endpoint) -> BTreeMap<String, String> {
        let mut out = e.attrs.clone();
        out.insert("Token".into(), e.token.clone());
        out.entry("Enabled".into()).or_insert_with(|| "true".into());
        out
    }

    /// `GetEndpointAttributes`.
    pub(crate) fn get_endpoint_attributes(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let e = self
            .endpoints
            .get(r.s("EndpointArn"))
            .ok_or_else(no_endpoint)?;
        Ok(json!({"Attributes": Self::endpoint_view(e)}))
    }

    /// `SetEndpointAttributes`.
    pub(crate) fn set_endpoint_attributes(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let attrs = r.map("Attributes");
        check_endpoint_attrs(&attrs)?;
        let e = self
            .endpoints
            .get_mut(r.s("EndpointArn"))
            .ok_or_else(no_endpoint)?;
        for (k, v) in attrs {
            if k == "Token" {
                e.token = v;
            } else {
                e.attrs.insert(k, v);
            }
        }
        Ok(Value::Null)
    }

    /// `DeleteEndpoint`: one that is not there is not an error.
    pub(crate) fn delete_endpoint(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        arn::split(r.s("EndpointArn"), "EndpointArn")?;
        self.endpoints.remove(r.s("EndpointArn"));
        Ok(Value::Null)
    }

    /// `ListEndpointsByPlatformApplication`.
    pub(crate) fn list_endpoints(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let app = self
            .application(r.s("PlatformApplicationArn"), r)?
            .arn
            .clone();
        let all: Vec<Value> = self
            .endpoints
            .iter()
            .map(|(_, e)| e)
            .filter(|e| e.application == app)
            .map(|e| json!({"EndpointArn": e.arn, "Attributes": Self::endpoint_view(e)}))
            .collect();
        let (shown, next) = page(&all, r.opt("NextToken"))?;
        Ok(json!({"Endpoints": shown, "NextToken": next}))
    }
}
