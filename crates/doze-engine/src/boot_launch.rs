//! A start's processes launched, and launched again on another port when the one the engine chose was taken before the
//! process could bind it (`crate::ports::lost`).

use super::prepare;
use crate::engine::Engine;
use crate::error::Error;
use crate::logs::RunLog;
use crate::stack::{PlanCtx, Planned, Service};

/// The plan made and its processes started and ready: made again, planned afresh, when the port the engine chose was
/// taken before the process could bind it. That race is the engine's, not the app's (the failed run stays in the log,
/// saying "address in use").
pub(super) async fn launch(
    e: &Engine,
    svc: &Service,
    ctx: &PlanCtx,
) -> Result<(Planned, RunLog, crate::pidfile::Pids, crate::exec::Unit), Error> {
    let mut lost = 0;
    loop {
        let (planned, log) = prepare(e, svc, ctx).await?;
        let pids = e.pids(&svc.name);
        match crate::exec::start(&planned.plan, &log, planned.ready_within, Some(&pids)).await {
            Ok(u) => return Ok((planned, log, pids, u)),
            Err(err) => {
                pids.remove();
                log.close().await;
                if let Some(port) = err.lost_port.filter(|_| lost < LOST_PORT_RETRIES) {
                    lost += 1;
                    tracing::warn!(
                        "\"{}\": port {port} was taken before it could listen there; starting it again on another",
                        svc.name
                    );
                    continue;
                }
                let kind = if err.timeout {
                    Error::Timeout
                } else {
                    Error::Failed
                };
                return Err(kind(err.message));
            }
        }
    }
}

/// How many times one start is made again because the port the engine chose was taken before its process could bind
/// it ([`crate::ports::lost`]). Each time is a fresh port, so a second loss is already rare.
const LOST_PORT_RETRIES: u32 = 3;
