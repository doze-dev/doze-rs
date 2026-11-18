// Whether the sidebar is open: the person's own choice when they made one (remembered per browser), else by the window's
// width: icons only below 1280 px, where the page needs the room (the audit's 1024 layout), full from 1280.
import { useState, useSyncExternalStore } from "react";

export const SIDEBAR_KEY = "doze:sidebar";
const WIDE = "(min-width: 1280px)";

function readChoice(): boolean | null {
  try {
    const v = globalThis.localStorage?.getItem(SIDEBAR_KEY);
    return v === "open" ? true : v === "closed" ? false : null;
  } catch {
    return null;
  }
}

function useWide(): boolean {
  return useSyncExternalStore(
    (on) => {
      const m = window.matchMedia(WIDE);
      m.addEventListener("change", on);
      return () => m.removeEventListener("change", on);
    },
    () => window.matchMedia(WIDE).matches,
    () => true,
  );
}

/** `[open, setOpen]` for the SidebarProvider: setting it is the person's choice, and is kept. */
export function useSidebarOpen(): [boolean, (open: boolean) => void] {
  const wide = useWide();
  const [choice, setChoice] = useState<boolean | null>(readChoice);
  const set = (open: boolean) => {
    setChoice(open);
    try {
      globalThis.localStorage?.setItem(SIDEBAR_KEY, open ? "open" : "closed");
    } catch {
      // private mode: the choice lasts as long as the page
    }
  };
  return [choice ?? wide, set];
}
