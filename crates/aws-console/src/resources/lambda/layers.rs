//! Lambda's layers: the account's layers, each with its versions. A layer version never changes; a new one is published, and an
//! old one deleted.

use super::super::Format::{Json, Mono, Number, Text as Plain, Time};
use super::super::{Deep, Id, Kind, List, Step, Sub, Text, col, field, op, ops};

/// What the calls of one layer name.
const LAYER: &str = r#"{"LayerName":"{id}"}"#;

pub(super) const LAYERS: Kind = Kind {
    id: "layers",
    title: "Layers",
    one: "layer",
    about: "Layers in this account. A layer is a .zip file archive that contains supplementary code or data.",
    members: &["LayerName"],
    list: List {
        step: Step::new("ListLayers", r#"{"MaxItems":50,"Marker":"{token}"}"#),
        items: "Layers",
        item_as: "LayerName",
        token_out: "NextMarker",
        id: "LayerName",
        name: Text::at("LayerName"),
        label: None,
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        col("Latest version", "LatestMatchingVersion.Version", Number),
        col("Description", "LatestMatchingVersion.Description", Plain),
        col(
            "Compatible runtimes",
            "LatestMatchingVersion.CompatibleRuntimes",
            Json,
        ),
        col(
            "Compatible architectures",
            "LatestMatchingVersion.CompatibleArchitectures",
            Json,
        ),
        col("Created", "LatestMatchingVersion.CreatedDate", Time),
        col("ARN", "LayerArn", Mono),
    ],
    id_of: Id::Name,
    detail: &[Step::new(
        "ListLayerVersions",
        r#"{"LayerName":"{id}","MaxItems":50}"#,
    )],
    fields: &[
        field("Latest version", "LayerVersions.0.Version", Number),
        field("Description", "LayerVersions.0.Description", Plain),
        field(
            "Compatible runtimes",
            "LayerVersions.0.CompatibleRuntimes",
            Json,
        ),
        field(
            "Compatible architectures",
            "LayerVersions.0.CompatibleArchitectures",
            Json,
        ),
        field("License", "LayerVersions.0.LicenseInfo", Plain),
        field("Created", "LayerVersions.0.CreatedDate", Time),
        field(
            "Latest version ARN",
            "LayerVersions.0.LayerVersionArn",
            Mono,
        ),
    ],
    subs: &[Sub {
        title: "Versions",
        step: Step::new("ListLayerVersions", r#"{"LayerName":"{id}","MaxItems":50}"#),
        items: "LayerVersions",
        item_as: "Version",
        columns: &[
            col("Version", "Version", Number),
            col("Description", "Description", Plain),
            col("Created", "CreatedDate", Time),
            col("ARN", "LayerVersionArn", Mono),
        ],
        link: None,
        more: "NextMarker",
        add: None,
    }],
    deep: &[Deep {
        id: "versions",
        title: "Versions",
        ops: &[
            op("ListLayerVersions", LAYER),
            op("DeleteLayerVersion", LAYER),
        ],
    }],
    // A new layer is its first version: no layer exists to name. `ListLayerVersions` is read by the page that adds a layer to a
    // function, which names the layer itself.
    ops: &[
        ops::free("PublishLayerVersion"),
        ops::free("ListLayerVersions"),
    ],
    ..Kind::BARE
};
