#!/usr/bin/env bun
// Records the S4 corpus: JSONata expressions inside single Pass states, run through states:TestState on the recorder account.
//   bun tools/aws-rec/asl-corpus.ts          writes docs/port/aws/spikes/S4-corpus.json
// TestState is free and creates nothing, so there is nothing to sweep. Needs: aws sso login --profile doze-rec-main
import { assertRecorder, PROFILE } from "./lib.ts";

type Case = { id: string; group: string; expr: string; input?: unknown; assign?: Record<string, string> };

const I = { n: 7, s: "Hello, World", xs: [3, 1, 2, 10, 20], o: { a: 1, b: { c: [1, 2, { d: "deep" }] } },
  people: [{ name: "Ann", age: 31, tags: ["a", "b"] }, { name: "Bob", age: 25, tags: [] }, { name: "Cy", age: 40, tags: ["c"] }],
  empty: [], nothing: null, t: true, f: false, price: 12.5, text: "the quick brown fox", csv: "a,b,,c" };

const g = (group: string, exprs: string[]): Case[] => exprs.map((expr, i) => ({ id: `${group}-${i + 1}`, group, expr, input: I }));

const cases: Case[] = [
  ...g("arith", ["1 + 2 * 3", "(1 + 2) * 3", "$states.input.n * 2", "$states.input.n / 2", "$states.input.n % 4", "10 / 4", "-$states.input.n",
    "0.1 + 0.2", "1e3 + 1", "$states.input.price * 3", "5 / 0", "2 * 3 * 4 * 5 * 6 * 7 * 8 * 9 * 10 * 11 * 12 * 13 * 14 * 15 * 16 * 17 * 18 * 19 * 20 * 21 * 22"]),
  ...g("compare", ["$states.input.n = 7", "$states.input.n != 7", "$states.input.n > 3 and $states.input.n < 10", "$states.input.s = \"Hello, World\"",
    "\"a\" < \"b\"", "$states.input.n in [1, 7, 9]", "$states.input.t and $states.input.f", "$states.input.t or $states.input.f", "$not($states.input.t)",
    "$states.input.nothing = null", "1 = \"1\"", "[1,2] = [1,2]"]),
  ...g("string", ["$uppercase($states.input.s)", "$lowercase($states.input.s)", "$length($states.input.s)", "$substring($states.input.s, 0, 5)", "$substring($states.input.s, -5)",
    "$substringBefore($states.input.s, \",\")", "$substringAfter($states.input.s, \", \")", "$trim(\"  hi   there  \")", "$pad(\"ab\", 5, \"*\")", "$pad(\"ab\", -5, \"*\")",
    "$contains($states.input.s, \"World\")", "$split($states.input.csv, \",\")", "$join([\"a\",\"b\",\"c\"], \"-\")", "$replace($states.input.s, \"o\", \"0\")",
    "$string($states.input.n) & \"!\"", "$string($states.input.o)", "$string(1/3)", "$string(1e21)", "$string(123456789012345678)", "$number(\"42.5\")", "$number(\"x\")",
    "$formatNumber(1234.5678, \"#,##0.00\")", "$base64encode(\"hello\")", "$base64decode(\"aGVsbG8=\")", "$encodeUrlComponent(\"a b&c\")", "$decodeUrlComponent(\"a%20b\")",
    "$string(true)", "$string(null)", "$string([1,\"a\"])", "$length(\"héllo\")", "$uppercase(\"straße\")"]),
  ...g("number", ["$round(2.5)", "$round(3.5)", "$round(-2.5)", "$round(1.2345, 2)", "$floor(-1.5)", "$ceil(1.2)", "$abs(-3)", "$sqrt(16)", "$power(2, 10)", "$power(2, 0.5)",
    "$sum($states.input.xs)", "$average($states.input.xs)", "$max($states.input.xs)", "$min($states.input.xs)", "$sum([])", "$max([])", "$formatBase(255, 16)", "$formatBase(10, 2)",
    "$number(\"0x1F\")", "$number(\"1e2\")", "9007199254740993", "$states.input.n / 3", "100000000000000000000", "0.000001", "1.0", "5e-7"]),
  ...g("array", ["$count($states.input.xs)", "$append([1,2],[3])", "$reverse($states.input.xs)", "$sort($states.input.xs)", "$sort($states.input.xs, function($a,$b){$b > $a})",
    "$distinct([1,2,2,3,1])", "$map($states.input.xs, function($v){$v * 2})", "$map($states.input.xs, function($v,$i){$v + $i})", "$filter($states.input.xs, function($v){$v > 2})",
    "$reduce($states.input.xs, function($a,$b){$a + $b})", "$reduce($states.input.xs, function($a,$b){$a + $b}, 100)", "$zip([1,2,3],[\"a\",\"b\"])", "[1..5]", "[5..1]", "$exists($states.input.empty)",
    "$shuffle([1])", "$states.input.xs[0]", "$states.input.xs[-1]", "$states.input.xs[1..2]", "$states.input.xs[$ > 2]", "$states.input.empty[0]", "[1,[2,[3]]]", "[[1,2],[3]].$", "$flatten"]),
  ...g("object", ["$keys($states.input.o)", "$lookup($states.input.o, \"a\")", "$merge([{\"a\":1},{\"b\":2},{\"a\":3}])", "$spread({\"a\":1,\"b\":2})", "$each({\"a\":1,\"b\":2}, function($v,$k){$k & $string($v)})",
    "$sift($states.input.o, function($v,$k){$k = \"a\"})", "{\"x\": 1, \"y\": [1,2]}", "{\"k\": $states.input.n}", "$type($states.input.o)", "$type($states.input.xs)", "$type($states.input.s)", "$type($states.input.n)",
    "$type($states.input.nothing)", "$type($states.input.t)", "$type($states.input.missing)", "$type($string)", "$exists($states.input.missing)", "$exists($states.input.o.a)"]),
  ...g("path", ["$states.input.o.b.c[2].d", "$states.input.o.b.c.d", "$states.input.people.name", "$states.input.people[age > 30].name", "$states.input.people[0].tags", "$states.input.people.tags",
    "$states.input.people.age ~> $sum()", "$states.input.people{name: age}", "$states.input.people.{\"n\": name, \"old\": age > 30}", "$states.input.people[name = \"Bob\"].age", "$states.input.people^(age).name",
    "$states.input.people^(>age).name", "$states.input.*.a", "$states.input.o.**.d", "$states.input.missing.deeper", "$states.input.people[age > 99]", "$count($states.input.people[tags])",
    "$states.input.people[0].tags[0]", "$states.input.people#$i[$i > 0].name", "$states.input.n ~> $string()", "$states.input.people@$p.$p.name", "$states.input"]),
  ...g("cond", ["$states.input.n > 5 ? \"big\" : \"small\"", "$states.input.missing ? 1 : 2", "$states.input.n > 100 ? \"x\"", "($x := 5; $x * 2)", "($f := function($a){$a + 1}; $f(2))",
    "($a := 1; $b := $a + 1; [$a, $b])", "function($x){$x * 2}(4)", "$states.input.xs ~> $map(function($v){$v + 1}) ~> $sum()", "$boolean(\"\")", "$boolean(\"x\")", "$boolean(0)", "$boolean([])",
    "$boolean([0])", "$boolean({})", "$boolean(null)", "$not(1)", "$exists(null)", "$states.input.nothing ?: 5", "$states.input.s ?? \"d\""]),
  ...g("regex", ["$match($states.input.text, /q\\w+/)", "$contains($states.input.text, /fox$/)", "$replace($states.input.text, /o/, \"0\")", "$replace($states.input.text, /(\\w+) (\\w+)/, \"$2 $1\")",
    "$split($states.input.text, / /)", "$split($states.input.text, / /, 2)", "$match(\"aBc\", /b/i)", "$replace(\"abc\", /(?<x>b)/, \"[$1]\")", "$match($states.input.text, /(?:)/)"]),
  ...g("date", ["$fromMillis(0)", "$fromMillis(1700000000000)", "$fromMillis(1700000000000, \"[Y0001]-[M01]-[D01]\")", "$toMillis(\"2023-11-14T22:13:20.000Z\")", "$toMillis(\"2023-11-14\")",
    "$fromMillis(1700000000000, \"[H01]:[m01]:[s01] [P]\")", "$toMillis(\"nonsense\")", "$fromMillis(-1)", "$toMillis(\"2023-11-14T22:13:20+05:30\")"]),
  ...g("aws", ["$partition([1,2,3,4,5], 2)", "$range(1, 10, 3)", "$range(10, 1, -4)", "$hash(\"abc\", \"SHA-256\")", "$hash(\"abc\", \"MD5\")", "$parse(\"{\\\"a\\\":1}\")",
    "$base64encode($string($states.input.n))", "$states.context.Execution.Name", "$states.context.State.Name", "$partition([], 3)", "$range(1, 3)", "$hash(\"abc\", \"SHA-512\")"]),
  ...g("error", ["$states.input.s + 1", "$states.input.xs.n.x + 1", "$number(\"abc\")", "$substring(5, 1)", "$undefinedFn(1)", "1 +", "$states.input.xs[\"a\"]", "$sum(\"a\")", "$count(1, 2)",
    "$map([1], 5)", "$string(function($x){$x})", "\"a\" - 1", "$states.input.missing", "$states.input.missing + 1", "[1] + [2]", "$error(\"boom\")", "$assert(false, \"nope\")", "1 / 0 + 1", "$eval(\"1+1\")"]),
];

