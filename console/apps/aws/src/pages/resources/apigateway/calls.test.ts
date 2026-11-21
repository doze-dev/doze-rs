import { expect, test } from "bun:test";
import { sdkShown } from "./calls";

test("PutIntegration is shown with the SDK's names", () => {
  expect(sdkShown("PutIntegration", { restApiId: "a", resourceId: "r", requestHttpMethod: "POST", type: "AWS_PROXY", httpMethod: "POST" })).toEqual({ restApiId: "a", resourceId: "r", httpMethod: "POST", type: "AWS_PROXY", integrationHttpMethod: "POST" });
  expect(sdkShown("PutMethod", { httpMethod: "GET" })).toEqual({ httpMethod: "GET" });
});
