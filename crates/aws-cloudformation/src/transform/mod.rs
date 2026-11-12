//! The transforms a template may declare: `AWS::Include` (snippets from S3), `AWS::LanguageExtensions` (`Fn::ForEach`, `Fn::Length`,
//! `Fn::ToJsonString`, the default of `Fn::FindInMap`, intrinsics in `DeletionPolicy` and `UpdateReplacePolicy`) and
//! `AWS::Serverless-2016-10-31` (SAM). They run in the order the template declares them; any other transform (a macro) is refused.

pub(crate) mod include;
pub(crate) mod langext;
pub(crate) mod sam;

use std::collections::BTreeMap;

use serde_json::Value;

use aws_wire::ApiError;
use aws_wire::service::Caller;

use crate::core::Core;
use crate::errors::validation;
use crate::tmpl::eval::Pseudo;

/// SAM's transform.
pub(crate) const SAM: &str = "AWS::Serverless-2016-10-31";
/// The language extensions.
pub(crate) const LANGUAGE_EXTENSIONS: &str = "AWS::LanguageExtensions";
/// Snippets from S3.
pub(crate) const INCLUDE: &str = "AWS::Include";

/// The template after its transforms.
///
/// # Errors
/// An unknown transform, or what a transform refuses.
pub(crate) async fn apply(
    original: &Value,
    core: &Core,
    caller: &Caller,
    params: &BTreeMap<String, Value>,
    pseudo: &Pseudo,
) -> Result<Value, ApiError> {
    let names = crate::tmpl::transforms(original);
    if let Some(other) = names
        .iter()
        .find(|n| ![SAM, LANGUAGE_EXTENSIONS, INCLUDE].contains(&n.as_str()))
    {
        // (memory) A macro is not served locally.
        return Err(validation(format!(
            "Template format error: Unrecognized transform: {other}. doze-aws runs AWS::Serverless-2016-10-31, AWS::LanguageExtensions and AWS::Include; macros are not served"
        )));
    }
    let mut t = original.clone();
    if let Some(o) = t.as_object_mut() {
        o.remove("Transform");
    }
    include::expand(&mut t, core.peers.as_deref(), caller, params, 0).await?;
    for name in &names {
        match name.as_str() {
            LANGUAGE_EXTENSIONS => langext::apply(&mut t, params, pseudo)?,
            SAM => sam::apply(&mut t, pseudo)?,
            _ => {}
        }
    }
    Ok(t)
}
