// What a scan's filter said, in the board's words (`4 have status shipped`) when it is one attribute compared with `=` to one value;
// anything else is "match the filter". Pure.

type Typed = Record<string, unknown>;

const read = (text: string): Typed => {
  try {
    const v: unknown = JSON.parse(text);
    return typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Typed) : {};
  } catch {
    return {};
  }
};

/** `status shipped` for `#s = :s` with `{"#s": "status"}` and `{":s": {"S": "shipped"}}`; null when the filter is anything else. */
export function equalityWords(filter: string, names: string, values: string): string | null {
  const m = /^\s*(#?[A-Za-z_][\w]*)\s*=\s*(:[A-Za-z_]\w*)\s*$/.exec(filter);
  if (!m) return null;
  const attribute = m[1]!.startsWith("#") ? read(names)[m[1]!] : m[1];
  const typed = read(values)[m[2]!];
  if (typeof attribute !== "string" || typeof typed !== "object" || typed === null) return null;
  const [one, ...more] = Object.entries(typed as Typed);
  if (!one || more.length > 0 || (one[0] !== "S" && one[0] !== "N") || typeof one[1] !== "string") return null;
  return `${attribute} ${one[1]}`;
}

/** `4 have status shipped`, `1 has status shipped`, or `4 match the filter`. */
export function matchedWords(count: number, filter: string, names: string, values: string): string {
  const equal = equalityWords(filter, names, values);
  if (equal) return `${count} ${count === 1 ? "has" : "have"} ${equal}`;
  return `${count} ${count === 1 ? "matches" : "match"} the filter`;
}
