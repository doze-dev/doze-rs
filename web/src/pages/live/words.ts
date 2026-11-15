// The words of a row, as the classic console says them (pages/live/parts.rs): one place, so every table and Sheet agrees.
import type { Tone } from "@/components/app/status-dot";
import type { WireClass } from "@/api/types/WireClass";
import type { WireRow } from "@/api/types/WireRow";
import type { LabDid } from "@/api/types/LabDid";
import { number } from "../../lib/format";

/** `Done`, `Told to rejoin`, `Refused`... (`class_word`). */
export function classWord(c: WireClass, code: number): string {
  switch (c) {
    case "done":
      return "Done";
    case "told":
      return [27, 79, 110, 113].includes(code) ? "Told to rejoin" : "Told to retry";
    case "failed":
      return "Failed";
    case "refused":
      return "Refused";
    case "denied":
      return "Denied";
    case "closed":
      return "Closed";
  }
}

/** Denied is amber, Kafka's refusals violet, failures red (`class_css`). */
export function classTone(c: WireClass): Tone | "refused" {
  switch (c) {
    case "done":
      return "ok";
    case "told":
    case "denied":
      return "warn";
    case "failed":
    case "closed":
      return "bad";
    case "refused":
      return "refused";
  }
}

/** Whether the row is a refusal of any kind (what "Refused only" keeps). */
export function isRefusal(c: WireClass): boolean {
  return c === "refused" || c === "denied" || c === "closed" || c === "failed";
}

/** `<1 ms`, `14 ms`, `1,203 ms`. */
export function took(us: number): string {
  return us < 1000 ? "<1 ms" : `${number(Math.floor(us / 1000))} ms`;
}

/** `22:14:22.904` (UTC). */
export function clockMs(ms: number): string {
  const d = new Date(ms);
  const two = (n: number) => String(n).padStart(2, "0");
  return `${two(d.getUTCHours())}:${two(d.getUTCMinutes())}:${two(d.getUTCSeconds())}.${String(d.getUTCMilliseconds()).padStart(3, "0")}`;
}

/** `2026-11-13 22:14:22.904 UTC`: a row's time in full, for its tooltip. */
export function dateTime(ms: number): string {
  return `${new Date(ms).toISOString().slice(0, 10)} ${clockMs(ms)} UTC`;
}

/** What a Lab rule did to a request. */
export function labDid(d: LabDid): string {
  switch (d.kind) {
    case "delayed":
      return `delayed ${number(d.ms)} ms`;
    case "failed":
      return "failed with an error of its own";
    case "dropped":
      return "closed the connection";
  }
}

/** Whether the result cell shows the code (an error) or only what the result adds. */
export function showsCode(r: Pick<WireRow, "class" | "code">): boolean {
  return !(r.class === "done" || (r.class === "closed" && r.code === 0));
}
