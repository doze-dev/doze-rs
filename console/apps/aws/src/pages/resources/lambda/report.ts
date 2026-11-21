// The REPORT line a run's log ends with (`REPORT RequestId: …\tDuration: 1.23 ms\t…`), read into its fields, as the Test
// tab's Details shows them. Pure, so it is tested without a browser.

/** The fields of the last REPORT line in `log`, in the order the line gives them; none when there is no such line. */
export function reportOf(log: string): [string, string][] {
  const line = log
    .split("\n")
    .reverse()
    .find((l) => l.startsWith("REPORT "));
  if (!line) return [];
  return line
    .slice("REPORT ".length)
    .split("\t")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => {
      const at = part.indexOf(":");
      return at < 0 ? [part, ""] : [part.slice(0, at).trim(), part.slice(at + 1).trim()];
    })
    .map(([k, v]) => [k === "RequestId" ? "Request ID" : k, v] as [string, string]);
}
