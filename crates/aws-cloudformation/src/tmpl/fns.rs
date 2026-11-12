//! The intrinsic functions that compute rather than refer: `Fn::Join`, `Fn::Select`, `Fn::Split`, `Fn::FindInMap`, `Fn::GetAZs`,
//! `Fn::Base64`, `Fn::Cidr`, `Fn::ImportValue`, and the language extensions' `Fn::Length` and `Fn::ToJsonString`.

use serde_json::Value;

use super::eval::{Out, Scope, err, intrinsic};

impl Scope<'_> {
    pub(super) fn function(&self, name: &str, arg: &Value) -> Out {
        match name {
            "Fn::Join" => self.join(arg),
            "Fn::Select" => self.select(arg),
            "Fn::Split" => self.split(arg),
            "Fn::FindInMap" => self.find_in_map(arg),
            "Fn::GetAZs" => {
                let region = self.text(arg, name)?;
                let region = if region.is_empty() {
                    self.pseudo.region.clone()
                } else {
                    region
                };
                Ok(Some(Value::Array(
                    azs(&region).into_iter().map(Value::String).collect(),
                )))
            }
            "Fn::Base64" => {
                let text = self.text(arg, name)?;
                Ok(Some(Value::String(aws_core::base64::encode(
                    text.as_bytes(),
                ))))
            }
            "Fn::Cidr" => self.cidr(arg),
            "Fn::ImportValue" => {
                let export = self.text(arg, name)?;
                match self.exports.get(&export) {
                    Some(v) => Ok(Some(Value::String(v.clone()))),
                    // (memory)
                    None => err(format!("No export named {export} found.")),
                }
            }
            "Fn::Length" => {
                let n = match self.value(arg, name)? {
                    Value::Array(a) => a.len(),
                    _ => return err("Template error: Fn::Length requires a list"), // (memory)
                };
                Ok(Some(Value::Number(n.into())))
            }
            "Fn::ToJsonString" => {
                let v = self.value(arg, name)?;
                Ok(Some(Value::String(v.to_string())))
            }
            // (memory)
            other => err(format!(
                "Template format error: Unrecognized function: {other}"
            )),
        }
    }

    fn join(&self, arg: &Value) -> Out {
        let Some([delim, list]) = arg.as_array().map(Vec::as_slice) else {
            return err(
                "Template error: every Fn::Join object requires two parameters, (1) a string delimiter and (2) a list of strings to be joined or a function that returns a list of strings (such as Fn::GetAZs) to be joined.",
            );
        };
        let delim = self.text(delim, "Fn::Join")?;
        let items = self.list(list, "Fn::Join")?;
        let mut parts = Vec::with_capacity(items.len());
        for i in &items {
            parts.push(match i {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                // (memory)
                _ => return err("Template error: every Fn::Join object requires two parameters, (1) a string delimiter and (2) a list of strings to be joined or a function that returns a list of strings (such as Fn::GetAZs) to be joined."),
            });
        }
        Ok(Some(Value::String(parts.join(&delim))))
    }

    fn select(&self, arg: &Value) -> Out {
        let Some([index, list]) = arg.as_array().map(Vec::as_slice) else {
            return err(
                "Template error: Fn::Select requires a list argument with two elements: an integer index and a list",
            ); // (memory)
        };
        let index = self.text(index, "Fn::Select")?;
        let items = self.list(list, "Fn::Select")?;
        let Ok(i) = index.parse::<usize>() else {
            return err(
                "Template error: Fn::Select requires a list argument with two elements: an integer index and a list",
            ); // (memory)
        };
        match items.get(i) {
            Some(v) => Ok(Some(v.clone())),
            // (memory)
            None => err(format!(
                "Template error: Fn::Select cannot select nonexistent value at index {i}"
            )),
        }
    }

    fn split(&self, arg: &Value) -> Out {
        let Some([delim, source]) = arg.as_array().map(Vec::as_slice) else {
            return err(
                "Template error: every Fn::Split object requires two parameters, (1) a string delimiter and (2) a string to be split or a function that returns a string to be split.",
            ); // (memory)
        };
        let delim = self.text(delim, "Fn::Split")?;
        let source = self.text(source, "Fn::Split")?;
        if delim.is_empty() {
            return err("Template error: Fn::Split requires a non-empty delimiter"); // (memory)
        }
        Ok(Some(Value::Array(
            source
                .split(delim.as_str())
                .map(|s| Value::String(s.to_owned()))
                .collect(),
        )))
    }

    fn find_in_map(&self, arg: &Value) -> Out {
        let items = arg.as_array().cloned().unwrap_or_default();
        let (keys, default) = match items.as_slice() {
            [a, b, c] => ([a, b, c], None),
            // The language extensions add a fourth argument: `{"DefaultValue": ...}`.
            [a, b, c, d] => ([a, b, c], d.get("DefaultValue")),
            _ => {
                return err(
                    "Template error: Fn::FindInMap requires a list argument with three elements",
                );
            } // (memory)
        };
        let [map, top, second] = keys;
        let (map, top, second) = (
            self.text(map, "Fn::FindInMap")?,
            self.text(top, "Fn::FindInMap")?,
            self.text(second, "Fn::FindInMap")?,
        );
        let found = self
            .mappings
            .and_then(|m| m.get(&map))
            .and_then(|m| m.get(&top))
            .and_then(|m| m.get(&second));
        match (found, default) {
            (Some(v), _) => self.eval(v),
            (None, Some(d)) => self.eval(d),
            // (memory)
            (None, None) => err(format!(
                "Template error: Unable to get mapping for {map}::{top}::{second}"
            )),
        }
    }

    fn cidr(&self, arg: &Value) -> Out {
        let Some([block, count, bits]) = arg.as_array().map(Vec::as_slice) else {
            return err("Template error: Fn::Cidr requires a list argument with three elements"); // (memory)
        };
        let block = self.text(block, "Fn::Cidr")?;
        let count: u32 = self.text(count, "Fn::Cidr")?.parse().unwrap_or(0);
        let bits: u32 = self.text(bits, "Fn::Cidr")?.parse().unwrap_or(0);
        match cidrs(&block, count, bits) {
            Some(list) => Ok(Some(Value::Array(
                list.into_iter().map(Value::String).collect(),
            ))),
            // (memory)
            None => err(format!(
                "Template error: Fn::Cidr cannot carve {count} subnets of {bits} host bits from {block}"
            )),
        }
    }
}

