import { describe, expect, test } from "bun:test";
import { credsFor, envBlock, snippets } from "./snippets";

describe("connect snippets", () => {
  test("trust mode carries test keys; verify leaves them to the environment", () => {
    expect(credsFor("trust")).toEqual({ key: "test", secret: "test" });
    expect(credsFor("verify")).toBeNull();
    expect(envBlock("http://127.0.0.1:4566", "eu-west-1", null)).toBe("export AWS_ENDPOINT_URL=http://127.0.0.1:4566\nexport AWS_REGION=eu-west-1");
  });

  test("every client names the endpoint and the region it was given", () => {
    const all = snippets("http://127.0.0.1:9999", "eu-west-2", credsFor("trust"));
    expect(all.map((s) => s.id)).toEqual(["cli", "js", "boto3", "go", "java", "rust"]);
    for (const s of all) {
      expect(s.code).toContain("http://127.0.0.1:9999");
      expect(s.code).toContain("eu-west-2");
      expect(s.s3.length).toBeGreaterThan(0);
    }
    expect(snippets("http://h", "r", null).some((s) => s.code.includes('"test"'))).toBe(false);
  });
});
