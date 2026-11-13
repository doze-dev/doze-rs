// Where the focus goes when an overlay closes: back to what opened it. ONE mechanism, used by every Dialog and Sheet (the
// shared `DialogContent` and `SheetContent` call it), whatever opened the overlay (a click, a key, an address).
//
// Why Radix alone is not enough here (the audit of 2026-11-13 found seven overlays that lost the focus):
// - Firefox and Safari do not focus a clicked button, so at open time `document.activeElement` is not the opener;
// - the page behind an open overlay is made `inert` (use-inert-page.ts), which blurs the opener before Radix records it;
// - a routed overlay opens a render after the click (the address changes first), and a refresh may draw the opener again (a
//   new node for the same button), so the node kept may be gone when the overlay closes.
// So: the last pointer press and the last focus are followed for the whole document; when an overlay mounts, the newer of the
// two is its opener, kept with a way to find the same control again (its focus key, test id or label, inside its row); when it
// closes, the focus goes there, or to the opener of the overlay it was opened from, in that order.

type Seen = { el: HTMLElement; at: number };
export type Opener = { el: HTMLElement; find: string | null; text: string; parent: Opener | null };

const INTERACTIVE = "button, a[href], input, select, textarea, summary, [role=button], [role=menuitem], [role=menuitemcheckbox], [role=menuitemradio], [role=tab], [role=option], [role=checkbox], [role=switch], [tabindex]:not([tabindex='-1'])";
const OVERLAY = "[data-slot=dialog-content], [data-slot=sheet-content]";

let pressed: Seen | null = null;
let focused: Seen | null = null;
const openers = new WeakMap<Element, Opener>();

function follow() {
  if (typeof document === "undefined" || (globalThis as { __dozeOpener?: boolean }).__dozeOpener) return;
  (globalThis as { __dozeOpener?: boolean }).__dozeOpener = true;
  document.addEventListener(
    "pointerdown",
    (e) => {
      const t = e.target instanceof Element ? e.target : null;
      const el = t ? (t.closest<HTMLElement>(INTERACTIVE) ?? t.closest<HTMLElement>("tr[data-row]")) : null;
      if (el) pressed = { el, at: performance.now() };
    },
    true,
  );
  document.addEventListener(
    "focusin",
    (e) => {
      if (e.target instanceof HTMLElement && e.target !== document.body) focused = { el: e.target, at: performance.now() };
    },
    true,
  );
}
follow();

const esc = (s: string) => (typeof CSS !== "undefined" ? CSS.escape(s) : s.replace(/["\\]/g, "\\$&"));

/** A caller that knows its opener (a row's action button) names it: it counts as the newest press. */
export function hintOpener(el: HTMLElement | null | undefined) {
  if (el) pressed = { el, at: performance.now() };
}

/** A menu item goes away with its menu: its menu's trigger stands for it. A row that is not focusable: its first control. */
function settle(el: HTMLElement): HTMLElement {
  const menu = el.closest<HTMLElement>("[role=menu]");
  if (menu) {
    const by = menu.getAttribute("aria-labelledby");
    const trigger = by ? document.getElementById(by) : null;
    if (trigger) return trigger;
  }
  if (el.matches("tr[data-row]")) return el.querySelector<HTMLElement>(INTERACTIVE) ?? el;
  return el;
}

/** A selector that finds the same control again after its node was drawn anew. */
function finder(el: HTMLElement): string | null {
  const own = el.dataset.focusKey
    ? `[data-focus-key="${esc(el.dataset.focusKey)}"]`
    : el.dataset.testid
      ? `[data-testid="${esc(el.dataset.testid)}"]`
      : el.getAttribute("aria-label")
        ? `${el.tagName.toLowerCase()}[aria-label="${esc(el.getAttribute("aria-label")!)}"]`
        : el.id && !el.id.startsWith("radix-")
          ? `#${esc(el.id)}`
          : null;
  if (!own) return null;
  const row = el.closest<HTMLElement>("[data-row]");
  return row && row !== el ? `[data-row="${esc(row.dataset.row ?? "")}"] ${own}` : own;
}

/** The opener of an overlay that is mounting now (called while it renders, before the page behind it goes inert). */
export function captureOpener(): Opener | null {
  const active = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;
  // The newer of the last press and the last focus; the focus as it is now when neither was seen (a key opened it).
  const seen = pressed && (!focused || pressed.at >= focused.at) && pressed.el.isConnected ? pressed.el : (active ?? (focused?.el.isConnected ? focused.el : null));
  if (!seen) return null;
  const el = settle(seen);
  const host = el.closest(OVERLAY);
  return { el, find: finder(el), text: (el.textContent ?? "").trim().slice(0, 80), parent: host ? (openers.get(host) ?? null) : null };
}

/** Records the overlay's content node, so an overlay opened from inside it can fall back to its opener. */
export function registerOpener(content: Element | null, o: Opener | null) {
  if (content && o) openers.set(content, o);
}

function usable(el: HTMLElement | null): boolean {
  return !!el && el.isConnected && !el.closest("[inert]") && !(el as HTMLButtonElement).disabled && el.getClientRects().length > 0;
}

/** Another modal overlay is still open (one opened from this one as it closed): the focus is its business. */
function otherOpen(except: Element | null): boolean {
  return [...document.querySelectorAll(OVERLAY)].some((n) => n !== except && n.getAttribute("data-state") === "open");
}

function again(o: Opener): HTMLElement | null {
  if (usable(o.el)) return o.el;
  if (o.find) {
    const found = document.querySelector<HTMLElement>(o.find);
    if (found && usable(found)) return found;
  }
  // Last: the only control of the same kind with the same words (a button whose page was drawn again without keys).
  if (o.text) {
    const tag = o.el.tagName.toLowerCase();
    const same = [...document.querySelectorAll<HTMLElement>(tag)].filter((n) => usable(n) && (n.textContent ?? "").trim().slice(0, 80) === o.text);
    if (same.length === 1) return same[0]!;
  }
  return null;
}

/** For an overlay's `onCloseAutoFocus`: the focus back to its opener (or the opener's own opener). True when it went there. */
export function returnFocus(e: Event, o: Opener | null, content: Element | null): boolean {
  if (otherOpen(content)) {
    e.preventDefault();
    return true;
  }
  for (let x = o; x; x = x.parent) {
    const el = again(x);
    if (el) {
      e.preventDefault();
      el.focus({ preventScroll: true });
      return true;
    }
  }
  // The opener went with what the overlay did (a removed row): the page's main region, so Tab goes on from the page and not
  // from the top of the document. Opened by an address (nothing opened it), Radix's own rule stands.
  const main = document.getElementById("main");
  if (o && main) {
    e.preventDefault();
    main.focus({ preventScroll: true });
    return true;
  }
  return false;
}
