// A REST stage's settings as the edit dialogs hold them, and the patch each makes (UpdateStage takes `{op, path, value}`; a value is
// text). A patch holds only what changed, so an edit that changes nothing sends nothing.
import type { Patch } from "./api";
import type { Stage } from "./types";

/** The account-wide defaults a stage reports when nothing is set on it. */
export const DEFAULT_RATE = 10000;
export const DEFAULT_BURST = 5000;

export type Details = { description: string; cache: boolean; throttle: boolean; rate: string; burst: string };
export type Logging = { level: "OFF" | "ERROR" | "INFO"; dataTrace: boolean; metrics: boolean; access: boolean; destination: string; format: string };

const all = (stage: Stage) => stage.methodSettings?.["*/*"];

export function detailsOf(s: Stage): Details {
  const m = all(s);
  const rate = m?.throttlingRateLimit ?? DEFAULT_RATE;
  const burst = m?.throttlingBurstLimit ?? DEFAULT_BURST;
  return { description: s.description ?? "", cache: Boolean(s.cacheClusterEnabled), throttle: rate !== DEFAULT_RATE || burst !== DEFAULT_BURST, rate: String(rate), burst: String(burst) };
}
export function loggingOf(s: Stage): Logging {
  const m = all(s);
  const level = (m?.loggingLevel ?? "OFF").toUpperCase();
  return { level: level === "INFO" || level === "ERROR" ? level : "OFF", dataTrace: Boolean(m?.dataTraceEnabled), metrics: Boolean(m?.metricsEnabled), access: Boolean(s.accessLogSettings?.destinationArn), destination: s.accessLogSettings?.destinationArn ?? "", format: s.accessLogSettings?.format ?? "" };
}

const put = (out: Patch[], path: string, before: string | undefined, after: string) => {
  if (before !== after) out.push({ op: "replace", path, value: after });
};

export function detailsPatches(before: Stage, f: Details): Patch[] {
  const was = detailsOf(before);
  const out: Patch[] = [];
  put(out, "/description", was.description, f.description);
  put(out, "/cacheClusterEnabled", String(was.cache), String(f.cache));
  const rate = f.throttle ? f.rate : String(DEFAULT_RATE);
  const burst = f.throttle ? f.burst : String(DEFAULT_BURST);
  put(out, "/*/*/throttling/rateLimit", was.rate, rate);
  put(out, "/*/*/throttling/burstLimit", was.burst, burst);
  return out;
}

export function loggingPatches(before: Stage, f: Logging): Patch[] {
  const was = loggingOf(before);
  const out: Patch[] = [];
  put(out, "/*/*/logging/loglevel", was.level, f.level);
  put(out, "/*/*/logging/dataTrace", String(was.dataTrace), String(f.dataTrace));
  put(out, "/*/*/metrics/enabled", String(was.metrics), String(f.metrics));
  if (f.access) {
    put(out, "/accessLogSettings/destinationArn", was.destination, f.destination);
    put(out, "/accessLogSettings/format", was.format, f.format);
  } else if (was.access) out.push({ op: "remove", path: "/accessLogSettings" });
  return out;
}

/** Variables as rows of name and value; the patch sets each changed one (replace, new or not) and removes each that is gone. */
export function variablesPatches(before: Record<string, string> | undefined, rows: readonly { name: string; value: string }[]): Patch[] {
  const was = before ?? {};
  const now = Object.fromEntries(rows.filter((r) => r.name !== "").map((r) => [r.name, r.value]));
  const out: Patch[] = [];
  for (const [k, v] of Object.entries(now)) if (was[k] !== v) out.push({ op: "replace", path: `/variables/${k}`, value: v });
  for (const k of Object.keys(was)) if (!(k in now)) out.push({ op: "remove", path: `/variables/${k}` });
  return out;
}

export const levelWords = (l: Logging["level"]): string => ({ OFF: "Off", ERROR: "Errors only", INFO: "Errors and info logs" })[l];
