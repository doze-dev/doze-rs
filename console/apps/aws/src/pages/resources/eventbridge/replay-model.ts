// The start-replay form: the rules of its fields and StartReplay's input (times are epoch seconds, as the JSON protocol writes a timestamp).

export type ReplayForm = { name: string; description: string; archive: string | null; all: boolean; rules: string[]; startMs: number | null; endMs: number | null };
export const emptyReplay = (archive: string | null, now = Date.now()): ReplayForm => ({ name: "", description: "", archive, all: true, rules: [], startMs: Math.floor(now / 1000) * 1000 - 3600_000, endMs: Math.floor(now / 1000) * 1000 });

/** A replay's name: 1 to 64 letters, digits, dot, hyphen and underscore. */
export function nameProblem(name: string): string | null {
  if (name === "") return "Name the replay.";
  if (name.length > 64) return "A replay name is at most 64 characters.";
  return /^[.\-_A-Za-z0-9]+$/.test(name) ? null : "A replay name takes letters, digits, dot, hyphen and underscore only.";
}

export function windowProblem(startMs: number | null, endMs: number | null): string | null {
  if (startMs === null || endMs === null) return "Set the start and the end of the window.";
  return endMs <= startMs ? "The end must be after the start." : null;
}

export function replayProblem(f: ReplayForm, taken: readonly string[]): string | null {
  return nameProblem(f.name) ?? (taken.includes(f.name) ? `${f.name} already ran.` : null) ?? (f.archive ? null : "Choose the archive to replay.") ?? windowProblem(f.startMs, f.endMs) ?? (!f.all && f.rules.length === 0 ? "Pick at least one rule, or replay to all rules." : null);
}

/** A moment as the CLI and the SDKs write it: `2026-11-07T00:00:00Z`. */
export const isoTime = (ms: number): string => new Date(ms).toISOString().replace(/\.\d{3}Z$/, "Z");

/** StartReplay's input: the archive, the window, and the bus it came from as the destination (with the rules picked, when not all).
 * `shown`: the call as a person writes it (the times as ISO 8601 text, as the CLI and boto3 take them), not as it goes on the wire. */
export function startReplayInput(f: ReplayForm, archiveArn: string, busArn: string, ruleArn: (rule: string) => string, shown = false): Record<string, unknown> {
  const time = (ms: number | null) => (shown ? (ms === null ? undefined : isoTime(ms)) : (ms ?? 0) / 1000);
  return {
    ReplayName: f.name,
    ...(f.description ? { Description: f.description } : {}),
    EventSourceArn: archiveArn,
    EventStartTime: time(f.startMs),
    EventEndTime: time(f.endMs),
    Destination: { Arn: busArn, ...(!f.all && f.rules.length > 0 ? { FilterArns: f.rules.map(ruleArn) } : {}) },
  };
}
