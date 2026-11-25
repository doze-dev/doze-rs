// A rail that scrolls shows it: the edge that has more past it fades, so a row cut by the edge reads as "more below", not as a
// row somebody cut. The hook writes two CSS variables on the scroller (`--fade-top`, `--fade-bottom`, px) that its mask reads.
import { useEffect, useRef } from "react";

/** How far (px) the fade reaches into the scroller. */
export const FADE = 28;

/** How much to fade at each edge of a box scrolled to `scrollTop`: the distance left to scroll past that edge, at most {@link FADE}
 * (so a box at the end fades nothing, and a box that does not scroll fades nothing at all). */
export function fadeOf(box: { scrollTop: number; scrollHeight: number; clientHeight: number }, size = FADE): { top: number; bottom: number } {
  const left = box.scrollHeight - box.clientHeight;
  if (left <= 1) return { top: 0, bottom: 0 };
  const top = Math.max(0, Math.min(size, box.scrollTop));
  const bottom = Math.max(0, Math.min(size, left - box.scrollTop));
  return { top: Math.round(top), bottom: Math.round(bottom) };
}

/** The ref of a scroller to fade at its edges that have more past them: kept current as it scrolls, resizes and its content changes. */
export function useScrollFade<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const paint = () => {
      const f = fadeOf(el);
      el.style.setProperty("--fade-top", `${f.top}px`);
      el.style.setProperty("--fade-bottom", `${f.bottom}px`);
    };
    paint();
    el.addEventListener("scroll", paint, { passive: true });
    const sized = new ResizeObserver(paint);
    sized.observe(el);
    const changed = new MutationObserver(paint);
    changed.observe(el, { childList: true, subtree: true });
    return () => {
      el.removeEventListener("scroll", paint);
      sized.disconnect();
      changed.disconnect();
    };
  }, []);
  return ref;
}
