//! Layers: published archives that a function's runtime finds on its search paths.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::arn;
use crate::config::stamp;
use crate::core::{Core, Out, Staged};
use crate::errors::{conflict, invalid, not_found};
use crate::functions::{page, uuid};
use crate::model::{Layer, LayerVersion};
use crate::req::Req;

/// A layer name from a name or an ARN.
fn name_of(text: &str) -> &str {
    match text.split(':').collect::<Vec<_>>().as_slice() {
        ["arn", _, "lambda", _, _, "layer", name, ..] => name,
        _ => text,
    }
}

fn version_view(r: &Req, name: &str, v: &LayerVersion) -> Value {
    let mut out = Map::new();
    out.insert(
        "LayerVersionArn".into(),
        json!(arn::layer(r.region(), r.account(), name, Some(v.number))),
    );
    out.insert("Version".into(), json!(v.number));
    out.insert("Description".into(), json!(v.description));
    out.insert("CreatedDate".into(), json!(stamp(v.created_ms)));
    out.insert("CompatibleRuntimes".into(), json!(v.runtimes));
    out.insert("CompatibleArchitectures".into(), json!(v.architectures));
    if let Some(l) = &v.license {
        out.insert("LicenseInfo".into(), json!(l));
    }
    Value::Object(out)
}

fn full_view(r: &Req, name: &str, v: &LayerVersion) -> Value {
    let mut out = version_view(r, name, v);
    if let Value::Object(o) = &mut out {
        o.insert(
            "LayerArn".into(),
            json!(arn::layer(r.region(), r.account(), name, None)),
        );
        o.insert(
            "Content".into(),
            json!({
                "Location": format!("{}/_aws/lambda/layer/{}/{name}/{}", r.caller.endpoint, r.region(), v.number),
                "CodeSha256": v.code.sha256,
                "CodeSize": v.code.size,
            }),
        );
    }
    out
}

