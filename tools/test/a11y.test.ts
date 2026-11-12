import { afterAll, describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { applyKnown, contrastPairs, lintContrast, lintMotion, lintPage, ratio } from "../a11y-lint.ts";
import { kitDir, root } from "../lib/boards.ts";
import { boards, byId, render } from "./helpers.ts";

const known = JSON.parse(readFileSync(join(import.meta.dir, "a11y-known.json"), "utf8")) as Array<{ board: string; check: string; reason: string }>;
const checks = (html: string): string[] => lintPage(html, "x.html").findings.map((f) => f.check);
const into = (html: string, extra: string): string => html.replace("</main>", `${extra}</main>`);

// A page that is clean by itself, and one with an overlay.
const clean = render(byId.get("topics-list")!);
const withLive = render(byId.get("home")!); // has data-extra="live-sentence"

describe("the lint on the rendered boards", () => {
  test("the rendered boards are clean, except the canvas defects listed in a11y-known.json (each with a reason)", () => {
    const findings = boards.flatMap((b) => lintPage(render(b), `${b.id}.html`).findings);
    const { failing, knownFindings, errors } = applyKnown(findings, known);
    expect(failing.map((f) => `${f.file} [${f.check}] ${f.message}`)).toEqual([]);
    expect(errors).toEqual([]);
    expect(knownFindings.length).toBe(findings.length);
    for (const k of known) {
      expect(k.reason.length, `${k.board} ${k.check}`).toBeGreaterThan(20);
      expect(byId.has(k.board)).toBe(true);
    }
  });

  test("the kit CSS meets WCAG AA for text and 3:1 for glyph hues; it has no animation", () => {
    const css = ["paper.css", "kafka.css"].map((f) => readFileSync(join(kitDir, f), "utf8")).join("\n");
    expect(lintContrast(css, "kit")).toEqual([]);
    expect(lintMotion(css, "kit")).toEqual([]);
    const pairs = contrastPairs(css);
    expect(pairs.length).toBeGreaterThan(25);
    // A few pairs the console depends on, computed from the tokens.
    const find = (fg: string, bg: string) => pairs.find((p) => p.fg === fg && p.bg === bg);
    expect(find("#6f6888", "#ffffff")!.ratio).toBeGreaterThan(5); // --dim on --panel
    expect(find("#ffffff", "#7a4fe0")!.ratio).toBeGreaterThan(4.5); // white on --accent (primary button)
  });

  test("the contrast arithmetic matches the WCAG examples", () => {
    expect(ratio([0, 0, 0], [255, 255, 255])).toBeCloseTo(21, 5);
    expect(ratio([119, 119, 119], [255, 255, 255])).toBeCloseTo(4.48, 2);
  });
});

describe("a planted defect of each kind is found", () => {
  test("the base pages are clean", () => {
    expect(checks(clean)).toEqual([]);
    expect(checks(withLive)).toEqual([]);
  });

  test("lang", () => {
    expect(checks(clean.replace(' lang="en"', ""))).toEqual(["lang"]);
    expect(checks(clean.replace('lang="en"', 'lang=""'))).toEqual(["lang"]);
  });

  test("h1: none, two", () => {
    expect(checks(clean.replace(/<h1\b/, "<h2").replace("</h1>", "</h2>"))).toContain("h1");
    expect(checks(into(clean, "<h1>Another</h1>"))).toContain("h1");
  });

  test("heading-order", () => {
    const f = lintPage(into(clean, "<h4>Skipped</h4>"), "x.html").findings;
    expect(f.map((x) => x.check)).toEqual(["heading-order"]);
    expect(f[0].message).toContain("h4");
  });

  test("name: an icon-only button, an empty link, an unlabelled input", () => {
    expect(lintPage(into(clean, '<button class="btn" type="button"><svg aria-hidden="true"></svg></button>'), "x").findings.map((x) => x.check)).toEqual(["name"]);
    expect(checks(into(clean, '<a href="/somewhere"><svg aria-hidden="true"></svg></a>'))).toEqual(["name"]);
    expect(checks(into(clean, '<input type="text" placeholder="only a placeholder">'))).toEqual(["name"]);
    expect(checks(into(clean, "<select><option>a</option></select>"))).toEqual(["name"]);
    expect(checks(into(clean, "<textarea></textarea>"))).toEqual(["name"]);
  });

  test("name: every way of giving one is accepted", () => {
    const named = [
      '<button type="button" aria-label="Close"></button>',
      '<input type="text" aria-label="Find">',
      '<label for="q">Find</label><input id="q" type="text">',
      '<label>Find <input type="text"></label>',
      '<span id="n">Name</span><input type="text" aria-labelledby="n">',
      '<a href="/x" title="Open">&nbsp;</a>',
      '<input type="submit" value="Go">',
      '<a href="/x"><img src="/i.png" alt="Home"></a>',
    ];
    for (const n of named) expect(checks(into(clean, n)), n).toEqual([]);
    // Hidden and href-less anchors are not controls.
    expect(checks(into(clean, '<a name="top"></a><button hidden></button><input type="hidden">'))).toEqual([]);
  });

  test("th-scope: a body th without scope", () => {
    expect(checks(into(clean, "<table><tbody><tr><th>Row</th><td>x</td></tr></tbody></table>"))).toEqual(["th-scope"]);
    expect(checks(into(clean, '<table><tbody><tr><th scope="row">Row</th><td>x</td></tr></tbody></table>'))).toEqual([]);
    expect(checks(into(clean, "<table><thead><tr><th>Col</th></tr></thead></table>"))).toEqual([]);
  });

  test("tabindex: positive only", () => {
    expect(checks(into(clean, '<div tabindex="3">x</div>'))).toEqual(["tabindex"]);
    expect(checks(into(clean, '<div tabindex="0">x</div><div tabindex="-1">y</div>'))).toEqual([]);
  });

  test("dialog-label: missing, dangling, and present", () => {
    const dlg = render(byId.get("topic-delete")!);
    expect(checks(dlg)).toEqual([]);
    expect(checks(dlg.replace(' aria-labelledby="dlg-title"', ""))).toEqual(["dialog-label"]);
    expect(checks(dlg.replace('aria-labelledby="dlg-title"', 'aria-labelledby="nothing"'))).toEqual(["dialog-label"]);
    // a dialog page has no h1: that is fine; a dialog page with a stray h1 on top of the dialog is fine too
    expect(checks(dlg)).not.toContain("h1");
  });

  test("live-region: only where the page has a live sentence", () => {
    const quiet = render(boards.find((b) => b.fm.kind === "page" && !b.html.includes("live-sentence"))!);
    expect(checks(quiet)).toEqual([]);
    expect(checks(into(quiet, '<div aria-live="polite">3 new</div>'))).toEqual(["live-region"]);
    expect(checks(into(quiet, '<div role="status">3 new</div>'))).toEqual(["live-region"]);
    expect(checks(into(withLive, '<div aria-live="polite">3 new</div>'))).toEqual([]);
    expect(checks(into(quiet, '<div aria-live="off">x</div>'))).toEqual([]);
  });

  test("status-word: a state with no word", () => {
    expect(checks(into(clean, '<span class="st ok"><svg aria-hidden="true"></svg></span>'))).toEqual(["status-word"]);
    expect(checks(into(clean, '<span class="rw refused"></span>'))).toEqual(["status-word"]);
    expect(checks(into(clean, '<span class="st ok">Reachable</span>'))).toEqual([]);
  });

  test("motion: animation without a reduced-motion block", () => {
    expect(lintMotion(".a{transition:opacity .2s}", "c").map((f) => f.check)).toEqual(["motion"]);
    expect(lintMotion("@keyframes spin{to{transform:rotate(1turn)}}.a{animation:spin 1s infinite}", "c").length).toBe(1);
    expect(lintMotion(".a{transition:opacity .2s}@media (prefers-reduced-motion: reduce){.a{transition:none}}", "c")).toEqual([]);
    expect(lintMotion(".a{color:red}", "c")).toEqual([]);
  });

  test("contrast: a failing pair is reported with its ratio, and var() tokens are resolved", () => {
    const css = ".root{--text:#3a3550;--bg:#ffffff;--panel:#ffffff;--well:#f6f4fb;--faint:#aaaaaa}.hint{color:var(--faint)}.ok{color:var(--text)}.btn{color:#fff;background:#999999}";
    const f = lintContrast(css, "x.css");
    expect(f.every((x) => x.check === "contrast")).toBe(true);
    expect(f.some((x) => x.message.includes("#aaaaaa on #ffffff") && x.message.includes("2.32:1") && x.message.includes(".hint"))).toBe(true);
    expect(f.some((x) => x.message.includes("#ffffff on #999999") && x.message.includes(".btn"))).toBe(true);
    expect(f.some((x) => x.message.includes("#3a3550"))).toBe(false);
  });

  test("contrast: glyph hues are held to 3:1, opacity-disabled controls are exempt, decoration is skipped", () => {
    const tokens = ".root{--bg:#ffffff;--panel:#ffffff;--well:#ffffff;--text:#000000;--g:#8c8c8c}";
    expect(lintContrast(tokens + ".d-msg{color:var(--g)}", "x")).toEqual([]); // 3.4:1 passes as a graphic
    expect(lintContrast(tokens + ".label{color:var(--g)}", "x").length).toBe(1); // ... but not as text
    expect(lintContrast(tokens + ".btn.dis{color:var(--g);opacity:.5}", "x")).toEqual([]);
    expect(lintContrast(tokens + ".crumbs svg{color:#eeeeee}.x::before{color:#eeeeee}", "x")).toEqual([]);
  });
});

describe("known findings", () => {
  const f = { file: "/t/topics-none.html", check: "heading-order", message: "m" };
  test("a known finding is listed, not failed; a reason is required; a stale entry is an error", () => {
    const r = applyKnown([f], [{ board: "topics-none", check: "heading-order", reason: "empty state uses h3" }]);
    expect(r.failing).toEqual([]);
    expect(r.knownFindings.length).toBe(1);
    expect(r.errors).toEqual([]);
    expect(applyKnown([f], [{ board: "topics-none", check: "heading-order", reason: " " }]).errors[0]).toContain("reason is required");
    expect(applyKnown([], [{ board: "topics-none", check: "heading-order", reason: "x" }]).errors[0]).toContain("stale known entry");
    expect(applyKnown([f], [{ board: "other", check: "heading-order", reason: "x" }]).failing.length).toBe(1);
  });
});

describe("the command line", () => {
  const dir = mkdtempSync(join(tmpdir(), "a11y-test-"));
  afterAll(() => rmSync(dir, { recursive: true, force: true }));
  const run = (args: string[]) => Bun.spawnSync(["bun", join(root, "tools/a11y-lint.ts"), ...args], { cwd: root });

  test("exit status, --warn, --known, --json", () => {
    writeFileSync(join(dir, "good.html"), clean);
    writeFileSync(join(dir, "bad.html"), into(clean, "<h4>skip</h4>"));
    const css = join(kitDir, "paper.css") + "," + join(kitDir, "kafka.css");
    expect(run([join(dir, "good.html"), "--css", css]).exitCode).toBe(0);
    const bad = run([join(dir, "bad.html"), "--css", css]);
    expect(bad.exitCode).toBe(1);
    expect(bad.stdout.toString()).toContain("FAIL bad.html [heading-order]");
    expect(run([join(dir, "bad.html"), "--css", css, "--warn"]).exitCode).toBe(0);
    writeFileSync(join(dir, "known.json"), JSON.stringify([{ board: "bad", check: "heading-order", reason: "planted" }]));
    expect(run([join(dir, "bad.html"), "--css", css, "--known", join(dir, "known.json")]).exitCode).toBe(0);
    const json = JSON.parse(run([join(dir, "bad.html"), "--css", css, "--json"]).stdout.toString());
    expect(json.findings[0].check).toBe("heading-order");
  });
});
