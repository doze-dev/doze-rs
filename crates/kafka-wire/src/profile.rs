//! Version profiles: what each `--kafka-version` advertises, and what a request may ask for.

use kafka_protocol::messages::ApiKey;

use crate::api::{API_TABLE, ApiRow};
use crate::error::WireError;

/// The versions an API is offered at in one profile: `Some((min, max))`, or `None` when not offered.
pub type Offered = Option<(i16, i16)>;

/// An API offered at versions `min` to `max`.
#[must_use]
pub const fn v(min: i16, max: i16) -> Offered {
    Some((min, max))
}

/// An API a profile does not offer.
pub const NONE: Offered = None;

/// Something beyond the Kafka major that an API depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Condition {
    /// Always offered where the profile offers it.
    Always,
    /// Only when SASL authentication is configured.
    Sasl,
    /// Only when client telemetry is on.
    Telemetry,
}

/// A set of APIs, by key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiSet(u128);

impl ApiSet {
    /// Every API.
    pub const ALL: ApiSet = ApiSet(u128::MAX);

    /// Exactly these APIs.
    #[must_use]
    pub const fn of(mut keys: &[ApiKey]) -> Self {
        let mut bits = 0;
        while let [key, rest @ ..] = keys {
            bits |= Self::bit(*key);
            keys = rest;
        }
        Self(bits)
    }

    /// Whether `key` is in the set.
    #[must_use]
    pub const fn contains(self, key: ApiKey) -> bool {
        self.0 & Self::bit(key) != 0
    }

    const fn bit(key: ApiKey) -> u128 {
        let k = key as i16;
        if k >= 0 && k < 128 { 1 << k } else { 0 }
    }
}

/// What the running broker has switched on, which some APIs wait for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Features {
    /// SASL authentication is configured.
    pub sasl: bool,
    /// Client telemetry (KIP-714) is on.
    pub telemetry: bool,
    /// The APIs the broker has a handler for. The rest are neither advertised nor accepted, so a client never
    /// negotiates an API it will be refused. Everything, unless narrowed.
    pub served: ApiSet,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            sasl: false,
            telemetry: false,
            served: ApiSet::ALL,
        }
    }
}

impl Features {
    fn allows(self, row: &ApiRow) -> bool {
        self.served.contains(row.key)
            && match row.when {
                Condition::Always => true,
                Condition::Sasl => self.sasl,
                Condition::Telemetry => self.telemetry,
            }
    }
}

/// A Kafka major version the broker can stand for: 1 to 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KafkaMajor(u8);

impl KafkaMajor {
    /// The version emulated when none is declared.
    pub const DEFAULT: KafkaMajor = KafkaMajor(4);
    /// The oldest and newest majors there is a column for. The floor is record batch format v2.
    pub const RANGE: std::ops::RangeInclusive<u8> = 1..=4;

    /// A major, if it is one this broker can stand for.
    #[must_use]
    pub fn new(major: u8) -> Option<Self> {
        Self::RANGE.contains(&major).then_some(Self(major))
    }

    /// The major as a number.
    #[must_use]
    pub fn get(self) -> u8 {
        self.0
    }

    fn column(self) -> usize {
        usize::from(self.0 - Self::RANGE.start())
    }
}

/// One line of an ApiVersions answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Advertised {
    /// The API.
    pub key: ApiKey,
    /// The lowest version served.
    pub min: i16,
    /// The highest version served.
    pub max: i16,
}

/// The behaviour envelope of one declared Kafka major.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    major: KafkaMajor,
}

impl Profile {
    /// The profile of a Kafka major.
    #[must_use]
    pub fn new(major: KafkaMajor) -> Self {
        Self { major }
    }

    /// The major this profile stands for.
    #[must_use]
    pub fn major(self) -> KafkaMajor {
        self.major
    }

    /// The versions this profile serves `key` at, or `None` if it does not. Never below what the wire
    /// crate can decode (Kafka 4.0 dropped the oldest versions of nine APIs, KIP-896).
    #[must_use]
    pub fn range(self, key: ApiKey) -> Option<(i16, i16)> {
        let row = API_TABLE.iter().find(|r| r.key == key)?;
        let (min, max) = row.offered.get(self.major.column()).copied().flatten()?;
        let can = key.valid_versions();
        let (min, max) = (min.max(can.min), max.min(can.max));
        (min <= max).then_some((min, max))
    }

    /// What ApiVersions reports and the router enforces, sorted by API key.
    #[must_use]
    pub fn advertised(self, features: Features) -> Vec<Advertised> {
        let mut out: Vec<Advertised> = API_TABLE
            .iter()
            .filter(|row| features.allows(row))
            .filter_map(|row| {
                self.range(row.key).map(|(min, max)| Advertised {
                    key: row.key,
                    min,
                    max,
                })
            })
            .collect();
        out.sort_by_key(|a| a.key as i16);
        out
    }

    /// Whether the KIP-848 consumer group protocol is offered in this profile.
    #[must_use]
    pub fn offers_consumer_groups(self) -> bool {
        self.range(ApiKey::ConsumerGroupHeartbeat).is_some()
    }

    /// Refuses a request outside what was advertised: an API not offered, or a version out of range.
    ///
    /// # Errors
    /// [`WireError::NotAdvertised`] for anything the profile does not serve.
    pub fn check(self, key: ApiKey, version: i16, features: Features) -> Result<(), WireError> {
        let offered = API_TABLE.iter().any(|r| r.key == key && features.allows(r));
        match self.range(key) {
            Some((min, max)) if offered && (min..=max).contains(&version) => Ok(()),
            _ => Err(WireError::NotAdvertised { key, version }),
        }
    }
}

#[cfg(test)]
mod tests;
