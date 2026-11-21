// What the page just left wants the next one to say (`Bucket larder-exports created.`): kept until the next page takes it, once.
// Not state of a page and not in the address: a result is said where the person lands, and gone after they have read it.
export type Flash = { tone?: "good" | "warn" | "bad" | "info"; title: string; returned?: string; left?: string };

let pending: Flash | null = null;

export function setFlash(f: Flash): void {
  pending = f;
}

/** The flash, and forget it. */
export function takeFlash(): Flash | null {
  const f = pending;
  pending = null;
  return f;
}

/** What emptying a bucket did, kept for its Status page (by bucket): objects removed, their size, and what could not be. */
export type Emptied = { deleted: number; bytes: number; failed: { key: string; code: string; message?: string }[] };
const emptied = new Map<string, Emptied>();
export const setEmptied = (bucket: string, e: Emptied): void => void emptied.set(bucket, e);
export const emptiedOf = (bucket: string): Emptied | undefined => emptied.get(bucket);
