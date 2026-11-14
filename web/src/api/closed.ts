// The door to other machines (SEC-250): a console served beyond loopback that only an open broker let start without
// --console-unprotected stops answering other machines once the broker has an ACL. Every API answer from then on is 403
// `CONSOLE_CLOSED_TO_NETWORK`; the first one seen here replaces the whole app with the page that says why (closed-to-network.tsx).
import { useSyncExternalStore } from "react";

/** The code the server refuses with. */
export const CLOSED_TO_NETWORK = "CONSOLE_CLOSED_TO_NETWORK";

let said: string | null = null;
const listeners = new Set<() => void>();

/** Marks the console closed to this machine, with the server's words. Called by the fetch client and the stream reader. */
export function markClosed(message: string) {
  if (said !== null) return;
  said = message;
  for (const l of listeners) l();
}

/** Whether a refusal body is the door's. */
export function isClosed(body: unknown): body is { code: string; message: string } {
  return typeof body === "object" && body !== null && (body as { code?: unknown }).code === CLOSED_TO_NETWORK && typeof (body as { message?: unknown }).message === "string";
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}

/** The server's words once the console is closed to this machine; null while it answers. */
export function useClosedToNetwork(): string | null {
  return useSyncExternalStore(
    subscribe,
    () => said,
    () => null,
  );
}