impl Core {
    pub(crate) fn publish_layer(&mut self, r: &Req, staged: &Staged) -> Result<Out, ApiError> {
        let name = name_of(r.text("LayerName")).to_owned();
        let code = staged
            .code
            .clone()
            .ok_or_else(|| invalid("Please provide a source for the layer content."))?;
        let now = self.now_ms();
        let list = |k: &str| -> Vec<String> {
            r.value(k)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        let layer = self.layers.get(&name).cloned().unwrap_or(Layer {
            name: name.clone(),
            versions: Vec::new(),
            next_version: 1,
        });
        let mut layer = layer;
        let version = LayerVersion {
            number: layer.next_version,
            description: r.opt("Description").unwrap_or_default().to_owned(),
            runtimes: list("CompatibleRuntimes"),
            architectures: list("CompatibleArchitectures"),
            license: r.opt("LicenseInfo").map(str::to_owned),
            code,
            created_ms: now,
            statements: std::collections::BTreeMap::new(),
        };
        layer.next_version += 1;
        let out = full_view(r, &name, &version);
        layer.versions.push(version);
        self.layers.insert(name, layer);
        Ok(Out {
            json: out,
            status: Some(201),
        })
    }

    fn layer_version(&self, r: &Req, name: &str, number: i64) -> Result<&LayerVersion, ApiError> {
        let missing = || {
            not_found(format!(
                "Layer Version Not Found: {}",
                arn::layer(r.region(), r.account(), name, u32::try_from(number).ok())
            ))
        };
        self.layers
            .get(name)
            .and_then(|l| l.versions.iter().find(|v| i64::from(v.number) == number))
            .ok_or_else(missing)
    }

    pub(crate) fn get_layer_version(&self, r: &Req) -> Result<Out, ApiError> {
        let name = name_of(r.text("LayerName"));
        let v = self.layer_version(r, name, r.int("VersionNumber").unwrap_or(0))?;
        Ok(full_view(r, name, v).into())
    }

    pub(crate) fn get_layer_version_by_arn(&self, r: &Req) -> Result<Out, ApiError> {
        let text = r.text("Arn");
        let (_, _, name, number) = arn::split_layer(text)
            .ok_or_else(|| invalid(format!("Invalid layer version ARN: {text}")))?;
        let v = self.layer_version(r, name, i64::from(number))?;
        Ok(full_view(r, name, v).into())
    }

    fn compatible(v: &LayerVersion, runtime: Option<&str>, arch: Option<&str>) -> bool {
        runtime.is_none_or(|x| v.runtimes.iter().any(|y| y == x))
            && arch.is_none_or(|x| v.architectures.iter().any(|y| y == x))
    }

    pub(crate) fn list_layers(&self, r: &Req) -> Result<Out, ApiError> {
        let (rt, arch) = (r.opt("CompatibleRuntime"), r.opt("CompatibleArchitecture"));
        let mut rows = Vec::new();
        let mut names: Vec<&str> = self.layers.iter().map(|(n, _)| n).collect();
        names.sort_unstable();
        for n in names {
            let Some(l) = self.layers.get(n) else {
                continue;
            };
            if let Some(v) = l
                .versions
                .iter()
                .rev()
                .find(|v| Self::compatible(v, rt, arch))
            {
                rows.push(
                    json!({"LayerName": n, "LayerArn": arn::layer(r.region(), r.account(), n, None),
                    "LatestMatchingVersion": version_view(r, n, v)}),
                );
            }
        }
        Ok(page(r, rows, "Layers", |v| {
            v.get("LayerName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
        .into())
    }

    pub(crate) fn list_layer_versions(&self, r: &Req) -> Result<Out, ApiError> {
        let name = name_of(r.text("LayerName"));
        let (rt, arch) = (r.opt("CompatibleRuntime"), r.opt("CompatibleArchitecture"));
        let rows: Vec<Value> = self
            .layers
            .get(name)
            .map(|l| {
                l.versions
                    .iter()
                    .rev()
                    .filter(|v| Self::compatible(v, rt, arch))
                    .map(|v| version_view(r, name, v))
                    .collect()
            })
            .unwrap_or_default();
        // Newest first: the marker is the version number of the last row sent, so the page is cut by position.
        let max = usize::try_from(r.int("MaxItems").unwrap_or(50))
            .unwrap_or(50)
            .clamp(1, 50);
        let start = r
            .opt("Marker")
            .and_then(|m| m.parse::<usize>().ok())
            .unwrap_or(0);
        let page_rows: Vec<Value> = rows.iter().skip(start).take(max).cloned().collect();
        let mut out = Map::new();
        if start + max < rows.len() {
            out.insert("NextMarker".into(), json!((start + max).to_string()));
        }
        out.insert("LayerVersions".into(), Value::Array(page_rows));
        Ok(Value::Object(out).into())
    }

    pub(crate) fn delete_layer_version(&mut self, r: &Req) -> Result<Out, ApiError> {
        let name = name_of(r.text("LayerName")).to_owned();
        let number = r.int("VersionNumber").unwrap_or(0);
        if let Some(l) = self.layers.get_mut(&name) {
            l.versions.retain(|v| i64::from(v.number) != number);
        }
        self.sweep_code();
        Ok(Out {
            json: json!({}),
            status: Some(204),
        })
    }

    fn layer_version_mut(&mut self, r: &Req) -> Result<&mut LayerVersion, ApiError> {
        let name = name_of(r.text("LayerName")).to_owned();
        let number = r.int("VersionNumber").unwrap_or(0);
        self.layer_version(r, &name, number)?;
        self.layers
            .get_mut(&name)
            .and_then(|l| {
                l.versions
                    .iter_mut()
                    .find(|v| i64::from(v.number) == number)
            })
            .ok_or_else(|| not_found("Layer Version Not Found"))
    }

    pub(crate) fn add_layer_permission(&mut self, r: &Req) -> Result<Out, ApiError> {
        let name = name_of(r.text("LayerName")).to_owned();
        let resource = arn::layer(
            r.region(),
            r.account(),
            &name,
            u32::try_from(r.int("VersionNumber").unwrap_or(0)).ok(),
        );
        let sid = r.text("StatementId").to_owned();
        let principal = if r.text("Principal") == "*" {
            json!("*")
        } else {
            json!({"AWS": format!("arn:aws:iam::{}:root", r.text("Principal"))})
        };
        let mut s = json!({"Sid": sid, "Effect": "Allow", "Principal": principal, "Action": r.text("Action"), "Resource": resource});
        if let Some(org) = r.opt("OrganizationId") {
            crate::set(
                &mut s,
                "Condition",
                json!({"StringEquals": {"aws:PrincipalOrgID": org}}),
            );
        }
        let v = self.layer_version_mut(r)?;
        if v.statements.contains_key(&sid) {
            return Err(conflict(format!(
                "The statement id ({sid}) provided already exists. Please provide a new statement id, or remove the existing statement."
            )));
        }
        v.statements.insert(sid, s.to_string());
        Ok(Out {
            json: json!({"Statement": s.to_string(), "RevisionId": uuid()?}),
            status: Some(201),
        })
    }

    pub(crate) fn remove_layer_permission(&mut self, r: &Req) -> Result<Out, ApiError> {
        let sid = r.text("StatementId").to_owned();
        let v = self.layer_version_mut(r)?;
        if v.statements.remove(&sid).is_none() {
            return Err(not_found(format!(
                "Statement {sid} is not found in resource policy."
            )));
        }
        Ok(Out {
            json: json!({}),
            status: Some(204),
        })
    }

    pub(crate) fn get_layer_policy(&self, r: &Req) -> Result<Out, ApiError> {
        let name = name_of(r.text("LayerName"));
        let v = self.layer_version(r, name, r.int("VersionNumber").unwrap_or(0))?;
        if v.statements.is_empty() {
            return Err(not_found(
                "No policy is associated with the given resource.",
            ));
        }
        let statements: Vec<Value> = v
            .statements
            .values()
            .filter_map(|s| serde_json::from_str(s).ok())
            .collect();
        let doc = json!({"Version": "2012-10-17", "Id": "default", "Statement": statements});
        Ok(json!({"Policy": doc.to_string(), "RevisionId": uuid()?}).into())
    }

    /// The unpacked directories of the layers a function names, in the order listed; a layer that is not kept here is `None`.
    pub(crate) fn layer_dirs(&self, layers: &[String]) -> Vec<Result<std::path::PathBuf, String>> {
        layers
            .iter()
            .map(|a| {
                let found = arn::split_layer(a).and_then(|(_, _, name, n)| {
                    self.layers
                        .get(name)?
                        .versions
                        .iter()
                        .find(|v| v.number == n)
                        .map(|v| self.code_root(&v.code))
                });
                found.ok_or_else(|| a.clone())
            })
            .collect()
    }
}
