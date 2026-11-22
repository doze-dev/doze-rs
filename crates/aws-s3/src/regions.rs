//! The Region names S3 knows, from the model's list for a bucket's location constraint.

use aws_model_s3::keys::CreateBucketConfiguration;
use aws_model_s3::tables::TABLES;

/// The AWS Region names, sorted: the model's location constraints (every Region but `us-east-1`, which has none, and the legacy `EU`)
/// and `us-east-1`. Built when asked, so a process that never validates a Region holds no list.
#[must_use]
pub fn known() -> Vec<&'static str> {
    let set = TABLES
        .member(CreateBucketConfiguration::LOCATION_CONSTRAINT.member())
        .and_then(|m| TABLES.cons(m.cons))
        .map_or(0, |c| c.enum_set);
    let mut all: Vec<&'static str> = TABLES
        .enum_values(set)
        .into_iter()
        .filter(|r| r.contains('-'))
        .chain(["us-east-1"])
        .collect();
    all.sort_unstable();
    all.dedup();
    all
}

#[cfg(test)]
mod tests {
    use super::known;

    #[test]
    fn the_models_regions_are_listed_with_the_home_region() {
        let all = known();
        for r in [
            "us-east-1",
            "eu-west-2",
            "ap-southeast-2",
            "us-gov-west-1",
            "mx-central-1",
        ] {
            assert!(all.contains(&r), "{r} in {all:?}");
        }
        assert!(!all.contains(&"EU"), "{all:?}");
        assert!(all.len() > 30, "{all:?}");
    }
}