/// The availability zones a region reports locally. (memory) An account sees six in us-east-1 and four in us-west-2; three
/// elsewhere is what most regions offer.
pub(crate) fn azs(region: &str) -> Vec<String> {
    let n = match region {
        "us-east-1" => 6,
        "us-west-2" => 4,
        _ => 3,
    };
    (b'a'..b'a' + n)
        .map(|c| format!("{region}{}", c as char))
        .collect()
}

/// `count` subnets with `bits` host bits each, carved in order from `block` (IPv4 or IPv6).
pub(crate) fn cidrs(block: &str, count: u32, bits: u32) -> Option<Vec<String>> {
    let (addr, prefix) = block.split_once('/')?;
    let prefix: u32 = prefix.parse().ok()?;
    let v6 = addr.contains(':');
    let width = if v6 { 128 } else { 32 };
    if prefix > width || bits == 0 || bits > width - prefix || !(1..=256).contains(&count) {
        return None;
    }
    let base: u128 = if v6 {
        u128::from(addr.parse::<std::net::Ipv6Addr>().ok()?)
    } else {
        u128::from(u32::from(addr.parse::<std::net::Ipv4Addr>().ok()?))
    };
    let new_prefix = width - bits;
    let available = 1u128.checked_shl(new_prefix - prefix).unwrap_or(u128::MAX);
    if u128::from(count) > available {
        return None;
    }
    let step = 1u128.checked_shl(bits)?;
    let mask = if prefix == 0 {
        0
    } else {
        u128::MAX << (width - prefix) & if v6 { u128::MAX } else { u128::from(u32::MAX) }
    };
    let start = base & mask;
    let mut out = Vec::new();
    for i in 0..u128::from(count) {
        let a = start + i * step;
        let text = if v6 {
            std::net::Ipv6Addr::from(a).to_string()
        } else {
            std::net::Ipv4Addr::from(u32::try_from(a).ok()?).to_string()
        };
        out.push(format!("{text}/{new_prefix}"));
    }
    Some(out)
}

/// Whether `v` holds any intrinsic call (so is not a plain literal).
pub(crate) fn has_intrinsic(v: &Value) -> bool {
    if intrinsic(v).is_some() {
        return true;
    }
    match v {
        Value::Object(o) => o.values().any(has_intrinsic),
        Value::Array(a) => a.iter().any(has_intrinsic),
        _ => false,
    }
}
