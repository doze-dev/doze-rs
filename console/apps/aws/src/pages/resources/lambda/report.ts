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

/** A trace without the long front of its file paths: doze-aws unpacks a function's code under its own data directory
 * (`<data>/lambda/code/<id>/root/lambda_function.py`), which AWS's traces never show (`/var/task/…`). The start up to and including
 * `/code/<id>/root/` is left out (`…/lambda_function.py`); a path that is not in a code directory is kept. */
export function tidyPaths(text: string): string {
  return text.replace(/\/[^"\\\s]*?\/code\/[\w-]+\/root\//g, "…/");
}

/** The key of the event a Python handler read that the event does not have: a `KeyError` whose key the trace shows as `event["Records"]`; null for any other error. */
export function missingEventKey(payload: string): string | null {
  try {
    const p = JSON.parse(payload) as { errorType?: unknown; errorMessage?: unknown; stackTrace?: unknown };
    if (p.errorType !== "KeyError" || typeof p.errorMessage !== "string") return null;
    const key = /^['"](.+)['"]$/.exec(p.errorMessage)?.[1];
    const trace = Array.isArray(p.stackTrace) ? p.stackTrace.join("\n") : "";
    return key && (trace.includes(`event["${key}"]`) || trace.includes(`event['${key}']`)) ? key : null;
  } catch {
    return null;
  }
}
