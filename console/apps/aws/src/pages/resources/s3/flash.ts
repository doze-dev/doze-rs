// What the page just left wants the next one to say (`Bucket larder-exports created.`): kept a few seconds, until it is dismissed.
// Not state of a page and not in the address: a result is said where the person lands, and gone after they have read it.
export type Flash = { tone?: "good" | "warn" | "bad" | "info"; title: string; returned?: string; left?: string };

let pending: Flash | null = null;
let at = 0;
/** How long a result stays for the page that is drawn next (a page drawn twice, as a lazy page is, still says it). */
const FRESH = 8000;

export function setFlash(f: Flash): void {
  pending = f;
  at = Date.now();
}

/** The flash, while it is fresh and not dismissed. */
export function takeFlash(): Flash | null {
  return pending && Date.now() - at < FRESH ? pending : null;
}

export function dismissFlash(): void {
  pending = null;
}

/** What emptying a bucket did, kept for its Status page (by bucket): objects removed, their size, and what could not be. */
export type Emptied = { deleted: number; bytes: number; failed: { key: string; code: string; message?: string }[] };
const emptied = new Map<string, Emptied>();
export const setEmptied = (bucket: string, e: Emptied): void => void emptied.set(bucket, e);
export const emptiedOf = (bucket: string): Emptied | undefined => emptied.get(bucket);
