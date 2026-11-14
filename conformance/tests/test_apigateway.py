"""API Gateway through boto3 — REST APIs (v1) and HTTP APIs (v2) — and then
through plain HTTP, because a deployed API is something a browser calls.
"""

import json
import urllib.error
import urllib.request

from harness.helpers import zip_bytes

BASIC = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
FAST = {"Delay": 1, "MaxAttempts": 120}

# API Gateway's own ids: ten random characters each.
IDS = ("id", "parentId", "rootResourceId", "restApiId", "resourceId", "deploymentId",
       "ApiId", "RouteId", "IntegrationId", "DeploymentId", "Target", "ApiEndpoint",
       "cacheNamespace", "uri", "IntegrationUri")
CLOCK = ("createdDate", "lastUpdatedDate", "CreatedDate", "LastUpdatedDate")

PROXY = '''
import json

def handler(event, context):
    # Both payload formats, reduced to what a handler usually reads.
    v2 = event.get("version") == "2.0"
    seen = {
        "method": event["requestContext"]["http"]["method"] if v2 else event["httpMethod"],
        "path": event["rawPath"] if v2 else event["path"],
        "query": event.get("queryStringParameters"),
        "path_parameters": event.get("pathParameters"),
        "body": event.get("body"),
        "stage": event["requestContext"]["stage"],
    }
    return {"statusCode": 201, "headers": {"content-type": "application/json", "x-handled-by": "lambda"},
            "body": json.dumps(seen)}
'''


def call(url, method="GET", body=None, headers=None):
    req = urllib.request.Request(url, data=body, method=method, headers=headers or {})
    try:
        with urllib.request.urlopen(req) as r:
            status, raw, got = r.status, r.read(), r.headers
    except urllib.error.HTTPError as e:
        status, raw, got = e.code, e.read(), e.headers
    try:
        parsed = json.loads(raw)
    except ValueError:
        parsed = raw.decode(errors="replace")
    return {"status": status, "body": parsed, "x-handled-by": got.get("x-handled-by"),
            "content-type": (got.get("content-type") or "").split(";")[0]}


def proxy_function(client, names, cleanup, service_role):
    lam = client("lambda")
    name = names("fn")
    arn = lam.create_function(
        FunctionName=name, Runtime="python3.12", Role=service_role("lambda", BASIC),
        Handler="index.handler", Code={"ZipFile": zip_bytes({"index.py": PROXY})}, Timeout=10,
    )["FunctionArn"]
    cleanup(lam.delete_function, FunctionName=name)
    lam.get_waiter("function_active_v2").wait(FunctionName=name, WaiterConfig=FAST)
    lam.add_permission(FunctionName=name, StatementId="apigw", Action="lambda:InvokeFunction",
                       Principal="apigateway.amazonaws.com")
    return name, arn


