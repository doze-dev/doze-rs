import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { captureOpener, type Opener, registerOpener, returnFocus } from "@/lib/opener";
import { useInertPage } from "./use-inert-page";

const OVERLAY = "[data-slot=dialog-content], [data-slot=sheet-content]";

/** Mounted inside an overlay's content (which exists only while it is open): learns the opener as the overlay first renders
 * (before the page goes inert), then makes the page behind it inert. Renders a hidden mark only. */
function OverlayMount({ onReady }: { onReady: (o: Opener | null, content: Element | null) => void }) {
  const [o] = useState(captureOpener);
  const mark = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const c = mark.current?.closest(OVERLAY) ?? null;
    registerOpener(c, o);
    onReady(o, c);
  }, [o, onReady]);
  useInertPage();
  return <span ref={mark} hidden data-slot="overlay-mark" />;
}

/** The shared focus return of every Dialog and Sheet (lib/opener.ts): `mount` goes inside the content, `onCloseAutoFocus` on
 * it. A caller's own `onCloseAutoFocus` runs first; when it prevents the default, it has placed the focus itself. */
export function useOverlayOpener(own?: (e: Event) => void) {
  const openerRef = useRef<Opener | null>(null);
  const contentRef = useRef<Element | null>(null);
  const onReady = useCallback((o: Opener | null, c: Element | null) => {
    openerRef.current = o;
    contentRef.current = c;
  }, []);
  return {
    mount: <OverlayMount onReady={onReady} />,
    onCloseAutoFocus: (e: Event) => {
      own?.(e);
      if (e.defaultPrevented) return;
      returnFocus(e, openerRef.current, contentRef.current);
    },
  };
}
