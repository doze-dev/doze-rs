//! Function URLs: the configuration, and finding the function a URL id belongs to. Serving the requests is `web.rs`.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::config::stamp;
use crate::core::{Core, Out};
use crate::errors::{conflict, invalid, not_found};
use crate::functions::{LATEST, page};
use crate::model::Url;
use crate::req::Req;

/// The host part of the address a URL is served at: `<id>.lambda-url.<region>.<endpoint host>`, which `*.localhost` resolves to the
/// loopback in browsers and curl; the path form `/_aws/lambda-url/<region>/<id>/` serves the same URL to a client that cannot.
pub(crate) fn address(endpoint: &str, id: &str, region: &str) -> String {
    let rest = endpoint.trim_start_matches("http://").trim_start_matches("https://");
    let (host, port) = rest.rsplit_once(':').map_or((rest, ""), |(h, p)| (h, p));
    let local = host == "localhost" || host == "127.0.0.1" || host == "[::1]" || host.parse::<std::net::IpAddr>().is_ok();
    let suffix = if local { "localhost" } else { host };
    let port = if port.is_empty() { String::new() } else { format!(":{port}") };
    format!("http://{id}.lambda-url.{region}.{suffix}{port}/")
}

impl Core {
    fn url_view(&self, r: &Req, name: &str, u: &Url) -> Value {
        let qualifier = (!u.qualifier.is_empty()).then_some(u.qualifier.as_str());
        let mut out = Map::new();
        out.insert("FunctionUrl".into(), json!(address(&r.caller.endpoint, &u.id, r.region())));
        out.insert("FunctionArn".into(), json!(self.arn(r, name, qualifier)));
        out.insert("AuthType".into(), json!(u.auth));
        if let Some(c) = u.cors.as_deref().and_then(|c| serde_json::from_str::<Value>(c).ok()) {
            out.insert("Cors".into(), c);
        }
        out.insert("CreationTime".into(), json!(u.created));
        out.insert("LastModifiedTime".into(), json!(u.modified));
        out.insert("InvokeMode".into(), json!(u.invoke_mode));
        Value::Object(out)
    }

    /// The alias a URL request names: none, or an alias that exists (`$LATEST` is not a qualifier a URL takes).
    fn url_qualifier(&self, r: &Req) -> Result<String, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        match f.qualifier.as_deref() {
            None => Ok(String::new()),
            Some(LATEST) => Err(invalid("The qualifier must be an alias name.")),
            Some(q) if function.aliases.iter().any(|a| a.name == q) => Ok(q.to_owned()),
            Some(q) => Err(not_found(format!(
                "Function not found: {}",
                self.arn(r, &f.name, Some(q))
            ))),
        }
    }

    pub(crate) fn create_url(&mut self, r: &Req) -> Result<Out, ApiError> {
        let q = self.url_qualifier(r)?;
        let f = r.function()?;
        let now = stamp(self.now_ms());
        let id = crate::functions::uuid()?.replace('-', "");
        let url = Url {
            qualifier: q.clone(),
            id,
            auth: r.text("AuthType").to_owned(),
            invoke_mode: r.opt("InvokeMode").unwrap_or("BUFFERED").to_owned(),
            cors: r.value("Cors").map(Value::to_string),
            created: now.clone(),
            modified: now,
        };
        let function = self.find_mut(r, &f)?;
        if function.urls.iter().any(|u| u.qualifier == q) {
            return Err(conflict("Failed to create function url config for [functionArn = ".to_owned()
                + &crate::arn::function(r.region(), r.account(), &f.name, None)
                + "]. Error message:  FunctionUrlConfig exists for this Lambda function"));
        }
        function.urls.push(url.clone());
        Ok(Out { json: self.url_view(r, &f.name, &url), status: Some(201) })
    }

    fn url_of(&self, r: &Req) -> Result<(String, Url), ApiError> {
        let q = self.url_qualifier(r)?;
        let f = r.function()?;
        let function = self.find(r, &f)?;
        function
            .urls
            .iter()
            .find(|u| u.qualifier == q)
            .map(|u| (f.name.clone(), u.clone()))
            .ok_or_else(|| not_found("The resource you requested does not exist."))
    }

    pub(crate) fn get_url(&self, r: &Req) -> Result<Out, ApiError> {
        let (name, url) = self.url_of(r)?;
        Ok(self.url_view(r, &name, &url).into())
    }

    pub(crate) fn update_url(&mut self, r: &Req) -> Result<Out, ApiError> {
        let (name, mut url) = self.url_of(r)?;
        if let Some(a) = r.opt("AuthType") {
            a.clone_into(&mut url.auth);
        }
        if let Some(m) = r.opt("InvokeMode") {
            m.clone_into(&mut url.invoke_mode);
        }
        if let Some(c) = r.value("Cors") {
            url.cors = Some(c.to_string());
        }
        url.modified = stamp(self.now_ms());
        let f = r.function()?;
        let function = self.find_mut(r, &f)?;
        if let Some(slot) = function.urls.iter_mut().find(|u| u.qualifier == url.qualifier) {
            *slot = url.clone();
        }
        Ok(self.url_view(r, &name, &url).into())
    }

    pub(crate) fn delete_url(&mut self, r: &Req) -> Result<Out, ApiError> {
        let (_, url) = self.url_of(r)?;
        let f = r.function()?;
        self.find_mut(r, &f)?.urls.retain(|u| u.qualifier != url.qualifier);
        Ok(Out { json: json!({}), status: Some(204) })
    }

    pub(crate) fn list_urls(&self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let rows = function.urls.iter().map(|u| self.url_view(r, &f.name, u)).collect();
        Ok(page(r, rows, "FunctionUrlConfigs", |v| {
            v.get("FunctionArn").and_then(Value::as_str).unwrap_or_default().to_owned()
        })
        .into())
    }

    /// The function a URL id belongs to, with what serving it needs.
    pub(crate) fn url_target(&self, id: &str) -> Option<Value> {
        self.functions.iter().find_map(|(name, f)| {
            let u = f.urls.iter().find(|u| u.id == id)?;
            Some(json!({"name": name, "qualifier": u.qualifier, "auth": u.auth, "cors": u.cors,
                "invokeMode": u.invoke_mode, "region": self.region}))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_endpoint_gets_a_localhost_url() {
        assert_eq!(
            address("http://127.0.0.1:4566", "abc", "us-east-1"),
            "http://abc.lambda-url.us-east-1.localhost:4566/"
        );
        assert_eq!(
            address("http://aws.harbour.doze:80", "abc", "eu-west-1"),
            "http://abc.lambda-url.eu-west-1.aws.harbour.doze:80/"
        );
    }
}
