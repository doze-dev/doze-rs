import { describe, expect, test } from "bun:test";
import { existsSync } from "node:fs";
import { shellWord } from "./shell";

const HOSTILE = [
  "x; open -a Calculator #",
  "$(touch /tmp/doze-pwned-web)",
  "`touch /tmp/doze-pwned-web`",
  "a'b",
  'a"b',
  "line\nnext",
  "two words",
  "x | sh",
  "''",
  "",
];

describe("shellWord", () => {
  test("is the server's shell_word: plain words stay, others are single-quoted", () => {
    expect(shellWord("payments.settled")).toBe("payments.settled");
    expect(shellWord("127.0.0.1:9092")).toBe("127.0.0.1:9092");
    expect(shellWord("a b")).toBe("'a b'");
    expect(shellWord("x'; rm -rf /; '")).toBe("'x'\\''; rm -rf /; '\\'''");
    expect(shellWord("")).toBe("''");
    expect(shellWord("$(id)")).toBe("'$(id)'");
    expect(shellWord("é")).toBe("'é'");
  });

  test("a real shell reads every hostile word back as one word and runs none of it", () => {
    const words = HOSTILE.map(shellWord).join(" ");
    const run = Bun.spawnSync(["/bin/sh", "-c", `set -- ${words}; printf '%s\\0' "$@"`]);
    expect(run.exitCode).toBe(0);
    const back = run.stdout.toString().split("\0").slice(0, -1);
    expect(back).toEqual(HOSTILE);
    expect(existsSync("/tmp/doze-pwned-web")).toBe(false);
  });
});
