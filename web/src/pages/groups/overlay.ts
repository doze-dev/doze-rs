// A routed overlay (a Dialog or a Sheet with its own address, live-design.md): opening it pushes an entry, so Back closes
// it; opened by a deep link there is nothing of ours to go back to, so closing replaces the address instead.
import { useRouter } from "@tanstack/react-router";
import { useRef } from "react";

export function useRoutedOverlay(param: string) {
  const router = useRouter();
  const pushed = useRef(false);
  const go = (value: unknown, replace: boolean) =>
    void router.navigate({ to: ".", search: ((s: Record<string, unknown>) => ({ ...s, [param]: value })) as never, replace, resetScroll: false });
  return {
    open: (value: unknown) => {
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
}
