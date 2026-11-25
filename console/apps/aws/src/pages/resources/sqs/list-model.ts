// The queues' list rows, read from the console's columns (crates/aws-console/src/resources/sqs.rs): the page shows encryption as
// one column and the dead-letter queue only to say which queues are one.
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { timeMs } from "../model";
import { encrypted } from "./queue-model";

export type QueueRow = { name: string; type: "FIFO" | "Standard"; available: number; inFlight: number; dlq: string | null; created: string | null; createdMs: number | null; encrypted: boolean; dedup: boolean };

const COUNT = (v: string | null | undefined): number => {
  const n = Number(v);
  return Number.isFinite(n) ? n : 0;
};

/** Cells are Type, Messages available, In flight, Dead-letter queue, Created, SQS-managed encryption, KMS key, Content-based deduplication. */
export function listRows(rows: readonly ResourceRow[]): QueueRow[] {
  return rows.map((r) => {
    const c = r.cells;
    const created = c[4] ?? null;
    return {
      name: r.name,
      type: c[0] === "FIFO" ? "FIFO" : "Standard",
      available: COUNT(c[1]),
      inFlight: COUNT(c[2]),
      dlq: c[3] || null,
      created,
      createdMs: created ? timeMs(created) : null,
      encrypted: encrypted(c[5] ?? null, c[6] ?? null),
      dedup: c[7] === "Enabled",
    };
  });
}

/** What a delete leaves: AWS keeps the name for 60 seconds. doze-aws does the same unless it was started with the flag. */
export const deletedWords = (): string =>
  "The queue and its messages are gone. AWS refuses to create a queue of the same name for 60 seconds (QueueDeletedRecently); doze-aws does the same unless it was started with --sqs-skip-60s-waits.";

/** A queue's creation in the list, to the minute and in UTC, which the column's header says (`2026-11-06 18:42`; the board shows it to the minute
 * too): the seconds made the column wider than the page's box. The whole time is the cell's title. */
export function createdWords(ms: number | null): { short: string; full: string } | null {
  if (ms === null) return null;
  const full = dateTimeUtc(ms);
  return { short: full.replace(/:\d\d UTC$/, ""), full };
}
