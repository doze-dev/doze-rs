// The lines under Create bucket's choices that name what the account already has (the board's "product-images turned it on"):
// the buckets that keep versions and whom they notify, and the tag keys the buckets use. Pure.
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { listWords } from "./words";

/** The columns the list of buckets adds after Region and Created: Versioning, then the first rule's destination by kind, then its event. */
const [VERSIONING, LAMBDA, QUEUE, TOPIC, EVENT] = [2, 3, 4, 5, 6];

/** `product-images turned it on; the ObjectCreated notification to resize-product-image fires for each new version.` */
export function versioningHint(rows: readonly ResourceRow[]): string | null {
  const on = rows.find((r) => r.cells[VERSIONING] === "Enabled");
  if (!on) return null;
  const to = on.cells[LAMBDA] ?? on.cells[QUEUE] ?? on.cells[TOPIC];
  const event = on.cells[EVENT]?.split(":")[1];
  const fires = to && event ? `; the ${event} notification to ${to} fires for each new version` : "";
  return `Versioning is off by default. ${on.name} turned it on${fires}.`;
}

/** `product-images uses app and owner.` for the first bucket that has tags (`tagged`: each bucket's tag keys). */
export function tagsHint(tagged: readonly { bucket: string; keys: string[] }[]): string | null {
  const one = tagged.find((t) => t.keys.length > 0);
  return one ? `${one.bucket} uses ${listWords(one.keys.slice(0, 4))}.` : null;
}

/** `text` as the start of a sentence: its first letter capitalised (`versioning off, ...` after a full stop). */
export const startSentence = (text: string): string => text.charAt(0).toUpperCase() + text.slice(1);

/** What Create bucket says when the name is one of the account's own buckets. In us-east-1 S3 answers CreateBucket for a bucket you own
 * with 200 and changes nothing, so the page would say it made one: it checks the account's buckets first and sends nothing. */
export function ownedRefusal(name: string): { title: string; why: string; suggestion: string } {
  return {
    title: "Bucket not created.",
    why: `${name} is already your bucket. In us-east-1 S3 answers CreateBucket for a bucket you own with 200 and changes nothing, so nothing was sent.`,
    suggestion: "Choose another name, or open it from the list.",
  };
}
