//! Converging on a start (Go's boot guard, `runtime.go`): a module instance whose structure was never converged in
//! this data folder (a fresh provision, a first converge that failed), or whose declaration changed since it was, is
//! converged as soon as its backend is ready, before anything that needs it starts. Its databases, users and the rest
//! then exist after `up` without an `apply`.
//!
//! The marker is Go's: `.doze-converged` in the data folder, holding the declaration's fingerprint. Go's fingerprint
//! hashes its own encoding of the spec, so a folder Go converged reads as changed once, and is converged (converge
//! does only what is missing) and marked again.

use crate::engine::Engine;
use crate::error::Error;
use crate::stack::{Kind as SvcKind, PlanCtx, Service, Structure};
use crate::sync::CONVERGED_MARKER;
use std::path::Path;

/// A module instance's structure, converged once its backend is ready when this data folder never converged it or its
/// declaration changed since ([`on_start`]). A failure stops the processes and taints the instance, as Go's
/// boot did: a database without its users must not serve.
pub(crate) async fn after_ready(
    e: &Engine,
    svc: &Service,
    ctx: &PlanCtx,
    unit: crate::exec::Unit,
) -> Result<crate::exec::Unit, Error> {
    let SvcKind::Planned(p) = &svc.kind else {
        return Ok(unit);
    };
    let Some(s) = p.structure() else {
        return Ok(unit);
    };
    let Err(msg) = crate::converge::on_start(s, ctx).await else {
        e.update(|c| c.m.table_mut().set_tainted(&svc.name, false));
        return Ok(unit);
    };
    let mut unit = unit;
    unit.stop(crate::exec::STOP_GRACE).await;
    e.update(|c| c.m.table_mut().set_tainted(&svc.name, true));
    Err(Error::Failed(format!(
        "provisioning \"{}\": {msg}",
        svc.name
    )))
}

/// Converges `s` in `ctx` when its marker is missing or names another fingerprint, then writes the marker. `Err` is
/// the module's reason; the marker is then removed, so the next start tries again.
pub(crate) async fn on_start(s: &dyn Structure, ctx: &PlanCtx) -> Result<(), String> {
    let fp = s.fingerprint();
    if converged(&ctx.data_dir).as_deref() == Some(fp.as_str()) {
        return Ok(());
    }
    match s.converge(ctx).await {
        Ok(()) => {
            mark(&ctx.data_dir, &fp);
            Ok(())
        }
        Err(e) => {
            clear(&ctx.data_dir);
            Err(e)
        }
    }
}

/// The fingerprint the last converge in `data_dir` recorded; `None` when it never converged there.
pub(crate) fn converged(data_dir: &Path) -> Option<String> {
    std::fs::read_to_string(data_dir.join(CONVERGED_MARKER))
        .ok()
        .map(|s| s.trim().to_string())
}

/// Records a converge of the declaration `fp` (as Go writes it: the fingerprint, no newline).
pub(crate) fn mark(data_dir: &Path, fp: &str) {
    if let Err(e) = std::fs::write(data_dir.join(CONVERGED_MARKER), fp) {
        tracing::warn!(
            "could not record the converge in {}: {e}",
            data_dir.display()
        );
    }
}

/// Forgets the converge (a failed one, a destroy).
pub(crate) fn clear(data_dir: &Path) {
    let _ = std::fs::remove_file(data_dir.join(CONVERGED_MARKER));
}
