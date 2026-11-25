// The version Home's subtitle says. A build that was not given a release number carries the workspace's placeholder, `0.0.0`: it
// says nothing and reads as unfinished, so it is left out.

/** The version to show beside `doze-aws`, or `null` for none or a placeholder (`0.0.0`, with or without a suffix). */
export function shownVersion(version: string | null | undefined): string | null {
  const v = (version ?? "").trim();
  return v === "" || /^0\.0\.0(?:[-+].*)?$/.test(v) ? null : v;
}
