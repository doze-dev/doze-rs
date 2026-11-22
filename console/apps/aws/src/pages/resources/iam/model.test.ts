import { describe, expect, test } from "bun:test";
import { aliasProblem, allowsSentence, audienceKey, awsArn, blankTrust, breakPoints, providerName, daysSince, lacksResource, list, nameProblem, needsExternalId, permissions, policyArnOfName, sameIgnoringCase, sessionWords, shortKey, suggestedResource, takenProblem, trustActions, trustDocument, trustedEntities, trustPrincipals, trustProblem, trustSentence } from "./model";

const ACCOUNT = "123456789012";
const trust = (principal: unknown, extra: object = {}) => JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: principal, Action: "sts:AssumeRole", ...extra }] });
const both = trust({ AWS: [`arn:aws:iam::${ACCOUNT}:user/ci-bot`, `arn:aws:iam::${ACCOUNT}:user/dev`] });

describe("words", () => {
  test("a list is joined as a sentence", () => {
    expect(list([])).toBe("");
    expect(list(["a"])).toBe("a");
    expect(list(["a", "b"])).toBe("a and b");
    expect(list(["a", "b", "c"])).toBe("a, b and c");
  });
  test("a key is cut for a strip and aged in whole days", () => {
    expect(shortKey("AKIAQ4HZLARDER7DEV2K")).toBe("AKIA...7DEV2K");
    const now = Date.UTC(2026, 10, 8, 12);
    expect(daysSince(now / 1000 - 3600, now)).toBe("0 days");
    expect(daysSince(now / 1000 - 86_400, now)).toBe("1 day");
    expect(daysSince(now / 1000 - 2 * 86_400 - 5, now)).toBe("2 days");
  });
  test("a session length is said in hours when it is whole ones", () => {
    expect(sessionWords(3600)).toBe("1 hour");
    expect(sessionWords(43_200)).toBe("12 hours");
    expect(sessionWords(900)).toBe("15 minutes");
  });
});

describe("names", () => {
  test("a name has IAM's characters and length", () => {
    expect(nameProblem("dev-intern", "user")).toBeNull();
    expect(nameProblem("", "user")).toBe("Enter a user name.");
    expect(nameProblem("a b", "role")).toContain("only letters, digits");
    expect(nameProblem("x".repeat(65), "role")).toContain("at most 64");
    expect(nameProblem("x".repeat(65), "group")).toBeNull();
  });
  test("a name that differs only in case is found", () => {
    expect(sameIgnoringCase("TESTUSER", ["testuser", "dev"])).toEqual(["testuser"]);
    expect(sameIgnoringCase("dev", ["dev"])).toEqual([]);
  });
  test("an alias is 3 to 63 of lowercase, digits and single hyphens, with a letter or digit at each end", () => {
    expect(aliasProblem("larder-dev")).toBeNull();
    expect(aliasProblem("ab")).toContain("3 to 63");
    expect(aliasProblem("Larder")).toContain("lowercase");
    expect(aliasProblem("-larder")).toContain("starts and ends");
    expect(aliasProblem("larder-")).toContain("starts and ends");
    expect(aliasProblem("lar--der")).toContain("two hyphens");
    expect(aliasProblem("a".repeat(64))).toContain("3 to 63");
  });
});

