// Rows of key and value (tags, SQS message attributes, environment variables): their checks and conversions, no React.

export type KeyValueRow = { /** Stable, for React's key; not sent anywhere. */ id: string; key: string; value: string };
export type RowError = { key?: string; value?: string; /** The problem is in what was typed (a repeated key, a key too long), so it is said at once; the others wait until the person has left the field. */ live?: boolean };
export type KeyValueRules = {
  /** Keys are case-sensitive (AWS tags are): `Env` and `env` are two keys. Default true. */
  caseSensitive?: boolean;
  /** The longest a key may be (a tag key: 128). */
  maxKey?: number;
  /** The longest a value may be (a tag value: 256). */
  maxValue?: number;
  /** The most rows (50 tags). */
  maxRows?: number;
  /** An empty value is allowed (a tag may have one). Default true. */
  emptyValue?: boolean;
};

let counter = 0;
/** A new row with an id no other row has. */
export const newRow = (key = "", value = ""): KeyValueRow => ({ id: `kv${++counter}`, key, value });
export const rowsFrom = (pairs: readonly { key: string; value: string }[]): KeyValueRow[] => pairs.map((p) => newRow(p.key, p.value));
export const pairsOf = (rows: readonly KeyValueRow[]): { key: string; value: string }[] => rows.map((r) => ({ key: r.key, value: r.value }));

/** The problems of each row, by row id: an empty key, a key another row has, a key or value over its limit, an empty value
 * where one is not allowed. A row's words say what is wrong with it, not how many rows are wrong. */
export function validateRows(rows: readonly KeyValueRow[], rules: KeyValueRules = {}): Map<string, RowError> {
  const out = new Map<string, RowError>();
  const fold = (k: string) => (rules.caseSensitive === false ? k.toLowerCase() : k);
  const seen = new Map<string, number>();
  for (const r of rows) if (r.key !== "") seen.set(fold(r.key), (seen.get(fold(r.key)) ?? 0) + 1);
  for (const r of rows) {
    // A blank row says nothing and is dropped when sending: it is not wrong, only unused.
    if (r.key === "" && r.value === "") continue;
    const e: RowError = {};
    if (r.key === "") e.key = "Enter a key.";
    else if ((seen.get(fold(r.key)) ?? 0) > 1) {
      e.key = "Another row uses this key.";
      e.live = true;
    } else if (rules.maxKey !== undefined && r.key.length > rules.maxKey) {
      e.key = `A key is at most ${rules.maxKey} characters.`;
      e.live = true;
    }
    if (rules.emptyValue === false && r.value === "") e.value = "Enter a value.";
    else if (rules.maxValue !== undefined && r.value.length > rules.maxValue) {
      e.value = `A value is at most ${rules.maxValue} characters.`;
      e.live = true;
    }
    if (e.key || e.value) out.set(r.id, e);
  }
  return out;
}

/** Whether the rows can be sent: no row has a problem and there are no more rows than allowed. */
export const rowsValid = (rows: readonly KeyValueRow[], rules: KeyValueRules = {}): boolean => validateRows(rows, rules).size === 0 && (rules.maxRows === undefined || rows.length <= rules.maxRows);

/** The rows that carry something (a row left blank is dropped when sending). */
export const filled = (rows: readonly KeyValueRow[]): KeyValueRow[] => rows.filter((r) => r.key !== "" || r.value !== "");
