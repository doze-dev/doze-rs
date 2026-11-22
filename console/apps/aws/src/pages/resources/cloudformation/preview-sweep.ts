// The preview change sets the update wizard makes (`console-preview-<ms>`) and what becomes of one a page did not get to remove: a tab closed or
// reloaded on the review step runs no code, so the set stays on the stack and shows in ListChangeSets. The next page of the stack removes it.
//
// A preview is only left alone while somebody is using it: the review step of a wizard open in a tab beats a heartbeat into the browser's
// local storage (every tab of the console sees it), and the one a tab keeps while its change set is read on a page of its own is known to
// that tab (`kept`). Any other `console-preview-*` set is nobody's.
export const PREVIEW = /^console-preview-\d+$/;
const KEY = "doze-aws:cfn-previews";
/** How often an open review step says it is there. */
export const BEAT_MS = 10_000;
/** How long after its last beat a review step is taken to be gone. */
export const LIVE_MS = 30_000;

type Beats = Record<string, number>;
type Store = Pick<Storage, "getItem" | "setItem">;

const store = (): Store | null => (typeof localStorage === "undefined" ? null : localStorage);

export function readBeats(s: Store | null = store()): Beats {
  try {
    const v: unknown = JSON.parse(s?.getItem(KEY) ?? "{}");
    return v !== null && typeof v === "object" && !Array.isArray(v) ? (v as Beats) : {};
  } catch {
    return {};
  }
}

/** Says the review step of `id` is open now (and forgets beats too old to matter). */
export function beat(id: string, now: number, s: Store | null = store()): void {
  const kept = Object.fromEntries(Object.entries(readBeats(s)).filter(([, t]) => now - t < LIVE_MS * 4));
  try {
    s?.setItem(KEY, JSON.stringify({ ...kept, [id]: now }));
  } catch {
    // storage refused (a private window): the sweep then keeps only the preview a tab holds itself
  }
}

/** Says the review step of `id` is closed. */
export function unbeat(id: string, s: Store | null = store()): void {
  const rest = { ...readBeats(s) };
  delete rest[id];
  try {
    s?.setItem(KEY, JSON.stringify(rest));
  } catch {
    // as above
  }
}

/** Of the change sets of a stack, the previews nobody is using: named as the wizard names them, not beating within `LIVE_MS`, not the one `kept` holds. */
export function stalePreviews(names: readonly string[], now: number, kept: string | null, beats: Beats = readBeats()): string[] {
  return names.filter((n) => PREVIEW.test(n) && n !== kept && now - (beats[n] ?? 0) > LIVE_MS);
}
