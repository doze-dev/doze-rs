// An input transformer worked out in the browser, for "Generate output": the same rules doze-aws applies when it delivers an event
// (crates/aws-eventbridge/src/input.rs and jsonpath.rs): each `<name>` in the template is replaced by the value its path finds in the
// event. Inside a JSON string a text value goes in as it is and any other value as JSON; outside a string, in a template that is JSON,
// text is quoted and other values are JSON; in a template that is plain text every text value goes in as it is. A path that finds
// nothing is an empty text inside a string and `null` outside one. Pure.

type Step = { field: string } | { index: number };

function steps(path: string): Step[] | null {
  if (!path.startsWith("$")) return null;
  let rest = path.slice(1);
  const out: Step[] = [];
  while (rest !== "") {
    if (rest.startsWith(".")) {
      const after = rest.slice(1);
      const end = after.search(/[.[]/);
      const field = end === -1 ? after : after.slice(0, end);
      if (field === "" || field.includes("*")) return null;
      out.push({ field });
      rest = end === -1 ? "" : after.slice(end);
    } else if (rest.startsWith("[")) {
      const close = rest.indexOf("]");
      if (close === -1) return null;
      const inside = rest.slice(1, close);
      const quoted = /^(['"])(.*)\1$/.exec(inside);
      if (quoted) out.push({ field: quoted[2]! });
      else if (/^\d+$/.test(inside)) out.push({ index: Number(inside) });
      else return null;
      rest = rest.slice(close + 1);
    } else return null;
  }
  return out;
}

/** Whether `path` is one EventBridge accepts in `InputPath` and `InputPathsMap`: `$`, fields by dot or bracket, array positions. */
export const validPath = (path: string): boolean => steps(path) !== null;

/** The value at `path` in `doc`, or `undefined` when there is none. */
export function select(doc: unknown, path: string): unknown {
  const s = steps(path);
  if (!s) return undefined;
  let at: unknown = doc;
  for (const step of s) {
    if (at === null || typeof at !== "object") return undefined;
    at = "field" in step ? (at as Record<string, unknown>)[step.field] : (at as unknown[])[step.index];
    if (at === undefined) return undefined;
  }
  return at;
}

const isName = (s: string) => /^[A-Za-z0-9_.-]+$/.test(s);

/** The names a template uses, in order, each once. */
export function placeholders(template: string): string[] {
  const out: string[] = [];
  for (const m of template.matchAll(/<([^<>]*)>/g)) if (isName(m[1]!) && !out.includes(m[1]!)) out.push(m[1]!);
  return out;
}

const fill = (value: unknown, raw: boolean): string => (typeof value === "string" && raw ? value : value === undefined ? (raw ? "" : "null") : JSON.stringify(value));

/** The text a target receives for `event` (a parsed event), or the reason the template cannot be filled. */
export function transform(event: unknown, paths: Readonly<Record<string, string>>, template: string, names: { ruleName?: string; ruleArn?: string } = {}): { output: string } | { problem: string } {
  const bad = Object.entries(paths).find(([, p]) => !validPath(p));
  if (bad) return { problem: `The path for ${bad[0]} is not valid: ${bad[1]}` };
  const unknown = placeholders(template).find((n) => !(n in paths) && !n.startsWith("aws.events."));
  if (unknown) return { problem: `The template names <${unknown}>, which no input path defines.` };
  const valueOf = (name: string): unknown => {
    if (name === "aws.events.rule-name") return names.ruleName ?? "";
    if (name === "aws.events.rule-arn") return names.ruleArn ?? "";
    if (name === "aws.events.event.json") return event;
    if (name === "aws.events.event") return JSON.stringify(event);
    if (name.startsWith("aws.events.")) return undefined;
    return select(event, paths[name]!);
  };
  const json = /^\s*[{[]/.test(template);
  let out = "";
  let inString = false;
  for (let i = 0; i < template.length; i++) {
    const c = template[i]!;
    if (c === "\\" && inString) {
      out += c + (template[i + 1] ?? "");
      i++;
    } else if (c === '"') {
      inString = !inString;
      out += c;
    } else if (c === "<") {
      const end = template.indexOf(">", i);
      const name = end === -1 ? "" : template.slice(i + 1, end);
      if (isName(name)) {
        out += fill(valueOf(name), inString || !json);
        i = end;
      } else out += c;
    } else out += c;
  }
  if (json) {
    try {
      JSON.parse(out);
    } catch (e) {
      return { problem: `The output is not JSON: ${(e as Error).message}` };
    }
  }
  return { output: out };
}
