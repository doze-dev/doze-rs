import { expect, test } from "bun:test";
import { sdkShown } from "./calls";

test("PutIntegration is shown with the SDK's names", () => {
  expect(sdkShown("PutIntegration", { restApiId: "a", resourceId: "r", requestHttpMethod: "POST", type: "AWS_PROXY", httpMethod: "POST" })).toEqual({ restApiId: "a", resourceId: "r", httpMethod: "POST", type: "AWS_PROXY", integrationHttpMethod: "POST" });
  expect(sdkShown("PutMethod", { httpMethod: "GET" })).toEqual({ httpMethod: "GET" });
});

test("an HTTP API's call is shown in camelCase, as the SDKs and the boards write it (the wire capitalises its path members)", () => {
  expect(sdkShown("CreateRoute", { ApiId: "q8m", routeKey: "GET /x", target: "integrations/abc" })).toEqual({ apiId: "q8m", routeKey: "GET /x", target: "integrations/abc" });
  expect(sdkShown("UpdateRoute", { ApiId: "q8m", RouteId: "r1", authorizerId: "a1" })).toEqual({ apiId: "q8m", routeId: "r1", authorizerId: "a1" });
  // The order is the input's, and a call that is not an object is left as it is.
  expect(Object.keys(sdkShown("CreateStage", { ApiId: "a", stageName: "$default", autoDeploy: true }) as object)).toEqual(["apiId", "stageName", "autoDeploy"]);
  expect(sdkShown("ListX", null)).toBeNull();
});
