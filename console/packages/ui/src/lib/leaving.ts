// A page being left starts no request (J500, CON-1514). Between `beforeunload` and the next document coming in, the old page
// still runs (a refresh, a tail's start, the first queries of a page that has only just loaded), but WebKit refuses every request
// it starts then and logs "Fetch API cannot load <url> due to access control checks." as an error, handled or not. So a request
// asked for in that window waits: for the page to go (it is never sent) or to stay (it is sent): `pageshow` (back from the
// back/forward cache), or STAY_MS with the page still here (a navigation that never came: a download, a 204, one cancelled).
// A request already in flight when the page is left is cancelled by the browser, quietly; only new ones are held.

/** How long after `beforeunload` a page still here is taken to be staying. */
export const STAY_MS = 3_000;

export type Leaving = {
  /** The page has started to be left (`beforeunload`). */
  leave(): void;
  /** The page is staying: held requests go. */
  stay(): void;
  /** Whether requests are being held now. */
  leaving(): boolean;
  /** Resolves when requests may go (at once when the page is not being left); rejects with an AbortError when `signal` aborts
   * first. Never settles if the page goes. */
  untilStaying(signal?: AbortSignal | null): Promise<void>;
};

export type Timers = { set(fn: () => void, ms: number): unknown; clear(t: unknown): void };

const realTimers: Timers = {
  set: (fn, ms) => setTimeout(fn, ms),
  clear: (t) => clearTimeout(t as ReturnType<typeof setTimeout>),
};

export function createLeaving(stayMs = STAY_MS, timers: Timers = realTimers): Leaving {
  let held = false;
  let timer: unknown;
  const waiters = new Set<() => void>();
  const stay = () => {
    held = false;
    if (timer !== undefined) timers.clear(timer);
    timer = undefined;
    const go = [...waiters];
    waiters.clear();
    for (const w of go) w();
  };
  return {
    leave() {
      held = true;
      if (timer !== undefined) timers.clear(timer);
      timer = timers.set(stay, stayMs);
    },
    stay,
    leaving: () => held,
    untilStaying(signal) {
      if (signal?.aborted) return Promise.reject(signal.reason ?? new DOMException("Aborted", "AbortError"));
      if (!held) return Promise.resolve();
      return new Promise<void>((resolve, reject) => {
        const go = () => {
          signal?.removeEventListener("abort", abort);
          resolve();
        };
        const abort = () => {
          waiters.delete(go);
          reject(signal?.reason ?? new DOMException("Aborted", "AbortError"));
        };
        waiters.add(go);
        signal?.addEventListener("abort", abort, { once: true });
      });
    },
  };
}

/** The page's own: every request of the console (the fetch client, both stream readers) waits on it. */
export const page = createLeaving();

if (typeof window !== "undefined") {
  // A handler that asks to stay (a prompt; the console has none) cancels the leave; STAY_MS covers one that runs after this.
  window.addEventListener("beforeunload", (e) => {
    if (!e.defaultPrevented) page.leave();
  });
  window.addEventListener("pageshow", () => page.stay());
}

/** Waits until a request may go; see `createLeaving`. */
export const untilStaying = (signal?: AbortSignal | null) => page.untilStaying(signal);

/** A code chunk's load (a route's page, a dialog's code) that waits like a request, and that the leaving cut short (Firefox:
 * "error loading dynamically imported module", WebKit: "Importing a module script failed") is not an error: the page is going,
 * so it waits, and loads again if the page stays. Without this the router's boundary catches the failure and React logs it. */
export function unlessLeaving<T>(load: () => Promise<T>, l: Leaving = page): () => Promise<T> {
  return async () => {
    for (;;) {
      await l.untilStaying();
      try {
        return await load();
      } catch (e) {
        if (!l.leaving()) throw e;
      }
    }
  };
}
