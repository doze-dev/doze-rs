// A routed overlay (a Dialog or a Sheet with its own address, live-design.md): opening it pushes an entry, so Back closes
// it; opened by a deep link there is nothing of ours to go back to, so closing replaces the address instead. The focus goes
// back to what opened it by the shared mechanism of every Dialog and Sheet (lib/opener.ts).
import { useRouter } from "@tanstack/react-router";
import { useMemo, useRef } from "react";
import { hintOpener } from "@/lib/opener";

export function useRoutedOverlay(param: string) {
  const router = useRouter();
  const pushed = useRef(false);
  // Stable functions: a table's cells that call them are not drawn again on every render.
  return useMemo(() => {
  const go = (value: unknown, replace: boolean) =>
    void router.navigate({ to: ".", search: ((s: Record<string, unknown>) => ({ ...s, [param]: value })) as never, replace, resetScroll: false });
  return {
    open: (value: unknown, from?: HTMLElement | null) => {
      hintOpener(from);
      pushed.current = true;
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
  };
  }, [router, param]);
}