def test_rest_api_with_a_mock_integration(client, names, cleanup, snapshot, target, eventually):
    api = client("apigateway")
    made = snapshot.match("create", api.create_rest_api(name=names("api"), description="conformance"),
                          opaque=IDS + CLOCK)
    rid = made["id"]
    cleanup(api.delete_rest_api, restApiId=rid)

    root = api.get_resources(restApiId=rid)["items"][0]
    snapshot.match("root", root, opaque=IDS)
    res = snapshot.match("resource", api.create_resource(
        restApiId=rid, parentId=root["id"], pathPart="ping"), opaque=IDS)
    at = dict(restApiId=rid, resourceId=res["id"], httpMethod="GET")
    snapshot.match("method", api.put_method(**at, authorizationType="NONE"))
    snapshot.match("integration", api.put_integration(
        **at, type="MOCK", requestTemplates={"application/json": '{"statusCode": 200}'}),
        opaque=IDS)
    snapshot.match("method-response", api.put_method_response(**at, statusCode="200"))
    snapshot.match("integration-response", api.put_integration_response(
        **at, statusCode="200", responseTemplates={"application/json": '{"pong": true}'}))

    deployed = snapshot.match("deploy", api.create_deployment(restApiId=rid, stageName="dev"), opaque=IDS + CLOCK)
    snapshot.match("stage", api.get_stage(restApiId=rid, stageName="dev"), opaque=IDS + CLOCK)
    # A deployment a stage serves cannot be deleted out from under it.
    snapshot.error("delete-a-served-deployment", lambda: api.delete_deployment(
        restApiId=rid, deploymentId=deployed["id"]))
    snapshot.error("delete-an-absent-deployment", lambda: api.delete_deployment(
        restApiId=rid, deploymentId="nonesuch"))

    def answered():
        r = call(target.execute_api_url(rid, "dev", "ping"))
        assert r["status"] == 200, r
        return r

    snapshot.match("get", eventually(answered, timeout=60))
    snapshot.match("no-such-path", call(target.execute_api_url(rid, "dev", "nowhere")))
    snapshot.match("no-such-method", call(target.execute_api_url(rid, "dev", "ping"), method="POST"))
    snapshot.match("resources", sorted(r["path"] for r in api.get_resources(restApiId=rid)["items"]))


def test_rest_api_proxies_to_lambda(client, names, cleanup, snapshot, service_role, target, eventually):
    api = client("apigateway")
    _, fn_arn = proxy_function(client, names, cleanup, service_role)
    rid = api.create_rest_api(name=names("api"))["id"]
    cleanup(api.delete_rest_api, restApiId=rid)
    root = api.get_resources(restApiId=rid)["items"][0]["id"]
    orders = api.create_resource(restApiId=rid, parentId=root, pathPart="orders")["id"]
    one = api.create_resource(restApiId=rid, parentId=orders, pathPart="{id}")["id"]
    uri = (f"arn:aws:apigateway:{target.region}:lambda:path/2015-03-31/functions/"
           f"{fn_arn}/invocations")
    for resource in (orders, one):
        api.put_method(restApiId=rid, resourceId=resource, httpMethod="ANY", authorizationType="NONE")
        api.put_integration(restApiId=rid, resourceId=resource, httpMethod="ANY",
                            type="AWS_PROXY", integrationHttpMethod="POST", uri=uri)
    api.create_deployment(restApiId=rid, stageName="prod")

    def answered():
        r = call(target.execute_api_url(rid, "prod", "orders/42?expand=items"))
        assert r["status"] == 201, r
        return r

    snapshot.match("get-with-a-path-parameter", eventually(answered, timeout=90))
    snapshot.match("post-with-a-body", call(
        target.execute_api_url(rid, "prod", "orders"), method="POST",
        body=b'{"sku": "book"}', headers={"content-type": "application/json"}))


def test_http_api_proxies_to_lambda(client, names, cleanup, snapshot, service_role, target, eventually):
    v2 = client("apigatewayv2")
    _, fn_arn = proxy_function(client, names, cleanup, service_role)
    made = snapshot.match("create", v2.create_api(Name=names("api"), ProtocolType="HTTP"),
                          opaque=IDS + CLOCK)
    aid = made["ApiId"]
    cleanup(v2.delete_api, ApiId=aid)

    integ = snapshot.match("integration", v2.create_integration(
        ApiId=aid, IntegrationType="AWS_PROXY", IntegrationUri=fn_arn, PayloadFormatVersion="2.0"),
        opaque=IDS)
    snapshot.match("route", v2.create_route(
        ApiId=aid, RouteKey="GET /orders/{id}", Target=f"integrations/{integ['IntegrationId']}"),
        opaque=IDS)
    snapshot.match("stage", v2.create_stage(ApiId=aid, StageName="$default", AutoDeploy=True),
                   opaque=IDS + CLOCK)

    def answered():
        r = call(made["ApiEndpoint"].rstrip("/") + "/orders/42?expand=items"
                 if target.name == "aws" else target.execute_api_url(aid, "$default", "orders/42?expand=items"))
        assert r["status"] == 201, r
        return r

    snapshot.match("get", eventually(answered, timeout=90))
    snapshot.match("routes", [r["RouteKey"] for r in v2.get_routes(ApiId=aid)["Items"]])


