//! API Gateway: REST APIs (its first API) and HTTP and WebSocket APIs (`apigatewayv2` on the bus), each with its stages and its
//! resources or routes, and the usage plans of the account. An API is named by its id, shown as its name.
//!
//! The pages the console's app draws for these (the APIs list, an API's resources, routes and stages, a method's four pages, the
//! usage plans) make their own calls through the kinds' `ops`: each is one operation of the service, made as the caller `console`.

mod http;
mod plans;
mod rest;

use super::Descriptor;

pub(super) static APIGATEWAY: Descriptor = Descriptor {
    service: "apigateway",
    target: "apigateway",
    kinds: &[rest::REST, http::HTTP, plans::PLANS],
};
