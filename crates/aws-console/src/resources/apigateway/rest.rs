//! REST APIs: the list, an API's page, and what the pages do to its resources, methods, stages, deployments and authorizers.

use super::super::Format::{Flag, Json, Mono, Pairs, Text as Plain, Time};
use super::super::{
    Create, Delete, Id, Input, InputKind, Kind, List, Step, Sub, Text, col, field, input, ops,
};

pub(super) const REST: Kind = Kind {
    id: "restapis",
    title: "REST APIs",
    one: "REST API",
    about: "REST APIs: resources and methods, each integrated with a function, a service or an HTTP endpoint, deployed to stages.",
    members: &["restApiId"],
    list: List {
        step: Step::new("GetRestApis", r#"{"limit":500,"position":"{token}"}"#),
        // A REST service's answer is read as the wire writes it: this list is `item` there.
        items: "item",
        item_as: "id",
        token_out: "position",
        id: "id",
        name: Text::at("id"),
        label: Some(Text::at("name")),
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        col("ID", "id", Mono),
        col("Description", "description", Plain),
        col("Endpoint", "endpointConfiguration.types.0", Plain),
        col("Created", "createdDate", Time),
    ],
    id_of: Id::Name,
    detail: &[Step::new("GetRestApi", r#"{"restApiId":"{id}"}"#)],
    fields: &[
        field("Name", "name", Plain),
        field("ID", "id", Mono),
        field("Description", "description", Plain),
        field("Endpoint", "endpointConfiguration.types.0", Plain),
        field("Root resource", "rootResourceId", Mono),
        field("API key source", "apiKeySource", Plain),
        field(
            "Default endpoint",
            "disableExecuteApiEndpoint",
            Flag("Disabled", "Enabled"),
        ),
        field("Version", "version", Mono),
        field("Created", "createdDate", Time),
        field("Tags", "tags", Pairs),
        field("Resource policy", "policy", Json),
    ],
    subs: &[
        Sub {
            title: "Stages",
            step: Step::new("GetStages", r#"{"restApiId":"{id}"}"#),
            items: "item",
            item_as: "stageName",
            columns: &[
                col("Stage", "stageName", Mono),
                col("Deployment", "deploymentId", Mono),
                col("Description", "description", Plain),
                col("Variables", "variables", Pairs),
                col("Updated", "lastUpdatedDate", Time),
            ],
            link: None,
            more: "",
            add: None,
        },
        Sub {
            title: "Resources",
            step: Step::new("GetResources", r#"{"restApiId":"{id}","limit":500}"#),
            items: "item",
            item_as: "path",
            columns: &[
                col("Path", "path", Mono),
                col("ID", "id", Mono),
                col("Methods", "resourceMethods", Json),
            ],
            link: None,
            more: "position",
            add: None,
        },
    ],
    create: Some(Create {
        inputs: &[
            Input {
                required: true,
                placeholder: "orders-api",
                ..input("name", "Name", "Need not be unique.", InputKind::Text)
            },
            input("description", "Description", "", InputKind::Text),
            // API Gateway makes an edge-optimised API (a CloudFront front) when none is said; locally there is no edge, so the
            // console says regional.
            Input {
                required: true,
                value: "REGIONAL",
                ..input(
                    "endpointType",
                    "Endpoint type",
                    "Regional is what local use wants: doze-aws has no edge network.",
                    InputKind::Select(&["REGIONAL", "EDGE", "PRIVATE"]),
                )
            },
        ],
        steps: &[Step::new(
            "CreateRestApi",
            r#"{"name":"{form:name}","description":"{form:description}","endpointConfiguration":{"types":["{form:endpointType}"]}}"#,
        )],
        name: Text::at("id"),
        key: "name",
    }),
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteRestApi", r#"{"restApiId":"{id}"}"#),
        words: "Deleting removes the API, its resources, methods, deployments and stages. Its URLs stop answering.",
    }),
    reveal: None,
    deep: &[],
    // What the pages make: the API's create and reads, its resources and methods (a method is four calls: the method, its
    // integration, and a response of each), its stages and deployments, its authorizers, and the usage plans that name it.
    ops: &[
        ops::free("GetRestApis"),
        ops::free("GetRestApi"),
        ops::free("CreateRestApi"),
        ops::free("GetResources"),
        ops::free("CreateResource"),
        ops::free("DeleteResource"),
        ops::free("GetMethod"),
        ops::free("PutMethod"),
        ops::free("UpdateMethod"),
        ops::free("DeleteMethod"),
        ops::free("PutIntegration"),
        ops::free("UpdateIntegration"),
        ops::free("PutIntegrationResponse"),
        ops::free("DeleteIntegrationResponse"),
        ops::free("PutMethodResponse"),
        ops::free("DeleteMethodResponse"),
        ops::free("GetModels"),
        ops::free("GetStages"),
        ops::free("GetStage"),
        ops::free("CreateStage"),
        ops::free("UpdateStage"),
        ops::free("DeleteStage"),
        ops::free("GetDeployments"),
        ops::free("CreateDeployment"),
        // A deployment a stage serves is refused by the service, which the delete says in words before it asks.
        ops::free("DeleteDeployment"),
        ops::free("GetAuthorizers"),
        ops::free("CreateAuthorizer"),
        ops::free("DeleteAuthorizer"),
        ops::free("GetUsagePlans"),
    ],
    ..Kind::BARE
};