def test_refusals(client, names, cleanup, snapshot):
    api, v2 = client("apigateway"), client("apigatewayv2")
    snapshot.error("absent-rest-api", lambda: api.get_rest_api(restApiId="neverwas00"))
    snapshot.error("absent-http-api", lambda: v2.get_api(ApiId="neverwas00"))

    rid = api.create_rest_api(name=names("api"))["id"]
    cleanup(api.delete_rest_api, restApiId=rid)
    root = api.get_resources(restApiId=rid)["items"][0]["id"]
    res = api.create_resource(restApiId=rid, parentId=root, pathPart="things")["id"]
    snapshot.error("same-path-twice", lambda: api.create_resource(
        restApiId=rid, parentId=root, pathPart="things"))
    snapshot.error("bad-path-part", lambda: api.create_resource(
        restApiId=rid, parentId=root, pathPart="has space"))
    snapshot.error("absent-parent", lambda: api.create_resource(
        restApiId=rid, parentId="neverwas00", pathPart="x"))
    snapshot.error("bad-http-method", lambda: api.put_method(
        restApiId=rid, resourceId=res, httpMethod="FETCH", authorizationType="NONE"))
    snapshot.error("integration-before-method", lambda: api.put_integration(
        restApiId=rid, resourceId=res, httpMethod="GET", type="MOCK"))
    snapshot.error("deploy-with-no-methods", lambda: api.create_deployment(
        restApiId=rid, stageName="dev"))
    api.put_method(restApiId=rid, resourceId=res, httpMethod="GET", authorizationType="NONE")
    snapshot.error("deploy-a-method-with-no-integration", lambda: api.create_deployment(
        restApiId=rid, stageName="dev"))
    snapshot.error("absent-stage", lambda: api.get_stage(restApiId=rid, stageName="nope"))

    aid = v2.create_api(Name=names("http"), ProtocolType="HTTP")["ApiId"]
    cleanup(v2.delete_api, ApiId=aid)
    snapshot.error("bad-route-key", lambda: v2.create_route(ApiId=aid, RouteKey="orders"))
    snapshot.error("route-to-absent-integration", lambda: v2.create_route(
        ApiId=aid, RouteKey="GET /x", Target="integrations/neverwas"))
    snapshot.error("bad-protocol", lambda: v2.create_api(Name=names("bad"), ProtocolType="GOPHER"))


# Written ahead of a recording (A15, 2026-11-13): UNVERIFIED until recorded. They create nothing billed by the hour (no cache
# cluster, no custom domain), and the account's CloudWatch role is never set.

def mock_api(api, names, cleanup, part="ping", stage=None):
    """A REST API with GET /<part> answered by MOCK: (api id, resource id)."""
    rid = api.create_rest_api(name=names("api"))["id"]
    cleanup(api.delete_rest_api, restApiId=rid)
    root = api.get_resources(restApiId=rid)["items"][0]["id"]
    res = api.create_resource(restApiId=rid, parentId=root, pathPart=part)["id"]
    at = dict(restApiId=rid, resourceId=res, httpMethod="GET")
    api.put_method(**at, authorizationType="NONE")
    api.put_integration(**at, type="MOCK", requestTemplates={"application/json": '{"statusCode": 200}'})
    api.put_method_response(**at, statusCode="200")
    api.put_integration_response(**at, statusCode="200", responseTemplates={"application/json": '{"ok": true}'})
    if stage:
        api.create_deployment(restApiId=rid, stageName=stage)
    return rid, res


