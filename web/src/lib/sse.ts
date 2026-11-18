// A reader of `text/event-stream` over fetch. The browser's EventSource cannot send the console's proof header (X-Console),
// so the tail reads the stream itself: the same format (WHATWG HTML, "server-sent events"), the same `Last-Event-ID`.

export type SseEvent = { event: string; data: string; id?: string };

/** An incremental parser: feed it text as it arrives, it returns the events completed by that text. */
export class SseParser {
  private buf = "";
  private event = "";
  private data: string[] = [];
  private id: string | undefined;

  push(chunk: string): SseEvent[] {
    this.buf += chunk;
    const out: SseEvent[] = [];
    for (;;) {
      const nl = this.buf.search(/\r\n|\r|\n/);
      if (nl < 0) break;
      const line = this.buf.slice(0, nl);
      const width = this.buf.startsWith("\r\n", nl) ? 2 : 1;
      this.buf = this.buf.slice(nl + width);
      if (line === "") {
        if (this.data.length > 0) out.push({ event: this.event || "message", data: this.data.join("\n"), id: this.id });
        this.event = "";
        this.data = [];
        this.id = undefined;
        continue;
      }
      if (line.startsWith(":")) continue;
      const colon = line.indexOf(":");
      const field = colon < 0 ? line : line.slice(0, colon);
      let value = colon < 0 ? "" : line.slice(colon + 1);
      if (value.startsWith(" ")) value = value.slice(1);
      if (field === "event") this.event = value;
      else if (field === "data") this.data.push(value);
      else if (field === "id" && !value.includes("\0")) this.id = value;
    }
    return out;
  }
}
