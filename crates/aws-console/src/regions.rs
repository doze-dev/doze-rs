//! The regions of the console: the AWS regions it can look at, the ones that hold something on disk, and the one a request is for.
//!
//! doze-aws itself answers for any region name (each region is kept apart, and one nobody uses costs nothing). The console is
//! narrower on purpose: it offers the regions AWS has, in one list, and looks at one at a time: the one a request names in
//! `X-Console-Region`, else the default (`--region`).

use std::collections::BTreeSet;
use std::path::Path;

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

use crate::api::error;

/// The regions of AWS's commercial partition, with the names the AWS console gives them (GovCloud and China are partitions of their own,
/// with accounts of their own).
pub const REGIONS: &[(&str, &str)] = &[
    ("us-east-1", "US East (N. Virginia)"),
    ("us-east-2", "US East (Ohio)"),
    ("us-west-1", "US West (N. California)"),
    ("us-west-2", "US West (Oregon)"),
    ("af-south-1", "Africa (Cape Town)"),
    ("ap-east-1", "Asia Pacific (Hong Kong)"),
    ("ap-east-2", "Asia Pacific (Taipei)"),
    ("ap-northeast-1", "Asia Pacific (Tokyo)"),
    ("ap-northeast-2", "Asia Pacific (Seoul)"),
    ("ap-northeast-3", "Asia Pacific (Osaka)"),
    ("ap-south-1", "Asia Pacific (Mumbai)"),
    ("ap-south-2", "Asia Pacific (Hyderabad)"),
    ("ap-southeast-1", "Asia Pacific (Singapore)"),
    ("ap-southeast-2", "Asia Pacific (Sydney)"),
    ("ap-southeast-3", "Asia Pacific (Jakarta)"),
    ("ap-southeast-4", "Asia Pacific (Melbourne)"),
    ("ap-southeast-5", "Asia Pacific (Malaysia)"),
    ("ap-southeast-7", "Asia Pacific (Thailand)"),
    ("ca-central-1", "Canada (Central)"),
    ("ca-west-1", "Canada West (Calgary)"),
    ("eu-central-1", "Europe (Frankfurt)"),
    ("eu-central-2", "Europe (Zurich)"),
    ("eu-north-1", "Europe (Stockholm)"),
    ("eu-south-1", "Europe (Milan)"),
    ("eu-south-2", "Europe (Spain)"),
    ("eu-west-1", "Europe (Ireland)"),
    ("eu-west-2", "Europe (London)"),
    ("eu-west-3", "Europe (Paris)"),
    ("il-central-1", "Israel (Tel Aviv)"),
    ("me-central-1", "Middle East (UAE)"),
    ("me-south-1", "Middle East (Bahrain)"),
    ("mx-central-1", "Mexico (Central)"),
    ("sa-east-1", "South America (Sao Paulo)"),
];

/// The region the services keep in their own directory (not under `regions/`).
const HOME: &str = "us-east-1";

/// Whether `code` is a region of AWS.
#[must_use]
pub fn is_region(code: &str) -> bool {
    REGIONS.iter().any(|(c, _)| *c == code)
}

/// The regions with something on disk under `data_dir`: a service's `regions/<name>` directory, and the home region when a service holds
/// files of its own. Nothing is opened or called.
#[must_use]
pub fn with_data(data_dir: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let Ok(services) = std::fs::read_dir(data_dir) else {
        return found;
    };
    for service in services.filter_map(Result::ok) {
        let Ok(inside) = std::fs::read_dir(service.path()) else {
            continue;
        };
        for entry in inside.filter_map(Result::ok) {
            if entry.file_name() == "regions" {
                if let Ok(regions) = std::fs::read_dir(entry.path()) {
                    found.extend(
                        regions
                            .filter_map(Result::ok)
                            .filter(|r| r.path().is_dir())
                            .filter_map(|r| r.file_name().into_string().ok())
                            .filter(|n| is_region(n)),
                    );
                }
            } else {
                found.insert(HOME.to_owned());
            }
        }
    }
    found
}

tokio::task_local! {
    /// The region the request being answered is for (set by [`select`] when it names one).
    static REGION: String;
}

/// The region of the request being answered: the one it named, else `default`.
#[must_use]
pub fn current(default: &str) -> String {
    REGION
        .try_with(Clone::clone)
        .unwrap_or_else(|_| default.to_owned())
}

/// A layer: a request that names a region (`X-Console-Region`) is answered for it; one that names a region AWS does not have is refused.
pub async fn select(req: Request, next: Next) -> Response {
    let asked = req
        .headers()
        .get("x-console-region")
        .map(|v| v.to_str().map(str::to_owned));
    match asked {
        None => next.run(req).await,
        Some(Ok(region)) if is_region(&region) => REGION.scope(region, next.run(req)).await,
        Some(_) => error(
            StatusCode::BAD_REQUEST,
            "UNKNOWN_REGION",
            "X-Console-Region names a region AWS does not have.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_regions_of_aws_are_regions() {
        assert!(is_region("us-east-1"));
        assert!(is_region("ap-southeast-7"));
        assert!(!is_region("not-a-region-9"));
        assert!(!is_region("us-gov-west-1"));
        assert!(!is_region(""));
        let mut codes: Vec<&str> = REGIONS.iter().map(|(c, _)| *c).collect();
        let n = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), n, "a region is listed twice");
    }

    #[test]
    fn the_regions_with_data_are_the_ones_a_service_keeps_files_for() {
        let dir = std::env::temp_dir().join(format!("doze-regions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sqs/regions/eu-west-1")).unwrap();
        std::fs::create_dir_all(dir.join("sqs/regions/not-a-region-9")).unwrap();
        std::fs::create_dir_all(dir.join("s3/regions/ap-south-1")).unwrap();
        // Nothing of the home region's own: only its `regions` directory.
        assert_eq!(
            with_data(&dir).into_iter().collect::<Vec<_>>(),
            vec!["ap-south-1".to_owned(), "eu-west-1".to_owned()]
        );
        // A service's own file is the home region's.
        std::fs::write(dir.join("sqs/sqs.redb"), b"x").unwrap();
        assert!(with_data(&dir).contains("us-east-1"));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(with_data(&dir).is_empty());
    }

    #[tokio::test]
    async fn a_request_is_for_the_region_it_names() {
        assert_eq!(current("us-east-1"), "us-east-1");
        let inside = REGION
            .scope("eu-west-1".to_owned(), async { current("us-east-1") })
            .await;
        assert_eq!(inside, "eu-west-1");
    }
}
