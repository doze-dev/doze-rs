//! Moving a staging label between versions: `UpdateSecretVersionStage`.
//!
//! A label names one version. `AWSCURRENT` cannot be dropped, only moved; when it moves, the version that held it takes
//! `AWSPREVIOUS` from whichever had it. The words of the refusals are not recorded.

use aws_model_secrets_manager::keys::{
    UpdateSecretVersionStageRequest as Stage, UpdateSecretVersionStageResponse as Staged,
};
use aws_wire::ApiError;
use aws_wire::handler::{reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::{Core, invalid_parameter, not_found};
use crate::model::{CURRENT, PREVIOUS, Secret};

fn no_version(id: &str) -> ApiError {
    ApiError::modeled(
        "ResourceNotFoundException",
        format!("Secrets Manager can't find the specified secret value for VersionId: {id}"),
    )
}

impl Core {
    pub(crate) fn update_secret_version_stage(&mut self, call: &Call) -> Reply {
        let v = request::<Stage>(call)?;
        let stage = v.get(Stage::VERSION_STAGE).unwrap_or_default();
        let (from, to) = (
            v.get(Stage::REMOVE_FROM_VERSION_ID),
            v.get(Stage::MOVE_TO_VERSION_ID),
        );
        let name = self.locate_live(v.get(Stage::SECRET_ID).unwrap_or_default())?;
        let now = self.now_ms();
        let secret = self.secrets.get_mut(&name).ok_or_else(not_found)?;
        move_stage(secret, stage, from, to)?;
        secret.changed_ms = now;
        let arn = secret.arn.clone();
        reply::<Staged>(|o| {
            o.put(Staged::ARN, &arn);
            o.put(Staged::NAME, &name);
        })
    }
}

fn move_stage(
    secret: &mut Secret,
    stage: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<(), ApiError> {
    if from.is_none() && to.is_none() {
        return Err(invalid_parameter(
            "You must specify MoveToVersionId, RemoveFromVersionId, or both.",
        ));
    }
    if stage == CURRENT && to.is_none() {
        return Err(invalid_parameter(
            "You can't remove AWSCURRENT from a version without moving it to another one.",
        ));
    }
    if let Some(id) = from {
        let holds = secret
            .versions
            .get(id)
            .ok_or_else(|| no_version(id))?
            .stages
            .contains(stage);
        if !holds {
            return Err(invalid_parameter(format!(
                "The staging label {stage} is not attached to version {id}."
            )));
        }
    }
    if let Some(id) = to
        && !secret.versions.contains_key(id)
    {
        return Err(no_version(id));
    }
    let holder = secret.staged(stage).map(|(id, _)| id.to_owned());
    if let Some(id) = from.or(if to.is_some() {
        holder.as_deref()
    } else {
        None
    }) && let Some(version) = secret.versions.get_mut(id)
    {
        version.stages.remove(stage);
    }
    let Some(target) = to else {
        return Ok(());
    };
    for version in secret.versions.values_mut() {
        version.stages.remove(stage);
    }
    if let Some(version) = secret.versions.get_mut(target) {
        version.stages.insert(stage.to_owned());
    }
    if stage == CURRENT
        && let Some(old) = holder.filter(|old| old != target)
    {
        for version in secret.versions.values_mut() {
            version.stages.remove(PREVIOUS);
        }
        if let Some(version) = secret.versions.get_mut(&old) {
            version.stages.insert(PREVIOUS.to_owned());
        }
    }
    Ok(())
}
