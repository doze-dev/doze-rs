import { describe, expect, test } from "bun:test";
import { newRow } from "@doze/ui/components/app/key-values";
import { connProblem, createConnInput, createDestInput, destProblem, emptyConn, emptyDest, endpointProblem, nameProblem, rateProblem, type HttpParam } from "./dest-model";

describe("the API destination form", () => {
  test("the endpoint takes http and https, with a * for a path wildcard", () => {
    expect(endpointProblem("https://status.larder.example/v1/incidents/*")).toBeNull();
    expect(endpointProblem("http://127.0.0.1:9000/in")).toBeNull();
    expect(endpointProblem("ftp://x")).toBe("The endpoint is an http or https address.");
    expect(endpointProblem("nope")).toBe("The endpoint is not an address.");
    expect(endpointProblem("")).toBe("Give the endpoint.");
  });
  test("names and the rate", () => {
    expect(nameProblem("", "API destination")).toBe("Name the API destination.");
    expect(nameProblem("a b", "connection")).toContain("connection name");
    expect(rateProblem("")).toBeNull();
    expect(rateProblem("0")).toContain("above 0");
    expect(rateProblem("300")).toBeNull();
  });
  test("what is missing, in order", () => {
    const f = { ...emptyDest("arn:conn"), name: "d", endpoint: "https://x.example/" };
    expect(destProblem(f, [])).toBeNull();
    expect(destProblem(f, ["d"])).toBe("d already exists.");
    expect(destProblem({ ...f, connection: null }, [])).toBe("Choose the connection.");
  });
  test("CreateApiDestination leaves out the rate when none was given", () => {
    const f = { ...emptyDest("arn:conn"), name: "d", endpoint: "https://x.example/", method: "PUT" };
    expect(createDestInput(f, "arn:conn")).toEqual({ Name: "d", InvocationEndpoint: "https://x.example/", HttpMethod: "PUT", ConnectionArn: "arn:conn" });
    expect(createDestInput({ ...f, rate: "5", description: "x" }, "arn:conn")).toMatchObject({ InvocationRateLimitPerSecond: 5, Description: "x" });
  });
});

describe("the connection form", () => {
  const f = { ...emptyConn(), name: "status-key", keyValue: "s3cret" };
  test("each authorization type needs its own fields", () => {
    expect(connProblem(f, [])).toBeNull();
    expect(connProblem({ ...f, keyValue: "" }, [])).toBe("An API key needs a name and a value.");
    expect(connProblem({ ...f, auth: "BASIC" }, [])).toContain("username");
    expect(connProblem({ ...f, auth: "OAUTH_CLIENT_CREDENTIALS", tokenEndpoint: "x", clientId: "a", clientSecret: "b" }, [])).toContain("authorization endpoint is an http");
    expect(connProblem(f, ["status-key"])).toBe("status-key already exists.");
  });
  test("CreateConnection carries the authorization and the parameters that were filled in", () => {
    const p: HttpParam = { ...newRow("X-Source", "larder"), kind: "header", secret: false };
    expect(createConnInput({ ...f, description: "d", params: [p, { ...newRow(), kind: "header", secret: false }] })).toEqual({
      Name: "status-key", Description: "d", AuthorizationType: "API_KEY",
      AuthParameters: { ApiKeyAuthParameters: { ApiKeyName: "X-Api-Key", ApiKeyValue: "s3cret" }, InvocationHttpParameters: { HeaderParameters: [{ Key: "X-Source", Value: "larder", IsValueSecret: false }] } },
    });
    expect(createConnInput({ ...f, auth: "BASIC", username: "u", password: "p" })).toMatchObject({ AuthorizationType: "BASIC", AuthParameters: { BasicAuthParameters: { Username: "u", Password: "p" } } });
  });
  test("the call shown writes every secret as ..., what is sent keeps it", () => {
    const secret: HttpParam = { ...newRow("X-Token", "t0k3n"), kind: "header", secret: true };
    const plain: HttpParam = { ...newRow("X-Source", "larder"), kind: "header", secret: false };
    const shown = JSON.stringify(createConnInput({ ...f, params: [secret, plain] }, true));
    expect(shown).toContain('"ApiKeyValue":"..."');
    expect(shown).toContain('"Value":"..."');
    expect(shown).toContain('"Value":"larder"');
    for (const hidden of ["s3cret", "t0k3n"]) expect(shown).not.toContain(hidden);
    expect(JSON.stringify(createConnInput({ ...f, params: [secret] }))).toContain("t0k3n");
    expect(JSON.stringify(createConnInput({ ...f, auth: "BASIC", username: "u", password: "pw" }, true))).not.toContain("pw");
    expect(JSON.stringify(createConnInput({ ...f, auth: "OAUTH_CLIENT_CREDENTIALS", tokenEndpoint: "https://a.example/t", clientId: "id", clientSecret: "shh" }, true))).not.toContain("shh");
    expect(JSON.stringify(createConnInput({ ...f, keyValue: "" }, true))).toContain('"ApiKeyValue":""');
  });
});
