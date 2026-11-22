// A JSON document written out for reading: objects one member a line, but a list of plain values (`["GET", "HEAD"]`) on one line,
// as the boards draw a CORS rule and a policy. `JSON.stringify(v, null, 2)` puts every list item on a line of its own.
const scalar = (v: unknown): boolean => v === null || typeof v !== "object";

export function readableJson(value: unknown, indent = ""): string {
  if (Array.isArray(value)) {
    if (value.every(scalar)) return `[${value.map((v) => JSON.stringify(v)).join(", ")}]`;
    const inner = `${indent}  `;
    return `[\n${value.map((v) => `${inner}${readableJson(v, inner)}`).join(",\n")}\n${indent}]`;
  }
  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>);
    if (entries.length === 0) return "{}";
    const inner = `${indent}  `;
    return `{\n${entries.map(([k, v]) => `${inner}${JSON.stringify(k)}: ${readableJson(v, inner)}`).join(",\n")}\n${indent}}`;
  }
  return JSON.stringify(value) ?? "null";
}

/** A text that is JSON, read as the document it holds and written out for reading; any other text as it is. */
export function readableText(text: string): string {
  try {
    return readableJson(JSON.parse(text));
  } catch {
    return text;
  }
}