describe("trust policies", () => {
  test("the principals a policy names are read, each once", () => {
    expect(trustPrincipals(both).map((p) => `${p.kind}:${p.label}`)).toEqual(["user:ci-bot", "user:dev"]);
    expect(trustPrincipals(trust({ Service: "lambda.amazonaws.com" })).map((p) => `${p.kind}:${p.label}`)).toEqual(["service:lambda"]);
    expect(trustPrincipals(trust({ AWS: `arn:aws:iam::${ACCOUNT}:root` }))[0]).toMatchObject({ kind: "root", account: ACCOUNT });
    expect(trustPrincipals(trust("*"))[0]?.kind).toBe("any");
    expect(trustPrincipals("not json")).toEqual([]);
  });
  test("the column names the account or the service that may assume it", () => {
    expect(trustedEntities(both)).toEqual([`AWS account: ${ACCOUNT}`]);
    expect(trustedEntities(trust({ Service: "lambda.amazonaws.com" }))).toEqual(["AWS service: lambda"]);
    expect(trustedEntities(trust({ Federated: "arn:aws:iam::1:saml-provider/x" }))[0]).toStartWith("Identity provider:");
  });
  test("the sentence says who is named and what they may do", () => {
    expect(trustSentence("report-job", both, ACCOUNT)).toBe("report-job names two principals in this account, ci-bot and dev, and one action, sts:AssumeRole.");
    expect(trustSentence("lambda-exec", trust({ Service: "lambda.amazonaws.com" }), ACCOUNT)).toBe("lambda-exec names the service lambda, and one action, sts:AssumeRole.");
    expect(trustSentence("larder-admin", trust({ AWS: `arn:aws:iam::${ACCOUNT}:root` }), ACCOUNT)).toBe(`larder-admin names the whole account ${ACCOUNT}, and one action, sts:AssumeRole.`);
    expect(trustSentence("x", JSON.stringify({ Statement: [] }), ACCOUNT)).toContain("names no principal");
    expect(trustActions(both)).toEqual(["sts:AssumeRole"]);
  });
  test("an external ID is seen in the condition", () => {
    expect(needsExternalId(trust({ AWS: "1" }, { Condition: { StringEquals: { "sts:ExternalId": "x" } } }))).toBe(true);
    expect(needsExternalId(both)).toBe(false);
  });
  test("a choice builds the trust policy it names", () => {
    expect(JSON.parse(trustDocument(blankTrust, ACCOUNT)).Statement[0].Principal).toEqual({ AWS: `arn:aws:iam::${ACCOUNT}:root` });
    expect(JSON.parse(trustDocument({ ...blankTrust, type: "service", service: "states.amazonaws.com" }, ACCOUNT)).Statement[0].Principal).toEqual({ Service: "states.amazonaws.com" });
    const other = JSON.parse(trustDocument({ ...blankTrust, account: "other", other: "210987654321", external: true, externalId: "abc" }, ACCOUNT)).Statement[0];
    expect(other.Principal).toEqual({ AWS: "arn:aws:iam::210987654321:root" });
    expect(other.Condition).toEqual({ StringEquals: { "sts:ExternalId": "abc" } });
    expect(JSON.parse(trustDocument({ ...blankTrust, mfa: true }, ACCOUNT)).Statement[0].Condition).toEqual({ Bool: { "aws:MultiFactorAuthPresent": "true" } });
    expect(trustDocument({ ...blankTrust, type: "custom", custom: "{}" }, ACCOUNT)).toBe("{}");
  });
  test("a choice that cannot be built says why", () => {
    expect(trustProblem(blankTrust)).toBeNull();
    expect(trustProblem({ ...blankTrust, account: "other", other: "12" })).toBe("An account ID is 12 digits.");
    expect(trustProblem({ ...blankTrust, external: true, externalId: "" })).toContain("external ID");
    expect(trustProblem({ ...blankTrust, type: "custom", custom: "{" })).toContain("not valid JSON");
    expect(trustProblem({ ...blankTrust, type: "custom", custom: trust("*") })).toBeNull();
  });
});

describe("policy documents", () => {
  const doc = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Action: ["sqs:SendMessage", "s3:GetObject"], Resource: "arn:aws:sqs:us-east-1:1:pick-lists" }, { Effect: "Deny", Action: "s3:*", Resource: "*" }] });
  test("the permissions are the Allow statements, a row per service", () => {
    expect(permissions(doc)).toEqual([
      { service: "SQS", actions: "sqs:SendMessage", resource: "arn:aws:sqs:us-east-1:1:pick-lists" },
      { service: "S3", actions: "s3:GetObject", resource: "arn:aws:sqs:us-east-1:1:pick-lists" },
    ]);
  });
  test("a statement without a Resource is found, and a resource is suggested for its service", () => {
    const bad = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Action: ["sqs:SendMessage"] }] });
    expect(lacksResource(bad)).toBe(true);
    expect(lacksResource(doc)).toBe(false);
    expect(suggestedResource(bad, ACCOUNT, "us-east-1")).toBe(`arn:aws:sqs:us-east-1:${ACCOUNT}:pick-lists`);
  });
  test("an AWS managed policy has its ARN from its name", () => {
    expect(awsArn("PowerUserAccess")).toBe("arn:aws:iam::aws:policy/PowerUserAccess");
  });
});

