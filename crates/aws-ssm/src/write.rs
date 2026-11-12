//! Writing parameters: `PutParameter`, `DeleteParameter`, `DeleteParameters`.
//!
//! Words that AWS gave when the scenarios were recorded are used as they were. Where it was not recorded, the comment says so:
//! "(memory)" is from the author's recollection of AWS's text and has not been checked against it.

use std::collections::BTreeSet;

use aws_model_ssm::keys::{
    DeleteParameterRequest as Delete, DeleteParametersRequest as DeleteMany,
    DeleteParametersResult as Deleted, PutParameterRequest as Put, PutParameterResult as Put_, Tag,
};
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};

use crate::core::{Core, empty, reply, request, validation};
use crate::model::{Kind, Parameter, Policy, Version};
use crate::{name, policy};

const STANDARD_BYTES: usize = 4096;
const ADVANCED_BYTES: usize = 8192;
const MAX_TAGS: usize = 50;

pub(crate) fn not_found() -> ApiError {
    // Recorded: AWS's message is empty.
    ApiError::modeled("ParameterNotFound", "")
}

/// What a write asks for, before the parameter's earlier settings fill in what it left out.
struct Asked<'a> {
    name: &'a str,
    value: &'a str,
    kind: Option<Kind>,
    key_id: Option<&'a str>,
    description: Option<&'a str>,
    pattern: Option<&'a str>,
    data_type: Option<&'a str>,
    tier: Option<&'a str>,
    policies: Option<&'a str>,
    tags: Vec<(String, String)>,
    overwrite: bool,
    by: String,
}

impl Core {
    pub(crate) fn put_parameter(&mut self, call: &Call) -> Reply {
        let v = request::<Put>(call)?;
        let name = v.get(Put::NAME).unwrap_or_default().trim();
        name::check_write(name)?;
        let kind = match v.get(Put::TYPE) {
            Some(text) => Some(
                Kind::parse(text)
                    .ok_or_else(|| validation(format!("Type {text} is not supported.")))?,
            ),
            None => None,
        };
        let tags = v
            .get(Put::TAGS)
            .map(|tags| {
                tags.map(|t| {
                    (
                        t.get(Tag::KEY).unwrap_or_default().to_owned(),
                        t.get(Tag::VALUE).unwrap_or_default().to_owned(),
                    )
                })
                .collect()
            })
            .unwrap_or_default();
        self.write(&Asked {
            name,
            value: v.get(Put::VALUE).unwrap_or_default(),
            kind,
            key_id: v.get(Put::KEY_ID),
            description: v.get(Put::DESCRIPTION),
            pattern: v.get(Put::ALLOWED_PATTERN),
            data_type: v.get(Put::DATA_TYPE),
            tier: v.get(Put::TIER),
            policies: v.get(Put::POLICIES),
            tags,
            overwrite: v.get(Put::OVERWRITE).unwrap_or(false),
            by: format!("arn:aws:iam::{}:root", call.caller.account_id),
        })
    }

    fn write(&mut self, a: &Asked<'_>) -> Reply {
        let before = self.params.get(a.name);
        if before.is_some() && !a.overwrite {
            return Err(ApiError::modeled(
                "ParameterAlreadyExists",
                "The parameter already exists. To overwrite this value, set the overwrite option in the request to true.",
            ));
        }
        if before.is_some() && !a.tags.is_empty() {
            // (memory)
            return Err(validation(
                "Invalid request: tags and overwrite can't be used together. To create a parameter with tags, please remove overwrite flag. To update tags for an existing parameter, please use AddTagsToResource or RemoveTagsFromResource.",
            ));
        }
        if a.tags.len() > MAX_TAGS {
            return Err(ApiError::modeled(
                "TooManyTagsError",
                "The Tags list has too many entries.",
            ));
        }
        let parsed = a.policies.map(policy::parse).transpose()?;
        let version = self.next_version(a, before.map(|p| &p.latest), parsed.as_ref())?;
        let (number, advanced) = (version.number, version.advanced);
        let expires = match &parsed {
            Some(p) => p.expires_ms,
            None => before.and_then(|p| p.expires_ms),
        };
        self.store(a.name, version, &a.tags, expires)?;
        reply::<Put_>(|o| {
            o.put_int(Put_::VERSION, number);
            o.put(Put_::TIER, if advanced { "Advanced" } else { "Standard" });
        })
    }

