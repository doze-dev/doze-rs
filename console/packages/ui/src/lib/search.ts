// Typed search params without a schema library in the main bundle: each parser keeps a value only when it is one of the
// allowed ones, so a hand-edited address can never put an unknown state on screen. An app builds its pages' parsers from these.

export function oneOf<const T extends readonly string[]>(allowed: T, raw: unknown): T[number] | undefined {
  return typeof raw === "string" && (allowed as readonly string[]).includes(raw) ? (raw as T[number]) : undefined;
}

export function shortText(raw: unknown, most = 64): string | undefined {
  return typeof raw === "string" && raw.length > 0 && raw.length <= most ? raw : undefined;
}
