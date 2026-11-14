// One path segment of an API address, made from data (a topic, a group, a transactional id, a rule id). Every character that
// is not unreserved is `%XX`, as the server's `base::segment` writes it. A name made of one or two dots is written `!.` or
// `!..`: a URL parser (the browser's, before fetch sends anything) takes `.`, `..` and their encodings `%2E` and `%2E%2E` as
// directory steps and removes them, so a group named `.` would turn `/api/v1/groups/./offsets/delete` into
// `/api/v1/groups/offsets/delete`. The server turns `!.` back into the name before it routes (`app::undot`); `!` is
// percent-encoded in every other name, so the mark is never ambiguous.

/** A path segment from data, safe to put between two slashes. */
export function segment(s: string | number): string {
  const out = encodeURIComponent(String(s)).replace(/[!'()*]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);
  return out === "." || out === ".." ? `!${out}` : out;
}