    /// The version a write makes: the settings it gave, and the earlier version's for those it left out (not recorded for
    /// each; the description and the policies are the ones a caller would notice).
    fn next_version(
        &self,
        a: &Asked<'_>,
        prev: Option<&Version>,
        parsed: Option<&policy::Parsed>,
    ) -> Result<Version, ApiError> {
        let kind = match (a.kind, prev) {
            (Some(kind), _) => kind,
            (None, Some(prev)) => prev.kind,
            (None, None) => {
                return Err(validation(
                    "A parameter type is required when you create a parameter.",
                ));
            }
        };
        let policies: Vec<Policy> = match parsed {
            Some(p) => p.policies.clone(),
            None => prev.map(|p| p.policies.clone()).unwrap_or_default(),
        };
        let advanced = resolve_tier(
            a.tier,
            prev.is_some_and(|p| p.advanced),
            a.value.len(),
            !policies.is_empty(),
        )?;
        let pattern = a
            .pattern
            .map(str::to_owned)
            .or_else(|| prev.and_then(|p| p.allowed_pattern.clone()));
        check_value(a.value, advanced, pattern.as_deref())?;
        let data_type = match a.data_type {
            Some(t @ ("text" | "aws:ec2:image")) => t.to_owned(),
            // (memory)
            Some(other) => {
                return Err(validation(format!(
                    "The following data type is not supported: {other} (Data type names are all lowercase.)"
                )));
            }
            None => prev.map_or_else(|| "text".to_owned(), |p| p.data_type.clone()),
        };
        let value = if kind == Kind::SecureString {
            self.sealer
                .seal(a.value.as_bytes())
                .ok_or_else(|| ApiError::internal("the value could not be sealed"))?
        } else {
            a.value.as_bytes().to_vec()
        };
        let key_id = (kind == Kind::SecureString).then(|| {
            a.key_id
                .map(str::to_owned)
                .or_else(|| prev.and_then(|p| p.key_id.clone()))
                .unwrap_or_else(|| "alias/aws/ssm".to_owned())
        });
        Ok(Version {
            number: prev.map_or(1, |p| p.number + 1),
            value,
            kind,
            key_id,
            description: a
                .description
                .map(str::to_owned)
                .or_else(|| prev.and_then(|p| p.description.clone())),
            allowed_pattern: pattern,
            data_type,
            advanced,
            policies,
            labels: BTreeSet::new(),
            modified_ms: self.now_ms(),
            modified_by: a.by.clone(),
        })
    }

    fn store(
        &mut self,
        name: &str,
        version: Version,
        tags: &[(String, String)],
        expires: Option<i64>,
    ) -> Result<(), ApiError> {
        if let Some(param) = self.params.get_mut(name) {
            let oldest = param.older.first().map_or(0, |v| v.number);
            param.push(version).map_err(|()| {
                // (memory)
                ApiError::modeled("ParameterMaxVersionLimitExceeded", format!("You attempted to create a new version of {name} by calling the PutParameter API with the overwrite flag. Version {oldest}, the oldest version, can't be deleted because it has a label associated with it. Move the label to another version of the parameter, and try again."))
            })?;
            param.expires_ms = expires;
        } else {
            self.params.insert(
                name.to_owned(),
                Parameter {
                    latest: version,
                    older: Vec::new(),
                    tags: tags.iter().cloned().collect(),
                    expires_ms: expires,
                },
            );
        }
        if let Some(at) = expires {
            self.expires_at(at);
        }
        Ok(())
    }

    pub(crate) fn delete_parameter(&mut self, call: &Call) -> Reply {
        let v = request::<Delete>(call)?;
        let name = v.get(Delete::NAME).unwrap_or_default();
        self.params.remove(name).ok_or_else(not_found)?;
        empty()
    }

    pub(crate) fn delete_parameters(&mut self, call: &Call) -> Reply {
        let v = request::<DeleteMany>(call)?;
        let names: Vec<&str> = v
            .get(DeleteMany::NAMES)
            .map(Iterator::collect)
            .unwrap_or_default();
        let (mut gone, mut invalid) = (Vec::new(), Vec::new());
        for name in names {
            if self.params.remove(name).is_some() {
                gone.push(name);
            } else {
                invalid.push(name);
            }
        }
        reply::<Deleted>(|o| {
            o.put_list(Deleted::DELETED_PARAMETERS, |l| {
                for n in &gone {
                    l.push(n);
                }
            });
            o.put_list(Deleted::INVALID_PARAMETERS, |l| {
                for n in &invalid {
                    l.push(n);
                }
            });
        })
    }
}

/// Whether a write is of the advanced tier. An advanced parameter cannot go back; `Intelligent-Tiering` picks advanced when
/// the value or the policies need it.
fn resolve_tier(
    asked: Option<&str>,
    was_advanced: bool,
    bytes: usize,
    policies: bool,
) -> Result<bool, ApiError> {
    match asked {
        Some("Advanced") => Ok(true),
        Some("Standard") if was_advanced => Err(validation(
            "An advanced parameter can't be changed back to the standard tier.",
        )),
        Some("Standard") | None if policies && !was_advanced => {
            // The model's documentation: standard parameters can't use policies.
            Err(validation(
                "Parameter policies can only be used with advanced-tier parameters. Set the Tier to Advanced.",
            ))
        }
        Some("Standard") => Ok(false),
        Some("Intelligent-Tiering") => Ok(was_advanced || bytes > STANDARD_BYTES || policies),
        Some(other) => Err(validation(format!("Tier {other} is not supported."))),
        None => Ok(was_advanced),
    }
}

/// A value fits its tier and its pattern.
fn check_value(value: &str, advanced: bool, pattern: Option<&str>) -> Result<(), ApiError> {
    let limit = if advanced {
        ADVANCED_BYTES
    } else {
        STANDARD_BYTES
    };
    if value.len() > limit {
        // (memory)
        return Err(validation(if advanced {
            "Advanced tier parameters support a maximum parameter value of 8192 characters."
        } else {
            "Standard tier parameters support a maximum parameter value of 4096 characters. To create a larger parameter value, upgrade the parameter to use the advanced-parameter tier."
        }));
    }
    pattern.map_or(Ok(()), |p| check_pattern(p, value))
}

fn check_pattern(pattern: &str, value: &str) -> Result<(), ApiError> {
    let re = regex_lite::Regex::new(pattern).map_err(|_| {
        ApiError::modeled(
            "InvalidAllowedPatternException",
            format!("The request AllowedPattern isn't a valid regular expression: {pattern}"),
        )
    })?;
    if re.is_match(value) {
        Ok(())
    } else {
        // (memory)
        Err(ApiError::modeled(
            "ParameterPatternMismatchException",
            format!("Parameter value doesn't match the allowed pattern {pattern}."),
        ))
    }
}
