//! What a control-plane request asks to do, as IAM sees it: API Gateway's actions are the HTTP methods (`apigateway:GET`,
//! `apigateway:POST`, `PUT`, `PATCH`, `DELETE`) on the resource the path names, `arn:aws:apigateway:<region>::/restapis/<id>/...`
//! (v2's `/v2/apis/...` is `::/apis/...`). The path is the operation's own `@http` template filled from the request, so it is the
//! path the client sent.

use aws_peers::Need;
use aws_schema::tables::Tables;
use aws_wire::service::Call;
use serde_json::Value;

use crate::core::{Api, Core};
use crate::req::Req;

/// Every thing `call` asks to do.
pub(crate) fn needs(api: Api, call: &Call, r: &Req, _core: &Core) -> Vec<Need> {
    let tables: &Tables = match api {
        Api::V1 => &aws_model_api_gateway::tables::TABLES,
        Api::V2 => &aws_model_apigatewayv2::tables::TABLES,
    };
    let Some(op) = tables.op(call.op) else {
        return Vec::new();
    };
    let path = fill(tables, op.input, op.uri, &r.body);
    let path = path.strip_prefix("/v2").unwrap_or(&path);
    vec![Need::new(
        format!("apigateway:{}", op.method),
        crate::v1::arn(r.region(), path),
    )]
}

/// The URI template with each `{label}` replaced by the request's value (the query part dropped).
fn fill(tables: &Tables, input: Option<u32>, uri: &str, body: &Value) -> String {
    let template = uri.split('?').next().unwrap_or(uri);
    let Some(shape) = input.and_then(|s| tables.shape(s)) else {
        return template.to_owned();
    };
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(rest.get(..open).unwrap_or_default());
        let after = rest.get(open + 1..).unwrap_or_default();
        let Some(close) = after.find('}') else {
            break;
        };
        let name = after.get(..close).unwrap_or_default().trim_end_matches('+');
        let wire = (shape.first..shape.first + shape.count)
            .filter_map(|i| tables.member(i))
            .find(|m| m.name == name)
            .map_or(name, |m| m.wire);
        out.push_str(body.get(wire).and_then(Value::as_str).unwrap_or_default());
        rest = after.get(close + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_path_is_the_template_filled_from_the_request() {
        let t = &aws_model_api_gateway::tables::TABLES;
        let op = t.op(t.find_op("PutIntegration").unwrap()).unwrap();
        let body = json!({"restApiId": "a1", "resourceId": "r1", "requestHttpMethod": "GET"});
        assert_eq!(
            fill(t, op.input, op.uri, &body),
            "/restapis/a1/resources/r1/methods/GET/integration"
        );
        let op = t.op(t.find_op("ImportRestApi").unwrap()).unwrap();
        assert_eq!(fill(t, op.input, op.uri, &json!({})), "/restapis");
    }
}
