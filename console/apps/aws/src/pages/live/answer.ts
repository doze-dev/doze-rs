// What the record knows of an answer beyond its status: how long it declared itself to be (a whole body; a streamed answer says
// none) and, for a function invocation, whether the function failed. The record keeps no bodies, so a sentence is as much as the
// length allows: an empty answer is `{}`, two bytes.
import { bytes } from "@doze/ui/lib/format";
import type { CallRecord } from "../../api/types/CallRecord";

type Answered = Pick<CallRecord, "action" | "answer_bytes" | "outcome" | "function_error"> & { summary?: string | null };

/** The few words after a success: what the recorder read of the call (`3 messages`, `sent 53 B`, `6 keys`) when it has a summary
 * for the operation; else `nothing` for a read of an empty answer, else the size. Null when the answer declared none (a stream, a
 * call between services). */
export function answerWords(c: Answered): string | null {
  if (c.outcome !== "ok" || c.function_error) return null;
  if (c.summary) return c.summary;
  if (c.answer_bytes === null) return null;
  const empty = c.answer_bytes <= 2;
  if (c.action === "ReceiveMessage") return empty ? "empty" : "has messages";
  if (c.action !== null && /^(List|Describe|Get|Query|Scan|Search)/.test(c.action) && empty) return "nothing in the answer";
  return bytes(c.answer_bytes);
}

/** `Unhandled` or `Handled` when a function invocation answered 200 and the function failed. */
export const functionFailed = (c: Pick<CallRecord, "function_error">): string | null => c.function_error;
