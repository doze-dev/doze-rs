//! `AWS::Serverless-2016-10-31` (SAM). Written in milestone 3.

use serde_json::Value;

use aws_wire::ApiError;

use crate::tmpl::eval::Pseudo;

/// Expands the SAM resources of `t` into plain CloudFormation.
///
/// # Errors
/// What SAM refuses.
pub(crate) fn apply(_t: &mut Value, _pseudo: &Pseudo) -> Result<(), ApiError> {
    Ok(())
}
