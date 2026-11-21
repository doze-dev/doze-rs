// One address parameter as a list's state (a filter's chip, a tab): read from the address, written back with `replace`, so the
// Back button is not made to walk through every click. The pure half; hooks/use-url-param.ts is the hook.

/** `search` with `key` set to `value`; a value of `undefined`, `null` or `""` removes the key. The other keys are kept. */
export function withParam(search: Record<string, unknown>, key: string, value: string | null | undefined): Record<string, unknown> {
  const next = { ...search };
  if (value === undefined || value === null || value === "") delete next[key];
  else next[key] = value;
  return next;
}

/** A parameter's value as text (the address reads `12` as a number and `true` as a boolean); `undefined` when absent. */
export function paramText(raw: unknown): string | undefined {
  if (typeof raw === "string") return raw;
  if (typeof raw === "number" || typeof raw === "boolean") return String(raw);
  return undefined;
}