describe("documents as IAM sends them", () => {
  test("a URL-encoded policy is read as the JSON it is", async () => {
    const { docText } = await import("./model");
    expect(docText("%7B%22Version%22%3A%222012-10-17%22%7D")).toBe('{"Version":"2012-10-17"}');
    expect(docText('{"a":1}')).toBe('{"a":1}');
    expect(docText({ a: 1 })).toBe('{"a":1}');
    expect(permissions("%7B%22Statement%22%3A%5B%7B%22Effect%22%3A%22Allow%22%2C%22Action%22%3A%22s3%3AGetObject%22%2C%22Resource%22%3A%22%2A%22%7D%5D%7D")[0]?.actions).toBe("s3:GetObject");
  });
});

describe("a policy named by its bare name", () => {
  test("is the policy of that name in this account", () => {
    expect(policyArnOfName(ACCOUNT, "orders-read")).toBe(`arn:aws:iam::${ACCOUNT}:policy/orders-read`);
    expect(policyArnOfName(ACCOUNT, "a+b=c,d.e@f_g-h")).toBe(`arn:aws:iam::${ACCOUNT}:policy/a+b=c,d.e@f_g-h`);
  });
  test("an ARN, an inline policy's pseudo-name and a name that cannot be one are left alone", () => {
    expect(policyArnOfName(ACCOUNT, `arn:aws:iam::${ACCOUNT}:policy/orders-read`)).toBeNull();
    expect(policyArnOfName(ACCOUNT, "arn:aws:iam::aws:policy/AdministratorAccess")).toBeNull();
    expect(policyArnOfName(ACCOUNT, "inline:dev:read")).toBeNull();
    expect(policyArnOfName(ACCOUNT, "")).toBeNull();
    expect(policyArnOfName(ACCOUNT, "x".repeat(129))).toBeNull();
  });
});

describe("a name that is taken", () => {
  test("is said with the name that holds it, and a name that differs only in case is the same name", () => {
    expect(takenProblem("user", "dev", ["dev", "ci-bot"])).toBe("A user named dev already exists.");
    expect(takenProblem("user", "DEV", ["dev", "ci-bot"])).toBe("A user named dev already exists; names are not told apart by case.");
    expect(takenProblem("user", "ops", ["dev"])).toBeNull();
  });
});

describe("where a resource may wrap", () => {
  test("after each colon and slash, and the pieces are the text", () => {
    const arn = "arn:aws:sqs:us-east-1:000000000000:pick-lists";
    expect(breakPoints(arn)).toEqual(["arn:", "aws:", "sqs:", "us-east-1:", "000000000000:", "pick-lists"]);
    expect(breakPoints("arn:aws:s3:::bucket/key/*").join("")).toBe("arn:aws:s3:::bucket/key/*");
    expect(breakPoints("*")).toEqual(["*"]);
    expect(breakPoints("")).toEqual([]);
  });
});

