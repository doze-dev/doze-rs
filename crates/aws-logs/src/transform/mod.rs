//! Log transformers (`PutTransformer`, `TestTransformer`): a pipeline of processors applied to each event as it is ingested, as the
//! model documents (`Processor` and its members). The event is a JSON object: the message itself when it is one, otherwise empty
//! until a parser fills it (memory). A parser with no `source` (or `@message`) reads the raw message and writes at the root or under
//! its `destination`; keys are dot paths (`store.book`). A processor that cannot apply leaves the event as it is and the pipeline
//! goes on; an event the pipeline leaves empty is not transformed. (memory: the user guide's processor pages are not in the model.)
//!
//! Built: `addKeys`, `copyValue`, `csv`, `dateTimeConverter`, `deleteKeys`, `grok`, `listToMap`, `lowerCaseString`, `moveKeys`,
//! `parseJSON`, `parseKeyValue`, `parseVPC`, `parseWAF`, `renameKeys`, `splitString`, `substituteString`, `trimString`,
//! `typeConverter`, `upperCaseString`. Refused with a sentence: `parseCloudfront`, `parseRoute53`, `parsePostgres`, `parseToOCSF`
//! (logs vended by AWS services that do not run locally).

mod apply;
mod datetime;
mod grok;
mod path;
mod procs;

pub(crate) use procs::invalid;

#[cfg(test)]
mod tests;

use aws_wire::ApiError;
use serde_json::{Map, Value};

use procs::Proc;

/// What a transformer may copy into an event (`copyValue` from `@logGroupName`, `@logGroupStream`, `@accountId`, `@regionName`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Meta<'a> {
    pub(crate) group: &'a str,
    pub(crate) stream: &'a str,
    pub(crate) account: &'a str,
    pub(crate) region: &'a str,
}

/// A checked transformer.
#[derive(Clone, Debug)]
pub(crate) struct Transformer {
    procs: Vec<Proc>,
}

impl Transformer {
    /// Checks a `transformerConfig` (the request's JSON array of processors).
    pub(crate) fn parse(config: &Value) -> Result<Self, ApiError> {
        Ok(Self {
            procs: procs::read(config)?,
        })
    }

    /// The event as the transformer leaves it (JSON text), or `None` when it cannot be transformed (a processor that fails
    /// leaves the event as it came, memory).
    pub(crate) fn apply(&self, message: &str, meta: &Meta<'_>) -> Option<String> {
        self.test(message, meta).ok().flatten()
    }

    /// As [`Self::apply`], but a processor's failure is the error `TestTransformer` answers.
    pub(crate) fn test(&self, message: &str, meta: &Meta<'_>) -> Result<Option<String>, String> {
        let obj = match serde_json::from_str::<Value>(message) {
            Ok(Value::Object(m)) => m,
            _ => Map::new(),
        };
        let mut event = apply::Event {
            message,
            meta,
            obj,
            failed: None,
        };
        for p in &self.procs {
            apply::run(p, &mut event);
            if let Some(why) = event.failed.take() {
                return Err(why);
            }
        }
        Ok((!event.obj.is_empty()).then(|| Value::Object(event.obj).to_string()))
    }
}

/// Whether `config_json` is a transformer that gives a transformed event for `message`.
pub(crate) fn check(config_json: &str, message: &str) -> bool {
    let Ok(config) = serde_json::from_str::<Value>(config_json) else {
        return false;
    };
    let meta = Meta {
        group: "g",
        stream: "s",
        account: "000000000000",
        region: "us-east-1",
    };
    Transformer::parse(&config).is_ok_and(|t| t.apply(message, &meta).is_some())
}
