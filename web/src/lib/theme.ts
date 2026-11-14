import { useSyncExternalStore } from "react";

/** The person's choice; "system" follows the OS (and changes with it). Kept in this browser only. */
export type ThemeChoice = "system" | "light" | "dark";
export type Theme = "light" | "dark";

const KEY = "doze-kafka.theme";
const media = () => window.matchMedia("(prefers-color-scheme: dark)");
const listeners = new Set<() => void>();

function read(): ThemeChoice {
  try {
    const v = localStorage.getItem(KEY);
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
  }
}

let choice: ThemeChoice = typeof window === "undefined" ? "system" : read();

export function resolve(c: ThemeChoice): Theme {
  return c === "system" ? (media().matches ? "dark" : "light") : c;
}

/** Puts the theme on <html>: `dark` when dark, and `light` when the person chose light over a dark OS (the CSS media query
 * decides when there is no class, so the first paint is right before this runs). */
export function applyTheme() {
  const el = document.documentElement;
  const theme = resolve(choice);
  el.classList.toggle("dark", theme === "dark");
  el.classList.toggle("light", theme === "light");
}

export function setTheme(c: ThemeChoice) {
  choice = c;
  try {
    if (c === "system") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, c);
  } catch {
    /* private mode: the choice lasts this page only */
  }
  applyTheme();
  listeners.forEach((l) => l());
}

/** Starts the theme: applies it, and follows the OS while the choice is "system". Call once, before the first render. */
export function startTheme() {
  applyTheme();
  media().addEventListener("change", () => {
    if (choice !== "system") return;
    applyTheme();
    listeners.forEach((l) => l());
  });
  // Another tab changed it.
  window.addEventListener("storage", (e) => {
    if (e.key !== KEY) return;
    choice = read();
    applyTheme();
    listeners.forEach((l) => l());
  });
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}

/** The choice and the theme it resolves to, kept current. */
export function useTheme(): { choice: ThemeChoice; theme: Theme } {
  const c = useSyncExternalStore(subscribe, () => choice, () => "system" as ThemeChoice);
  const theme = useSyncExternalStore(subscribe, () => resolve(choice), () => "light" as Theme);
  return { choice: c, theme };
}
