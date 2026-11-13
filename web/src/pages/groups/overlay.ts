// A routed overlay (a Dialog or a Sheet with its own address, live-design.md): opening it pushes an entry, so Back closes
// it; opened by a deep link there is nothing of ours to go back to, so closing replaces the address instead. The focus goes
// back to what opened it (Firefox and Safari do not focus a clicked button, so Radix alone would lose it).
import { useRouter } from "@tanstack/react-router";
import { useMemo, useRef } from "react";

export function useRoutedOverlay(param: string) {
  const router = useRouter();
  const pushed = useRef(false);
  const opener = useRef<HTMLElement | null>(null);
  // Stable functions: a table's cells that call them are not drawn again on every render.
  return useMemo(() => {
  const go = (value: unknown, replace: boolean) =>
    void router.navigate({ to: ".", search: ((s: Record<string, unknown>) => ({ ...s, [param]: value })) as never, replace, resetScroll: false });
  return {
    open: (value: unknown, from?: HTMLElement | null) => {
      pushed.current = true;
      opener.current = from ?? (document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null);
      go(value, false);
    },
    close: () => {
      if (pushed.current) {
        pushed.current = false;
        router.history.back();
      } else go(undefined, true);
    },
    /** Replace the overlay's value (another member, another generation) without a new entry. */
    swap: (value: unknown) => go(value, true),
    /** For the overlay's `onCloseAutoFocus`: back to the opener when it is still on the page. */
    returnFocus: (e: Event) => {
      let el = opener.current;
      opener.current = null;
      // A table may have drawn its row again while the overlay was open: the same control, found by its key.
      const key = el?.dataset.focusKey;
      if (el && !el.isConnected && key) el = document.querySelector<HTMLElement>(`[data-focus-key="${CSS.escape(key)}"]`);
      if (el?.isConnected) {
        e.preventDefault();
        el.focus();
      }
    },
  };
  }, [router, param]);
}
