//! CloudWatch: metric alarms, the metrics that have data, and dashboards.

mod alarms;
mod dashboards;
mod metrics;

use super::Descriptor;

pub(super) static CLOUDWATCH: Descriptor = Descriptor {
    service: "cloudwatch",
    target: "monitoring",
    kinds: &[alarms::ALARMS, metrics::METRICS, dashboards::DASHBOARDS],
};
