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

    /// This set and `key`.
    #[must_use]
    pub const fn with(self, key: ApiKey) -> Self {
        Self(self.0 | Self::bit(key))
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
    /// The versions Kafka marks unstable are offered too (Kafka's internal `unstable.api.versions.enable`, off by
    /// default): InitProducerId v6 (KIP-939) in Kafka 4.4.0.
    pub unstable: bool,
    /// The APIs the broker has a handler for. The rest are neither advertised nor accepted, so a client never
    /// negotiates an API it will be refused. Everything, unless narrowed.
    pub served: ApiSet,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            sasl: false,
            telemetry: false,
            unstable: false,
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

/// A Kafka major version the broker can stand for: 1 to 4. Each stands for the newest release of that major there is a
/// recording of (`tests/data/api-versions.json`): Kafka 1.1, 2.8, 3.9 and 4.4.0 ([`KafkaMajor::release`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KafkaMajor(u8);

impl KafkaMajor {
    /// The version emulated when none is declared.
    pub const DEFAULT: KafkaMajor = KafkaMajor(4);
    /// The oldest and newest majors there is a column for.
    pub const RANGE: std::ops::RangeInclusive<u8> = 1..=4;

    /// The release this major stands for: the newest of the major the version table was recorded from.
    #[must_use]
    pub fn release(self) -> &'static str {
        match self.0 {
            1 => "1.1",
            2 => "2.8",
            3 => "3.9",
            _ => "4.4",
        }
    }

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

/// A feature a profile supports (KIP-584): its name, the levels it can be set to, and the level it is at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureLevel {
    /// The feature's name, such as `transaction.version`.
    pub name: &'static str,
    /// The lowest level supported.
    pub min: i16,
    /// The highest level supported.
    pub max: i16,
    /// The level finalized.
    pub level: i16,
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

    /// The versions this profile serves `key` at, or `None` if it does not: the stable ones, as a broker with
    /// Kafka's defaults offers them. Never below what the wire crate can decode (Kafka 4.0 dropped the oldest versions
    /// of nine APIs, KIP-896).
    #[must_use]
    pub fn range(self, key: ApiKey) -> Option<(i16, i16)> {
        self.range_with(key, false)
    }

    /// [`Profile::range`], with the versions Kafka marks unstable if `unstable`.
    fn range_with(self, key: ApiKey, unstable: bool) -> Option<(i16, i16)> {
        let row = API_TABLE.iter().find(|r| r.key == key)?;
        let (min, max) = row.offered.get(self.major.column()).copied().flatten()?;
        let max = match row.unstable_from {
            Some(first) if !unstable => max.min(first - 1),
            _ => max,
        };
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
                self.range_with(row.key, features.unstable)
                    .map(|(min, max)| Advertised {
                        key: row.key,
                        min,
                        max,
                    })
            })
            .collect();
        out.sort_by_key(|a| a.key as i16);
        out
    }

    /// Whether consumer-protocol groups (KIP-848) run in this profile: Kafka 4. Kafka 3.9 lists ConsumerGroupHeartbeat
    /// and ConsumerGroupDescribe (v0), but `group.coordinator.rebalance.protocols` is `classic` by default there, so
    /// a 3.9 broker as installed answers both with UNSUPPORTED_VERSION (recorded, `tools/old-profiles-oracle`).
    #[must_use]
    pub fn offers_consumer_groups(self) -> bool {
        self.major.get() >= 4
    }

    /// The first version of `key` this profile reads, where it lists older ones it does not: Kafka 4 lists Produce from
    /// v0 but reads it from v3 (its schema starts there, KIP-896), and closes the connection of a client that sends v0 to
    /// v2 (`UnsupportedVersionException`, as a real 4.4.0 does: `tools/old-profiles-oracle`).
    fn read_from(self, key: ApiKey) -> Option<i16> {
        (self.major.get() >= 4 && key == ApiKey::Produce).then_some(3)
    }

    /// The `transaction.version` this profile finalizes: 2 where EndTxn v5 is offered (Kafka 4, KIP-890's second part:
    /// a client may add partitions by writing to them and every end raises the epoch), else 0 (the classic protocol,
    /// which Kafka 1 to 3 run). The level also decides how the broker ends a transaction on its own (a timeout, a new
    /// owner): Kafka's coordinator uses its own level for those.
    #[must_use]
    pub fn transaction_version(self) -> i16 {
        match self.range(ApiKey::EndTxn) {
            Some((_, max)) if max >= 5 => 2,
            _ => 0,
        }
    }

    /// The `share.version` this profile finalizes: 2 where the share group APIs are offered (Kafka 4, formatted at
    /// 4.4.0's defaults), else 0.
    #[must_use]
    pub fn share_version(self) -> i16 {
        if self.range(ApiKey::ShareFetch).is_some() {
            2
        } else {
            0
        }
    }

    /// The `streams.version` this profile finalizes: 1 where the streams group APIs are offered (Kafka 4, formatted at
    /// 4.4.0's defaults, which finalize `StreamsVersion.SV_1`), else 0.
    #[must_use]
    pub fn streams_version(self) -> i16 {
        i16::from(self.range(ApiKey::StreamsGroupHeartbeat).is_some())
    }

    /// Every feature (KIP-584) the recorded broker of this major supports, with the levels it supports and the one it
    /// finalizes when formatted with its defaults (`kafka-features.sh describe`, 2026-11-19): Kafka 4.4.0 (KRaft, static
    /// voters) and 3.9.1; Kafka 2 and 1 have none. The `transaction.version`, `share.version` and `streams.version` levels
    /// are this profile's own (see [`Profile::transaction_version`]), which are those of the recorded broker.
    #[must_use]
    pub fn features(self) -> Vec<FeatureLevel> {
        let f = |name, min, max, level| FeatureLevel {
            name,
            min,
            max,
            level,
        };
        match self.major.get() {
            4 => vec![
                f("eligible.leader.replicas.version", 0, 1, 1),
                f("group.version", 0, 1, 1),
                f("kraft.version", 0, 1, 0),
                f("metadata.version", 7, 33, 33),
                f("share.version", 0, 2, self.share_version()),
                f("streams.version", 0, 1, self.streams_version()),
                f("transaction.version", 0, 2, self.transaction_version()),
            ],
            3 => vec![
                f("kraft.version", 0, 1, 0),
                f("metadata.version", 1, 21, 21),
            ],
            _ => Vec::new(),
        }
    }

    /// Refuses a request outside what was advertised: an API not offered, or a version out of range; and a version that
    /// is listed and not read (Produce v0 to v2 at Kafka 4).
    ///
    /// # Errors
    /// [`WireError::NotAdvertised`] for anything the profile does not serve.
    pub fn check(self, key: ApiKey, version: i16, features: Features) -> Result<(), WireError> {
        let offered = API_TABLE.iter().any(|r| r.key == key && features.allows(r));
        let read = self.read_from(key).is_none_or(|from| version >= from);
        match self.range_with(key, features.unstable) {
            Some((min, max)) if offered && read && (min..=max).contains(&version) => Ok(()),
            _ => Err(WireError::NotAdvertised { key, version }),
        }
    }
}

#[cfg(test)]
mod tests;
