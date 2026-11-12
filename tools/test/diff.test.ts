import { afterAll, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync, cpSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { type DiffOptions, defaultOptions, runDiff } from "../board-diff.ts";
import { byId, failing, render, routes } from "./helpers.ts";
import { root } from "../lib/boards.ts";

const work = mkdtempSync(join(tmpdir(), "board-diff-test-"));
afterAll(() => rmSync(work, { recursive: true, force: true }));

let n = 0;
/** A fresh render directory holding the given pages (id -> html), a routes.json, and an allow.json. */
function setup(pages: Record<string, string>, allow: unknown = [], live: Record<string, string> = {}): DiffOptions {
  const dir = join(work, `case${++n}`);
  mkdirSync(join(dir, "fixture"), { recursive: true });
  mkdirSync(join(dir, "live"), { recursive: true });
  for (const [id, html] of Object.entries(pages)) writeFileSync(join(dir, "fixture", `${id}.html`), html);
  for (const [id, html] of Object.entries(live)) writeFileSync(join(dir, "live", `${id}.html`), html);
  writeFileSync(join(dir, "routes.json"), JSON.stringify(routes));
  writeFileSync(join(dir, "allow.json"), JSON.stringify(allow));
  return { ...defaultOptions(), dir: join(dir, "fixture"), liveDir: join(dir, "live"), routes: join(dir, "routes.json"), allow: join(dir, "allow.json") };
}
const page = (id: string): string => render(byId.get(id)!);
const mutate = (name: string, id: string): string => failing.find((m) => m.name === name)!.apply(page(id), byId.get(id)!)!;

describe("board-diff", () => {
  test("a board with no render is 'not built' and is not a failure", () => {
    const r = runDiff(setup({}));
    expect(r.ok).toBe(true);
    expect(r.summary).toMatchObject({ total: 96, built: 0, notBuilt: 96 });
  });

  test("clean renders pass; the others stay 'not built'", () => {
    const r = runDiff(setup({ home: page("home"), connect: page("connect") }));
    expect(r.code).toBe(0);
    expect(r.summary).toMatchObject({ built: 2, passed: 2, failed: 0, notBuilt: 94 });
  });

  test("--ids limits the boards compared", () => {
    const o = setup({ home: page("home"), connect: mutate("delete a button", "connect") });
    expect(runDiff({ ...o, ids: ["home"] }).code).toBe(0);
    expect(runDiff({ ...o, ids: ["connect"] }).code).toBe(1);
    expect(runDiff({ ...o, ids: ["nope"] }).code).toBe(2);
  });

  test("a difference fails with the missing element named", () => {
    const r = runDiff(setup({ home: mutate("delete a button", "home") }));
    expect(r.code).toBe(1);
    const diffs = r.boards.find((b) => b.id === "home")!.fixture!.differences;
    expect(diffs.length).toBeGreaterThan(0);
    expect(diffs[0]).toMatchObject({ kind: "missing", field: "button" });
  });

  test("--require makes a board that is not built a failure", () => {
    const o = setup({ home: page("home") });
    expect(runDiff({ ...o, require: ["home"] }).code).toBe(0);
    const r = runDiff({ ...o, require: ["home", "connect"] });
    expect(r.code).toBe(1);
    expect(r.errors.join("\n")).toContain('required board "connect" is not built');
    expect(runDiff({ ...o, require: "all" }).code).toBe(1);
  });

  test("the live render is compared on structure only: a different name passes, a missing column fails", () => {
    const strictOnly = mutate("change the h1 text", "topic-detail");
    const lone = runDiff(setup({}, [], { "topic-detail": strictOnly }));
    expect(lone.code).toBe(0); // h1 of a resource page is a name: not compared live
    const fixture = runDiff(setup({ "topic-detail": strictOnly }));
    expect(fixture.code).toBe(1); // ... but strictly it is
    const col = runDiff(setup({}, [], { "topic-detail": mutate("delete a table column", "topic-detail") }));
    expect(col.code).toBe(1);
    expect(col.boards.find((b) => b.id === "topic-detail")!.live!.differences[0].field).toBe("table.col");
  });

  test("boards.json that does not match the boards is refused (exit 2)", () => {
    const o = setup({});
    const bad = join(work, "stale-boards.json");
    writeFileSync(bad, "{}\n");
    const r = runDiff({ ...o, snapshot: bad });
    expect(r.code).toBe(2);
    expect(r.errors[0]).toContain("out of date");
  });
});

describe("the allow-list", () => {
  const html = (): string => mutate("delete a button", "home");
  const first = (): { field: string; value: string } => {
    const r = runDiff(setup({ home: html() }));
    const d = r.boards.find((b) => b.id === "home")!.fixture!.differences[0];
    return { field: d.field, value: d.value };
  };

  test("an entry silences exactly its difference and nothing else", () => {
    const { field, value } = first();
    const o = setup({ home: html() }, [{ board: "home", field, selector: value, reason: "the button is not built yet" }]);
    const r = runDiff(o);
    expect(r.code).toBe(0);
    expect(r.summary.allowed).toBe(1);
    expect(r.boards.find((b) => b.id === "home")!.fixture!.allowed[0].reason).toBe("the button is not built yet");

    // A second, different difference on the same board is not covered.
    const worse = mutate("remove the h1", "home").replace(/<(button|a)\b[^>]*class="btn[^"]*"[^>]*>[\s\S]*?<\/\1>/, "");
    const r2 = runDiff(setup({ home: worse }, [{ board: "home", field, selector: value, reason: "x" }]));
    expect(r2.code).toBe(1);
    expect(r2.boards.find((b) => b.id === "home")!.fixture!.differences.some((d) => d.field === "h1")).toBe(true);
  });

  test("an entry with a field and no selector silences every difference of that field on that board only", () => {
    const { field } = first();
    expect(runDiff(setup({ home: html() }, [{ board: "home", field, reason: "all of it" }])).code).toBe(0);
    // another board with the same mutation is not silenced
    const r = runDiff(setup({ home: html(), connect: mutate("delete a button", "connect") }, [{ board: "home", field, reason: "all of it" }]));
    expect(r.code).toBe(1);
    expect(r.boards.find((b) => b.id === "connect")!.fixture!.differences.length).toBeGreaterThan(0);
  });

  test("an entry with no reason is an error", () => {
    const { field, value } = first();
    for (const reason of [undefined, "", "   "]) {
      const r = runDiff(setup({ home: html() }, [{ board: "home", field, selector: value, reason }]));
      expect(r.code).toBe(1);
      expect(r.errors.join("\n")).toContain("needs a reason");
    }
  });

  test("an entry for an unknown board, or an unknown field, is an error", () => {
    const r = runDiff(setup({}, [{ board: "no-such-board", field: "button", reason: "x" }]));
    expect(r.code).toBe(1);
    expect(r.errors.join("\n")).toContain('unknown board "no-such-board"');
    const r2 = runDiff(setup({}, [{ board: "home", field: "colour", reason: "x" }]));
    expect(r2.errors.join("\n")).toContain("unknown field");
  });

  test("an entry that matches nothing is a stale-entry error", () => {
    const r = runDiff(setup({ home: page("home") }, [{ board: "home", field: "button", selector: "Gone [-] -> op:Nothing", reason: "was needed once" }]));
    expect(r.code).toBe(1);
    expect(r.errors.join("\n")).toContain("stale allow-list entry");
    // An entry for a board that is not built cannot be judged and is not stale.
    expect(runDiff(setup({}, [{ board: "home", field: "button", reason: "later" }])).code).toBe(0);
  });

  test("an entry expires with its slice", () => {
    const { field, value } = first();
    const entry = { board: "home", field, selector: value, reason: "until 9a", expires: "9a" };
    expect(runDiff({ ...setup({ home: html() }, [entry]), slice: "9a" }).code).toBe(0);
    const r = runDiff({ ...setup({ home: html() }, [entry]), slice: "9b" });
    expect(r.code).toBe(1);
    expect(r.errors.join("\n")).toContain("expired with slice 9a");
    expect(runDiff(setup({ home: html() }, [{ ...entry, expires: "9z" }])).errors.join("\n")).toContain("expires must be one of");
  });

  test("an entry limited to the live render does not silence the fixture render", () => {
    const { field, value } = first();
    const o = setup({ home: html() }, [{ board: "home", field, selector: value, reason: "live only", render: "live" }]);
    expect(runDiff(o).code).toBe(1);
  });
});

describe("the command line", () => {
  const run = (args: string[]) => Bun.spawnSync(["bun", join(root, "tools/board-diff.ts"), ...args], { cwd: root });

  test("exit codes, --list and --json", () => {
    const o = setup({ home: page("home"), connect: mutate("delete a button", "connect") });
    const args = ["--dir", o.dir, "--live-dir", o.liveDir, "--routes", o.routes, "--allow", o.allow];

    const bad = run(args);
    expect(bad.exitCode).toBe(1);
    const out = bad.stdout.toString();
    expect(out).toContain("FAIL connect (fixture)");
    expect(out).toContain("missing button");
    expect(out).toContain("ok   home (fixture)");

    expect(run([...args, "--ids", "home"]).exitCode).toBe(0);

    const list = run([...args, "--list"]);
    expect(list.exitCode).toBe(0);
    expect(list.stdout.toString()).toMatch(/home\s+home\s+fixture built, live not built/);
    expect(list.stdout.toString()).toMatch(/topic-detail\s+topics\s+fixture not built, live not built/);

    const json = JSON.parse(run([...args, "--json"]).stdout.toString());
    expect(json.summary).toMatchObject({ built: 2, passed: 1, failed: 1 });
    expect(json.boards.find((b: { id: string }) => b.id === "connect").fixture.differences[0].kind).toBe("missing");

    expect(run([...args, "--ids", "home", "--require", "connect"]).exitCode).toBe(1);
  });

  test("a copy of the whole design directory works from anywhere (paths are explicit)", () => {
    const o = setup({ home: page("home") });
    const copy = join(work, "design-copy");
    cpSync(join(root, "crates/kafka-console/design"), copy, { recursive: true });
    const r = run(["--dir", o.dir, "--routes", o.routes, "--allow", o.allow, "--boards", join(copy, "boards"), "--snapshot", join(copy, "boards.json")]);
    expect(r.exitCode).toBe(0);
  });
});
