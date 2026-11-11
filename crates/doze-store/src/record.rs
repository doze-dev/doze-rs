//! How a record is written to disk: a version byte, then postcard.
//!
//! A reader checks the version before it decodes, so a record written by a newer build is refused with a
//! name instead of being read as something else, and an older version can still be read by a branch of
//! its own (see [`split`]).

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::StoreError;

/// The record: `version`, then the postcard of `value`.
///
/// # Errors
/// `Corrupt` if the value cannot be serialized.
pub fn encode<T: Serialize>(what: &str, version: u8, value: &T) -> Result<Vec<u8>, StoreError> {
    postcard::to_extend(value, vec![version])
        .map_err(|e| StoreError::Corrupt(format!("{what} cannot be written: {e}")))
}

/// The version byte and the body after it.
///
/// # Errors
/// `Corrupt` if the record is empty.
pub fn split<'a>(what: &str, bytes: &'a [u8]) -> Result<(u8, &'a [u8]), StoreError> {
    bytes
        .split_first()
        .map(|(v, body)| (*v, body))
        .ok_or_else(|| StoreError::Corrupt(format!("{what}: empty")))
}

/// Decodes a body written as `T`.
///
/// # Errors
/// `Corrupt` if the bytes are not a `T`.
pub fn body<T: DeserializeOwned>(what: &str, body: &[u8]) -> Result<T, StoreError> {
    postcard::from_bytes(body).map_err(|e| StoreError::Corrupt(format!("{what}: {e}")))
}

/// Decodes a record whose only known version is `current`.
///
/// # Errors
/// `Corrupt` if the record is empty, of another version, or not a `T`.
pub fn decode<T: DeserializeOwned>(what: &str, current: u8, bytes: &[u8]) -> Result<T, StoreError> {
    let (version, rest) = split(what, bytes)?;
    if version != current {
        return Err(StoreError::Corrupt(format!(
            "{what}: version {version} is not known"
        )));
    }
    body(what, rest)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        name: String,
        n: u32,
    }

    #[test]
    fn a_record_comes_back_whole_and_a_stranger_is_refused() {
        let sample = Sample {
            name: "q".into(),
            n: 7,
        };
        let mut bytes = encode("sample", 3, &sample).unwrap();
        assert_eq!(bytes.first(), Some(&3));
        assert_eq!(decode::<Sample>("sample", 3, &bytes).unwrap(), sample);
        bytes[0] = 9;
        assert!(decode::<Sample>("sample", 3, &bytes).is_err());
        assert!(decode::<Sample>("sample", 3, &[]).is_err());
        assert!(decode::<Sample>("sample", 3, &[3, 0xff]).is_err());
    }

    #[test]
    fn an_older_version_can_be_read_by_its_own_branch() {
        let bytes = postcard::to_extend(&7u32, vec![1]).unwrap();
        let (version, rest) = split("old", &bytes).unwrap();
        assert_eq!(version, 1);
        assert_eq!(body::<u32>("old", rest).unwrap(), 7);
    }
}
