//! doze's own routes under `/_doze/`: not Confluent paths, and listed as such (docs/port/behaviours/registry.md). They ask
//! for the same credentials as every route.

use std::sync::PoisonError;

use serde::Serialize;

use super::route::Route;
use super::{Api, Call, Response, text};
use crate::error::RegistryError;
use crate::format::CompatReason;

#[derive(Serialize)]
struct Finding {
    version: u32,
    level: &'static str,
    reasons: Vec<CompatReason>,
}

#[derive(Serialize)]
struct Check {
    compatible: bool,
    level: String,
    compared: Vec<u32>,
    findings: Vec<Finding>,
}

impl Api {
    pub(super) fn extension(
        &self,
        route: Route,
        call: &Call<'_>,
    ) -> Result<Response, RegistryError> {
        let r = &self.registry;
        let subject = call.subject();
        Ok(match route {
            Route::DozeOverview => Response::ok(&r.overview()),
            Route::DozeRequests => {
                let before = call
                    .query("before")
                    .and_then(|b| b.parse::<u64>().ok())
                    .unwrap_or(0);
                let limit = call
                    .query("limit")
                    .as_deref()
                    .and_then(text::atoi)
                    .unwrap_or(0);
                let wanted = call.query("subject").unwrap_or_default();
                let ids = if wanted.is_empty() {
                    Vec::new()
                } else {
                    r.ids_of(&wanted)
                };
                let log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
                Response::ok(&log.page(&wanted, &ids, before, limit))
            }
            Route::DozeVersion => Response::ok(&r.version_info(subject, call.version())?),
            Route::DozeResolved => Response::ok(&r.resolved(subject, call.version())?),
            Route::DozeCheck => {
                // A check at any level, the subject's left as it is, each reason taken apart.
                let input = super::confluent::read_input(call)?;
                let (version, level) = (call.query("version"), call.query("level"));
                let (compared, found) = r.check(
                    subject,
                    version.as_deref(),
                    &input,
                    call.flag("normalize"),
                    level.as_deref(),
                )?;
                Response::ok(&Check {
                    compatible: found.is_empty(),
                    level: r.level_for(subject, level.as_deref()),
                    compared,
                    findings: found
                        .into_iter()
                        .map(|f| Finding {
                            version: f.version,
                            level: f.level.name(),
                            reasons: f.details,
                        })
                        .collect(),
                })
            }
            _ => return Err(RegistryError::http(404, "HTTP 404 Not Found")),
        })
    }
}
