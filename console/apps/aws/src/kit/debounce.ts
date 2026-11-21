// Debouncing, pure: the preview is asked for once a person stops typing, not on every key.
import { useEffect, useState } from "react";

/** A function that runs `ms` after the last call, with the last call's argument. `flush` runs it now; `cancel` drops it. */
export function debounce<A>(fn: (arg: A) => void, ms: number): { call: (arg: A) => void; flush: () => void; cancel: () => void; pending: () => boolean } {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let last: { arg: A } | undefined;
  const run = () => {
    timer = undefined;
    if (last) {
      const { arg } = last;
      last = undefined;
      fn(arg);
    }
  };
  return {
    call(arg) {
      last = { arg };
      clearTimeout(timer);
      timer = setTimeout(run, ms);
    },
    flush() {
      clearTimeout(timer);
      run();
    },
    cancel() {
      clearTimeout(timer);
      timer = undefined;
      last = undefined;
    },
    pending: () => timer !== undefined,
  };
}

/** `value` as it was `ms` ago when it has stopped changing: the first value at once (nothing to wait for), then each new value
 * after a pause. `settled` is false while a newer value is waiting. */
export function useDebounced<T>(value: T, ms: number, same: (a: T, b: T) => boolean = Object.is): { value: T; settled: boolean } {
  const [shown, setShown] = useState(value);
  const settled = same(shown, value);
  useEffect(() => {
    if (same(shown, value)) return;
    const id = setTimeout(() => setShown(value), ms);
    return () => clearTimeout(id);
  }, [value, shown, ms, same]);
  return { value: shown, settled };
}