def test_rest_api_update_operations(client, names, cleanup, snapshot):
    api = client("apigateway")
    rid, res = mock_api(api, names, cleanup)
    at = dict(restApiId=rid, resourceId=res, httpMethod="GET")
    snapshot.match("update-method", api.update_method(**at, patchOperations=[
        {"op": "replace", "path": "/operationName", "value": "ping"},
        {"op": "add", "path": "/requestParameters/method.request.querystring.q", "value": "false"}]))
    snapshot.match("update-integration", api.update_integration(**at, patchOperations=[
        {"op": "replace", "path": "/timeoutInMillis", "value": "1000"},
        {"op": "replace", "path": "/passthroughBehavior", "value": "NEVER"}]), opaque=IDS)
    snapshot.match("update-method-response", api.update_method_response(**at, statusCode="200", patchOperations=[
        {"op": "add", "path": "/responseParameters/method.response.header.X-Ping", "value": "false"}]))
    snapshot.match("update-integration-response", api.update_integration_response(**at, statusCode="200", patchOperations=[
        {"op": "add", "path": "/responseParameters/method.response.header.X-Ping", "value": "'pong'"}]))
    snapshot.error("update-an-unknown-path", lambda: api.update_method(**at, patchOperations=[
        {"op": "replace", "path": "/nonsense", "value": "1"}]))
    snapshot.match("get-method", api.get_method(**at), opaque=IDS)
    snapshot.error("put-the-method-twice", lambda: api.put_method(**at, authorizationType="NONE"))
    dep = api.create_deployment(restApiId=rid, stageName="dev", variables={"color": "blue"})
    snapshot.match("update-stage", api.update_stage(restApiId=rid, stageName="dev", patchOperations=[
        {"op": "replace", "path": "/tracingEnabled", "value": "true"},
        {"op": "add", "path": "/variables/size", "value": "large"},
        {"op": "replace", "path": "/*/*/metrics/enabled", "value": "true"},
        {"op": "replace", "path": "/*/*/throttling/burstLimit", "value": "100"}]), opaque=IDS + CLOCK)
    snapshot.error("logging-without-a-role", lambda: api.update_stage(restApiId=rid, stageName="dev", patchOperations=[
        {"op": "replace", "path": "/*/*/logging/loglevel", "value": "INFO"}]))
    snapshot.match("update-deployment", api.update_deployment(restApiId=rid, deploymentId=dep["id"], patchOperations=[
        {"op": "replace", "path": "/description", "value": "first"}]), opaque=IDS + CLOCK)
    snapshot.match("update-rest-api", api.update_rest_api(restApiId=rid, patchOperations=[
        {"op": "replace", "path": "/description", "value": "changed"},
        {"op": "add", "path": "/binaryMediaTypes/image~1png"}]), opaque=IDS + CLOCK)


def test_models_validators_and_gateway_responses(client, names, cleanup, snapshot):
    api = client("apigateway")
    rid, _ = mock_api(api, names, cleanup)
    snapshot.match("default-models", sorted(m["name"] for m in api.get_models(restApiId=rid)["items"]))
    snapshot.match("model", api.create_model(restApiId=rid, name="Pet", contentType="application/json",
                                             schema='{"type": "object"}'), opaque=IDS)
    snapshot.error("model-twice", lambda: api.create_model(restApiId=rid, name="Pet", contentType="application/json",
                                                           schema='{"type": "object"}'))
    snapshot.error("model-bad-name", lambda: api.create_model(restApiId=rid, name="no-dash", contentType="application/json",
                                                              schema='{"type": "object"}'))
    snapshot.match("validator", api.create_request_validator(restApiId=rid, name="body", validateRequestBody=True),
                   opaque=IDS)
    snapshot.match("default-response", api.get_gateway_response(restApiId=rid, responseType="MISSING_AUTHENTICATION_TOKEN"))
    snapshot.match("put-response", api.put_gateway_response(
        restApiId=rid, responseType="DEFAULT_4XX", statusCode="418",
        responseParameters={"gatewayresponse.header.X-Teapot": "'yes'"}))
    snapshot.match("response-types", sorted(
        r["responseType"] for r in api.get_gateway_responses(restApiId=rid, limit=500)["items"]))


