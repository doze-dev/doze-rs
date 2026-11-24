//! The view-model of the region switcher (`api/regions.rs`).

use serde::Serialize;

/// `GET /api/v1/regions`: the regions of AWS the console can look at, and the one it looks at when a request names none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RegionsView {
    /// The default region (`--region`).
    pub default: String,
    /// Every region of AWS, the default first, then the ones that hold something, then the rest by name.
    pub regions: Vec<RegionRow>,
}

/// One region.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RegionRow {
    /// `eu-west-1`.
    pub code: String,
    /// `Europe (Ireland)`.
    pub name: String,
    /// Whether a service keeps something on disk for it (the default region always counts).
    pub has_data: bool,
}