describe("the federated trust policies", () => {
  const oidc = `arn:aws:iam::${ACCOUNT}:oidc-provider/token.actions.githubusercontent.com`;
  const saml = `arn:aws:iam::${ACCOUNT}:saml-provider/okta`;
  test("a provider is named by what its ARN ends with, and the audience key by its URL", () => {
    expect(providerName(oidc)).toBe("token.actions.githubusercontent.com");
    expect(providerName(saml)).toBe("okta");
    expect(audienceKey(oidc)).toBe("token.actions.githubusercontent.com:aud");
    expect(audienceKey(`arn:aws:iam::${ACCOUNT}:oidc-provider/idp.example.com/realms/larder`)).toBe("idp.example.com/realms/larder:aud");
  });
  test("web identity trusts the provider to AssumeRoleWithWebIdentity for the audience", () => {
    const doc = JSON.parse(trustDocument({ ...blankTrust, type: "web", provider: oidc, audience: " sts.amazonaws.com " }, ACCOUNT)) as { Version: string; Statement: unknown[] };
    expect(doc.Version).toBe("2012-10-17");
    expect(doc.Statement).toEqual([{ Effect: "Allow", Principal: { Federated: oidc }, Action: "sts:AssumeRoleWithWebIdentity", Condition: { StringEquals: { "token.actions.githubusercontent.com:aud": "sts.amazonaws.com" } } }]);
  });
  test("SAML trusts the provider to AssumeRoleWithSAML, for the API alone or with the sign-in page", () => {
    const aud = (console: boolean) => (JSON.parse(trustDocument({ ...blankTrust, type: "saml", provider: saml, console }, ACCOUNT)) as { Statement: { Condition: { StringEquals: Record<string, string> } }[] }).Statement[0]!.Condition.StringEquals["SAML:aud"];
    expect(aud(false)).toBe("https://localhost/saml");
    expect(aud(true)).toBe("https://signin.aws.amazon.com/saml");
    expect(JSON.parse(trustDocument({ ...blankTrust, type: "saml", provider: saml }, ACCOUNT)).Statement[0].Action).toBe("sts:AssumeRoleWithSAML");
  });
  test("a choice with no provider (or no audience) is not built, and what it says is a placeholder", () => {
    expect(trustProblem({ ...blankTrust, type: "web" })).toBe("Choose an identity provider.");
    expect(trustProblem({ ...blankTrust, type: "web", provider: oidc })).toBe("Enter the audience.");
    expect(trustProblem({ ...blankTrust, type: "web", provider: oidc, audience: "a" })).toBeNull();
    expect(trustProblem({ ...blankTrust, type: "saml" })).toBe("Choose a SAML provider.");
    expect(trustProblem({ ...blankTrust, type: "saml", provider: saml })).toBeNull();
    expect(trustDocument({ ...blankTrust, type: "web" }, ACCOUNT)).toContain("<provider>:aud");
  });
});

describe("what a policy being written allows, in a line", () => {
  const one = (action: unknown, resource: unknown, effect = "Allow") => JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: effect, Action: action, Resource: resource }] });
  test("one action on one queue is said as the board says it", () => {
    expect(allowsSentence(one(["sqs:SendMessage"], "arn:aws:sqs:us-east-1:123456789012:pick-lists"))).toBe("This one allows only sqs:SendMessage, on the one queue pick-lists.");
  });
  test("several actions, a wildcard resource, many resources, an ARN of no known kind", () => {
    expect(allowsSentence(one(["sqs:SendMessage", "sqs:ReceiveMessage"], "*"))).toBe("This one allows sqs:SendMessage and sqs:ReceiveMessage, on every resource.");
    expect(allowsSentence(one(["s3:GetObject", "s3:PutObject", "s3:DeleteObject", "s3:ListBucket"], ["arn:aws:s3:::a/*", "arn:aws:s3:::b/*"]))).toBe("This one allows 4 actions, on 2 resources.");
    expect(allowsSentence(one("*", "arn:aws:iam::123456789012:role/x"))).toBe("This one allows every action, on arn:aws:iam::123456789012:role/x.");
    expect(allowsSentence(one("dynamodb:GetItem", "arn:aws:dynamodb:us-east-1:123456789012:table/orders"))).toBe("This one allows only dynamodb:GetItem, on the one table orders.");
  });
  test("a policy that allows nothing, or is not JSON, has no line", () => {
    expect(allowsSentence(one(["sqs:SendMessage"], "*", "Deny"))).toBeNull();
    expect(allowsSentence("{ nope")).toBeNull();
  });
});