// Assign: the variables a state sets, then used in its Output.
cases.push(
  { id: "assign-1", group: "assign", expr: "$a + $b", input: I, assign: { a: "{% 2 %}", b: "{% $a * 5 %}" } },
  { id: "assign-2", group: "assign", expr: "$o.k", input: I, assign: { o: "{% {\"k\": [1,2]} %}" } },
  { id: "assign-3", group: "assign", expr: "$states.input.n + $c", input: I, assign: { c: "{% $states.input.n * 2 %}" } },
);

const definition = (c: Case) => {
  const d: Record<string, unknown> = { Type: "Pass", QueryLanguage: "JSONata", Output: `{% ${c.expr} %}` };
  if (c.assign) d.Assign = c.assign;
  return JSON.stringify(d);
};

const sh = async (args: string[]) => {
  const p = Bun.spawn(["/opt/homebrew/bin/aws", "stepfunctions", "test-state", "--profile", PROFILE, "--region", "us-east-1", ...args, "--output", "json"], { stdout: "pipe", stderr: "pipe" });
  const [out, err] = await Promise.all([new Response(p.stdout).text(), new Response(p.stderr).text()]);
  return { code: await p.exited, out, err };
};

async function record(c: Case) {
  for (let attempt = 0; attempt < 5; attempt++) {
    const r = await sh(["--definition", definition(c), "--input", JSON.stringify(c.input ?? {})]);
    if (r.code === 0) {
      const j = JSON.parse(r.out);
      return { ...c, definition: JSON.parse(definition(c)), expected: { status: j.status, output: j.output ?? null, error: j.error ?? null, cause: j.cause ?? null } };
    }
    if (/Throttl|Rate exceeded|TooManyRequests/i.test(r.err)) { await Bun.sleep(1500 * (attempt + 1)); continue; }
    // A definition AWS refuses before running it is also an answer.
    const m = r.err.match(/\(([A-Za-z]+)\) when calling the TestState operation: (.*)/s);
    return { ...c, definition: JSON.parse(definition(c)), expected: { status: "REFUSED", output: null, error: m?.[1] ?? "unknown", cause: (m?.[2] ?? r.err).trim().slice(0, 300) } };
  }
  throw new Error(`throttled: ${c.id}`);
}

console.log(`recorder: ${await assertRecorder()}`);
const results: unknown[] = [];
const queue = [...cases];
await Promise.all(Array.from({ length: 4 }, async () => {
  for (let c = queue.shift(); c; c = queue.shift()) results.push(await record(c));
}));
const order = new Map(cases.map((c, i) => [c.id, i]));
results.sort((a: any, b: any) => order.get(a.id)! - order.get(b.id)!);
const out = `${import.meta.dir}/../../docs/port/aws/spikes/S4-corpus.json`;
await Bun.write(out, JSON.stringify(results, null, 1) + "\n");
const by: Record<string, number> = {};
for (const r of results as any[]) by[r.expected.status] = (by[r.expected.status] ?? 0) + 1;
console.log(`${results.length} cases recorded: ${JSON.stringify(by)} -> ${out}`);
