//! `GET /api/v1/about` and `GET /api/v1/services`: what this doze-aws is, and how much of each service is built.

use std::collections::BTreeSet;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;

use super::json;
use crate::facts::ServiceFacts;
use crate::model::{About, ApiCoverage, ServiceName, ServiceRow, ServicesResponse};
use crate::state::Shared;

/// The most calls looked at for the regions of [`About`]: the whole ring.
const ALL: usize = usize::MAX;

/// `GET /api/v1/about`.
pub async fn about(State(s): State<Shared>) -> Response {
    let f = &s.facts;
    let mut regions: BTreeSet<String> = s
        .recorder
        .newest(ALL)
        .calls
        .iter()
        .filter(|c| !c.region.is_empty())
        .map(|c| c.region.to_string())
        .collect();
    regions.insert(f.region.clone());
    json(
        StatusCode::OK,
        &About {
            name: "doze-aws".to_owned(),
            version: f.version.clone(),
            account: f.account.clone(),
            region: f.region.clone(),
            regions: regions.into_iter().collect(),
            endpoint: f.endpoint.clone(),
            console: s.url(),
            auth: f.auth.clone(),
            iam: f.iam.clone(),
            data_dir: f.data_dir.clone(),
            pid: f.pid,
            started: f.started,
            services: f
                .services
                .iter()
                .map(|v| ServiceName {
                    name: v.name.to_owned(),
                    title: v.title.to_owned(),
                    served: v.served,
                })
                .collect(),
        },
    )
}

/// One service's row: its operations counted on the services themselves, as `doze-aws compat` counts them.
#[must_use]
pub fn row(service: &ServiceFacts) -> ServiceRow {
    let apis: Vec<ApiCoverage> = service
        .apis
        .iter()
        .map(|api| {
            let (meta, tables) = (api.meta(), api.tables());
            let (mut built, mut total) = (0, 0);
            while tables.op(total).is_some() {
                built += u32::from(api.serves(total));
                total += 1;
            }
            ApiCoverage {
                sdk_id: meta.sdk_id.to_owned(),
                signing_name: meta.signing_name.to_owned(),
                built,
                total,
            }
        })
        .collect();
    ServiceRow {
        name: service.name.to_owned(),
        title: service.title.to_owned(),
        group: crate::resources::catalog::ALL
            .iter()
            .find(|e| e.service == service.name)
            .map_or_else(String::new, |e| e.group.to_owned()),
        served: service.served,
        built: apis.iter().map(|a| a.built).sum(),
        total: apis.iter().map(|a| a.total).sum(),
        apis,
    }
}

/// `GET /api/v1/services`, in the Resources index's order: a group's services together.
pub async fn services(State(s): State<Shared>) -> Response {
    let place = |name: &str| {
        crate::resources::catalog::ALL
            .iter()
            .position(|e| e.service == name)
            .unwrap_or(usize::MAX)
    };
    let mut services: Vec<ServiceRow> = s.facts.services.iter().map(row).collect();
    services.sort_by_key(|r| place(&r.name));
    json(StatusCode::OK, &ServicesResponse { services })
}
