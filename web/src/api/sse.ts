// Server-sent events over fetch: EventSource cannot send the `X-Console: 1` proof the API asks for, so the stream is read
// from a fetch body. One connection; the caller reconnects (with `Last-Event-ID`) on its own schedule.
import { basePath } from "./client";

export type SseEvent = { event: string; id?: string; data: string };

/** Splits a buffer into complete events; returns them and what is left over. */
export function parseEvents(buffer: string): { events: SseEvent[]; rest: string } {
  const events: SseEvent[] = [];
  const text = buffer.replace(/\r\n?/g, "\n");
  const blocks = text.split("\n\n");
  const rest = blocks.pop() ?? "";
  for (const block of blocks) {
    let event = "message";
    let id: string | undefined;
    const data: string[] = [];
    for (const line of block.split("\n")) {
      if (!line || line.startsWith(":")) continue;
      const i = line.indexOf(":");
      const field = i < 0 ? line : line.slice(0, i);
      const value = i < 0 ? "" : line.slice(i + 1).replace(/^ /, "");
      if (field === "event") event = value;
      else if (field === "id") id = value;
      else if (field === "data") data.push(value);
    }
    if (data.length || event !== "message") events.push({ event, id, data: data.join("\n") });
  }
  return { events, rest };
}

/** Opens `path` as an event stream and calls `on` for each event until the stream ends or `signal` aborts. Resolves when the
 * stream ends; rejects with the server's refusal words (or a network failure). */
export async function streamEvents(path: string, on: (e: SseEvent) => void, signal: AbortSignal, lastEventId?: string): Promise<void> {
  const headers = new Headers({ "X-Console": "1", Accept: "text/event-stream" });
  if (lastEventId) headers.set("Last-Event-ID", lastEventId);
  const res = await fetch(`${basePath}${path}`, { headers, credentials: "same-origin", signal, cache: "no-store" });
  if (!res.ok || !res.body) {
    let message = `The console answered ${res.status}.`;
    try {
      const body = (await res.json()) as { message?: string };
      if (body.message) message = body.message;
    } catch {
      // not JSON
    }
    throw Object.assign(new Error(message), { status: res.status });
  }
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  for (;;) {
    const { done, value } = await reader.read();
    if (done) return;
    buffer += decoder.decode(value, { stream: true });
    const { events, rest } = parseEvents(buffer);
    buffer = rest;
    for (const e of events) on(e);
  }
}
