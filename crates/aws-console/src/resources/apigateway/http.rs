//! HTTP and WebSocket APIs (`apigatewayv2`): the list, an API's page, and what the pages do to its routes, integrations,
//! authorizers, CORS, stages and deployments.

use super::super::Format::{Flag, Json, Mono, Pairs, Text as Plain, Time};
use super::super::{
    Create, Delete, Id, Input, InputKind, Kind, List, Step, Sub, Text, col, field, input, ops,
};

pub(super) const HTTP: Kind = Kind {
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
    // What the pages make: the wizard's four creates, a route's attachments, CORS (an update of the API), the authorizers, the
    // stages and their deployments.
    ops: &[
        ops::free("GetApis").of("apigatewayv2"),
        ops::free("GetApi").of("apigatewayv2"),
        ops::free("CreateApi").of("apigatewayv2"),
        ops::free("UpdateApi").of("apigatewayv2"),
        ops::free("GetRoutes").of("apigatewayv2"),
        ops::free("CreateRoute").of("apigatewayv2"),
        ops::free("UpdateRoute").of("apigatewayv2"),
        ops::free("DeleteRoute").of("apigatewayv2"),
        ops::free("GetIntegrations").of("apigatewayv2"),
        ops::free("CreateIntegration").of("apigatewayv2"),
        ops::free("UpdateIntegration").of("apigatewayv2"),
        ops::free("DeleteIntegration").of("apigatewayv2"),
        ops::free("GetAuthorizers").of("apigatewayv2"),
        ops::free("CreateAuthorizer").of("apigatewayv2"),
        ops::free("DeleteAuthorizer").of("apigatewayv2"),
        ops::free("GetStages").of("apigatewayv2"),
        ops::free("CreateStage").of("apigatewayv2"),
        ops::free("UpdateStage").of("apigatewayv2"),
        ops::free("DeleteStage").of("apigatewayv2"),
        ops::free("GetDeployments").of("apigatewayv2"),
        ops::free("CreateDeployment").of("apigatewayv2"),
        ops::free("DeleteDeployment").of("apigatewayv2"),
    ],
    ..Kind::BARE
};
