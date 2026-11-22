import { describe, expect, test } from "bun:test";
import { newRow } from "@doze/ui/components/app/key-values";
import { othersWords, aliasInput, aliasProblem, blankKey, callsOf, choose, createInput, policyOf, policySent, shownInput, specsFor, usagesFor } from "./create-model";

const ACCOUNT = "123456789012";
const O = { account: ACCOUNT, withPolicy: true, withLabels: true };

describe("the key choices", () => {
  test("symmetric keys encrypt, or make MACs with an HMAC spec; asymmetric keys sign, encrypt or agree", () => {
    expect(usagesFor("symmetric").map((u) => u.value)).toEqual(["ENCRYPT_DECRYPT", "GENERATE_VERIFY_MAC"]);
    expect(usagesFor("asymmetric").map((u) => u.value)).toEqual(["SIGN_VERIFY", "ENCRYPT_DECRYPT", "KEY_AGREEMENT"]);
    expect(specsFor("symmetric", "ENCRYPT_DECRYPT")).toEqual(["SYMMETRIC_DEFAULT"]);
    expect(specsFor("symmetric", "GENERATE_VERIFY_MAC")[0]).toBe("HMAC_224");
    expect(specsFor("asymmetric", "ENCRYPT_DECRYPT")).toEqual(["RSA_2048", "RSA_3072", "RSA_4096"]);
    expect(specsFor("asymmetric", "KEY_AGREEMENT")).not.toContain("RSA_2048");
  });
  test("choosing a kind moves the usage and spec to ones it allows, and keeps what still holds", () => {
    const asym = choose(blankKey(), { kind: "asymmetric" });
    expect([asym.usage, asym.spec]).toEqual(["SIGN_VERIFY", "RSA_2048"]);
    expect(choose(asym, { spec: "ECC_NIST_P256" }).spec).toBe("ECC_NIST_P256");
    expect(choose(choose(asym, { spec: "ECC_NIST_P256" }), { usage: "ENCRYPT_DECRYPT" }).spec).toBe("RSA_2048");
    expect(choose(asym, { kind: "symmetric" }).spec).toBe("SYMMETRIC_DEFAULT");
  });
});

describe("the alias", () => {
  test("it needs a name the service takes and nobody has", () => {
    expect(aliasProblem("", [])).toMatch(/Enter an alias/);
    expect(aliasProblem("alias/x", [])).toMatch(/Do not type alias\//);
    expect(aliasProblem("aws/s3", [])).toMatch(/aws\//);
    expect(aliasProblem("has space", [])).toMatch(/letters, digits/);
    expect(aliasProblem("larder-app", ["larder-app"])).toBe("larder-app is already an alias in this Region.");
    expect(aliasProblem("larder-reports", ["larder-app"])).toBeNull();
    expect(aliasProblem("a".repeat(257), [])).toMatch(/256/);
  });
});

describe("the calls", () => {
  test("the first steps send the spec and usage; labels add the description, tags and the alias call", () => {
    const f = { ...blankKey(), alias: "larder-reports", description: "Encrypts report-job output", tags: [newRow("Project", "larder"), newRow("", "")] };
    expect(createInput(f, { account: ACCOUNT, withPolicy: false, withLabels: false })).toEqual({ KeySpec: "SYMMETRIC_DEFAULT", KeyUsage: "ENCRYPT_DECRYPT" });
    const calls = callsOf(f, { account: ACCOUNT, withPolicy: false, withLabels: true });
    expect(calls.map((c) => c.operation)).toEqual(["CreateKey", "CreateAlias"]);
    expect(calls[0]!.input).toEqual({ KeySpec: "SYMMETRIC_DEFAULT", KeyUsage: "ENCRYPT_DECRYPT", Description: "Encrypts report-job output", Tags: [{ TagKey: "Project", TagValue: "larder" }] });
    expect(calls[1]!.input).toEqual(aliasInput(f));
    expect(aliasInput(f).AliasName).toBe("alias/larder-reports");
  });
  test("a multi-Region key is asked for with MultiRegion", () => {
    expect(createInput({ ...blankKey(), multiRegion: true }, O).MultiRegion).toBe(true);
    expect(createInput(blankKey(), O).MultiRegion).toBeUndefined();
  });
  test("the policy sent is the edited one, else the one the choices build, shown as a count of statements", () => {
    const f = { ...blankKey(), admins: [`arn:aws:iam::${ACCOUNT}:role/larder-admin`] };
    expect(JSON.parse(policySent(f, ACCOUNT)).Statement).toHaveLength(2);
    expect(policySent({ ...f, policy: "{}" }, ACCOUNT)).toBe("{}");
    expect(policyOf(f, ACCOUNT)).toContain("Allow access for Key Administrators");
    const sent = createInput(f, O);
    expect((shownInput("CreateKey", sent) as { Policy: string }).Policy).toBe("{ 2 statements }");
    expect(shownInput("CreateAlias", { AliasName: "x" })).toEqual({ AliasName: "x" });
  });
});

test("the other keys are said by what each is for", () => {
  expect(othersWords([{ aliases: ["larder-app"], id: "a", usage: "ENCRYPT_DECRYPT", state: "Enabled" }, { aliases: ["b"], id: "b", usage: "SIGN_VERIFY", state: "Enabled" }, { aliases: [], id: "z", usage: "ENCRYPT_DECRYPT", state: "Enabled" }, { aliases: ["gone"], id: "c", usage: "SIGN_VERIFY", state: "PendingDeletion" }])).toBe("b signs and verifies, larder-app encrypts and decrypts.");
  expect(othersWords([])).toBeNull();
});