def test_api_keys_and_usage_plans(client, names, cleanup, snapshot, target, eventually):
    api = client("apigateway")
    rid, res = mock_api(api, names, cleanup, stage="live")
    key = snapshot.match("key", api.create_api_key(name=names("key"), enabled=True,
                                                   value="doze0123456789abcdefghijk"), opaque=IDS + CLOCK)
    cleanup(api.delete_api_key, apiKey=key["id"])
    snapshot.match("key-hidden", api.get_api_key(apiKey=key["id"]), opaque=IDS + CLOCK)
    plan = snapshot.match("plan", api.create_usage_plan(name=names("plan"), apiStages=[{"apiId": rid, "stage": "live"}],
                                                        quota={"limit": 100, "period": "DAY"}), opaque=IDS + ("apiId",))
    cleanup(api.delete_usage_plan, usagePlanId=plan["id"])
    snapshot.match("plan-key", api.create_usage_plan_key(usagePlanId=plan["id"], keyId=key["id"], keyType="API_KEY"),
                   opaque=IDS)
    cleanup(api.delete_usage_plan_key, usagePlanId=plan["id"], keyId=key["id"])
    snapshot.error("absent-stage-in-a-plan", lambda: api.create_usage_plan(
        name=names("plan2"), apiStages=[{"apiId": rid, "stage": "nope"}]))
    api.update_method(restApiId=rid, resourceId=res, httpMethod="GET",
                      patchOperations=[{"op": "replace", "path": "/apiKeyRequired", "value": "true"}])
    api.create_deployment(restApiId=rid, stageName="live")

    def keyless():
        r = call(target.execute_api_url(rid, "live", "ping"))
        assert r["status"] == 403, r
        return r

    snapshot.match("without-a-key", eventually(keyless, timeout=60))

    def keyed():
        r = call(target.execute_api_url(rid, "live", "ping"), headers={"x-api-key": "doze0123456789abcdefghijk"})
        assert r["status"] == 200, r
        return r

    snapshot.match("with-the-key", eventually(keyed, timeout=60))


IMPORT = '''openapi: 3.0.1
info:
  title: imported
  version: "1"
paths:
  /pets/{id}:
    get:
      parameters:
      - name: id
        in: path
        required: true
        schema:
          type: string
      responses:
        "200":
          description: ok
      x-amazon-apigateway-integration:
        type: mock
        requestTemplates:
          application/json: '{"statusCode": 200}'
        responses:
          default:
            statusCode: "200"
            responseTemplates:
              application/json: '{"id": "$input.params(''id'')"}'
'''


def test_import_and_export_a_rest_api(client, names, cleanup, snapshot, target, eventually):
    api = client("apigateway")
    made = snapshot.match("import", api.import_rest_api(body=IMPORT.encode()), opaque=IDS + CLOCK)
    rid = made["id"]
    cleanup(api.delete_rest_api, restApiId=rid)
    snapshot.match("paths", sorted(r["path"] for r in api.get_resources(restApiId=rid)["items"]))
    api.create_deployment(restApiId=rid, stageName="v1")

    def answered():
        r = call(target.execute_api_url(rid, "v1", "pets/7"))
        assert r["status"] == 200, r
        return r

    snapshot.match("call", eventually(answered, timeout=60))
    exported = api.get_export(restApiId=rid, stageName="v1", exportType="oas30", accepts="application/json")
    doc = json.loads(exported["body"].read())
    snapshot.match("export-paths", sorted(doc["paths"]))
    snapshot.match("export-methods", sorted(doc["paths"]["/pets/{id}"]))
    snapshot.error("not-openapi", lambda: api.import_rest_api(body=b'{"hello": "world"}'))


