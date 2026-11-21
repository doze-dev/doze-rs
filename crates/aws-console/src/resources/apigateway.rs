//! API Gateway: REST APIs (its first API) and HTTP and WebSocket APIs (`apigatewayv2` on the bus), each with its stages, and
//! its resources or routes. An API is named by its id, shown as its name.

use super::Format::{Flag, Json, Mono, Pairs, Text as Plain, Time};
use super::{
    Create, Delete, Descriptor, Id, Input, InputKind, Kind, List, Step, Sub, Text, col, field,
    input,
};

pub(super) static APIGATEWAY: Descriptor = Descriptor {
    service: "apigateway",
    target: "apigateway",
    kinds: &[REST, HTTP],
};

const REST: Kind = Kind {
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
    ..Kind::BARE
};

const HTTP: Kind = Kind {
    id: "apis",
    title: "HTTP and WebSocket APIs",
    one: "API",
    // Read as the wire writes it: this API's members are camelCase there (`apiId`).
    about: "HTTP APIs (routes to a function or an HTTP endpoint) and WebSocket APIs, deployed to stages.",
    members: &["ApiId"],
    list: List {
        step: Step::new("GetApis", r#"{"MaxResults":"100","NextToken":"{token}"}"#).of("apigatewayv2"),
        items: "items",
        item_as: "ApiId",
        token_out: "nextToken",
        id: "apiId",
        name: Text::at("apiId"),
        label: Some(Text::at("name")),
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        col("ID", "apiId", Mono),
        col("Protocol", "protocolType", Plain),
        col("Endpoint", "apiEndpoint", Mono),
        col("Created", "createdDate", Time),
    ],
    id_of: Id::Name,
    detail: &[Step::new("GetApi", r#"{"ApiId":"{id}"}"#).of("apigatewayv2")],
    fields: &[
        field("Name", "name", Plain),
        field("ID", "apiId", Mono),
        field("Protocol", "protocolType", Plain),
        field("Endpoint", "apiEndpoint", Mono),
        field("Description", "description", Plain),
        field("Route selection", "routeSelectionExpression", Mono),
        field(
            "Default endpoint",
            "disableExecuteApiEndpoint",
            Flag("Disabled", "Enabled"),
        ),
        field("Created", "createdDate", Time),
        field("Tags", "tags", Pairs),
        field("CORS", "corsConfiguration", Json),
    ],
    subs: &[
        Sub {
            title: "Stages",
            step: Step::new("GetStages", r#"{"ApiId":"{id}"}"#).of("apigatewayv2"),
            items: "items",
            item_as: "StageName",
            columns: &[
                col("Stage", "stageName", Mono),
                col("Auto-deploy", "autoDeploy", Flag("On", "Off")),
                col("Deployment", "deploymentId", Mono),
                col("Variables", "stageVariables", Pairs),
                col("Updated", "lastUpdatedDate", Time),
            ],
            link: None,
            more: "nextToken",
            add: None,
        },
        Sub {
            title: "Routes",
            step: Step::new("GetRoutes", r#"{"ApiId":"{id}"}"#).of("apigatewayv2"),
            items: "items",
            item_as: "RouteKey",
            columns: &[
                col("Route", "routeKey", Mono),
                col("Target", "target", Mono),
                col("Authorization", "authorizationType", Plain),
            ],
            link: None,
            more: "nextToken",
            add: None,
        },
    ],
    create: Some(Create {
        inputs: &[
            Input {
                required: true,
                placeholder: "orders-http",
                ..input("Name", "Name", "Need not be unique.", InputKind::Text)
            },
            Input {
                required: true,
                value: "HTTP",
                ..input(
                    "ProtocolType",
                    "Protocol",
                    "Set at creation only.",
                    InputKind::Select(&["HTTP", "WEBSOCKET"]),
                )
            },
            Input {
                placeholder: "$request.body.action",
                ..input(
                    "RouteSelectionExpression",
                    "Route selection",
                    "WebSocket APIs only, where API Gateway needs it.",
                    InputKind::Text,
                )
            },
        ],
        steps: &[Step::new(
            "CreateApi",
            // Its body is read as the wire writes it: camelCase.
            r#"{"name":"{form:Name}","protocolType":"{form:ProtocolType}","routeSelectionExpression":"{form:RouteSelectionExpression}"}"#,
        )
        .of("apigatewayv2")],
        name: Text::at("apiId"),
        key: "Name",
    }),
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteApi", r#"{"ApiId":"{id}"}"#).of("apigatewayv2"),
        words: "Deleting removes the API, its routes, integrations and stages. Its URLs stop answering.",
    }),
    reveal: None,
    deep: &[],
    ..Kind::BARE
};