def test_http_api_routes_stages_and_deployments(client, names, cleanup, snapshot):
    v2 = client("apigatewayv2")
    made = snapshot.match("create", v2.create_api(
        Name=names("api"), ProtocolType="HTTP",
        CorsConfiguration={"AllowOrigins": ["https://example.com"], "AllowMethods": ["GET"], "MaxAge": 60}),
        opaque=IDS + CLOCK)
    aid = made["ApiId"]
    cleanup(v2.delete_api, ApiId=aid)
    integ = snapshot.match("integration", v2.create_integration(
        ApiId=aid, IntegrationType="HTTP_PROXY", IntegrationMethod="GET", IntegrationUri="https://example.com/{proxy}"),
        opaque=IDS)
    target_ = f"integrations/{integ['IntegrationId']}"
    route = snapshot.match("route", v2.create_route(ApiId=aid, RouteKey="GET /items/{proxy+}", Target=target_), opaque=IDS)
    snapshot.error("route-twice", lambda: v2.create_route(ApiId=aid, RouteKey="GET /items/{proxy+}", Target=target_))
    snapshot.match("update-route", v2.update_route(ApiId=aid, RouteId=route["RouteId"], OperationName="items"), opaque=IDS)
    snapshot.match("stage", v2.create_stage(ApiId=aid, StageName="manual", StageVariables={"a": "1"}), opaque=IDS + CLOCK)
    dep = snapshot.match("deployment", v2.create_deployment(ApiId=aid, StageName="manual"), opaque=IDS + CLOCK)
    snapshot.match("stage-after", v2.get_stage(ApiId=aid, StageName="manual"), opaque=IDS + CLOCK)
    snapshot.match("update-stage", v2.update_stage(ApiId=aid, StageName="manual", StageVariables={"b": "2"}),
                   opaque=IDS + CLOCK)
    snapshot.error("delete-a-used-integration", lambda: v2.delete_integration(
        ApiId=aid, IntegrationId=integ["IntegrationId"]))
    snapshot.error("delete-a-served-deployment", lambda: v2.delete_deployment(ApiId=aid, DeploymentId=dep["DeploymentId"]))
    v2.delete_cors_configuration(ApiId=aid)
    snapshot.match("after-cors", v2.get_api(ApiId=aid), opaque=IDS + CLOCK)


def test_websocket_api_control_plane(client, names, cleanup, snapshot):
    v2 = client("apigatewayv2")
    made = snapshot.match("create", v2.create_api(
        Name=names("ws"), ProtocolType="WEBSOCKET", RouteSelectionExpression="$request.body.action"),
        opaque=IDS + CLOCK)
    aid = made["ApiId"]
    cleanup(v2.delete_api, ApiId=aid)
    snapshot.error("no-selection-expression", lambda: v2.create_api(Name=names("ws2"), ProtocolType="WEBSOCKET"))
    integ = snapshot.match("integration", v2.create_integration(ApiId=aid, IntegrationType="MOCK",
                                                                RequestTemplates={"$default": '{"statusCode": 200}'}),
                           opaque=IDS)
    snapshot.match("integration-response", v2.create_integration_response(
        ApiId=aid, IntegrationId=integ["IntegrationId"], IntegrationResponseKey="$default",
        ResponseTemplates={"$default": '{"echo": true}'}), opaque=IDS + ("IntegrationResponseId",))
    route = snapshot.match("route", v2.create_route(ApiId=aid, RouteKey="$default",
                                                    Target=f"integrations/{integ['IntegrationId']}",
                                                    RouteResponseSelectionExpression="$default"), opaque=IDS)
    snapshot.match("route-response", v2.create_route_response(ApiId=aid, RouteId=route["RouteId"],
                                                              RouteResponseKey="$default"),
                   opaque=IDS + ("RouteResponseId",))
    snapshot.match("stage", v2.create_stage(ApiId=aid, StageName="prod", AutoDeploy=True), opaque=IDS + CLOCK)
    snapshot.match("routes", sorted(r["RouteKey"] for r in v2.get_routes(ApiId=aid)["Items"]))
