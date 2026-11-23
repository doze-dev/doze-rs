// The pattern step's pure parts: the sample events, what a pattern names (so a test can say what it checked), and the form's two fields.

export const SAMPLE_OWN = `{
  "version": "0",
  "id": "5f1c2d4e-9b7a-4c33-8a10-0e6f3b2a7d11",
  "detail-type": "OrderPlaced",
  "source": "larder.orders",
  "account": "123456789012",
  "time": "2026-11-08T08:13:41Z",
  "region": "us-east-1",
  "resources": [],
  "detail": {
    "order_id": "o-20931",
    "customer_id": "c-1187",
    "total_cents": 4850
  }
}`;

/** An event an AWS service puts on the default bus, for "AWS events". */
export const SAMPLE_AWS = `{
  "version": "0",
  "id": "6a7e8feb-b491-4cf7-a9f1-bf3703467718",
  "detail-type": "Object Created",
  "source": "aws.s3",
  "account": "123456789012",
  "time": "2026-11-08T08:13:41Z",
  "region": "us-east-1",
  "resources": ["arn:aws:s3:::product-images"],
  "detail": {
    "bucket": { "name": "product-images" },
    "object": { "key": "hero.png", "size": 5342 },
    "reason": "PutObject"
  }
}`;

export type SampleKind = "aws" | "own";

/** The fields a pattern names, as dotted paths: `source`, `detail-type`, `detail.total_cents`. */
export function patternFields(pattern: string): string[] {
  try {
    const walk = (v: unknown, at: string): string[] => (v !== null && typeof v === "object" && !Array.isArray(v) ? Object.entries(v).flatMap(([k, x]) => walk(x, at ? `${at}.${k}` : k)) : [at]);
    return walk(JSON.parse(pattern), "");
  } catch {
    return [];
  }
}

/** What the test of a pattern against a sample said, in words. */
export function testWords(matches: boolean, pattern: string): string {
  const fields = patternFields(pattern);
  const named = fields.length > 0 ? fields.join(", ") : "its fields";
  return matches ? `The sample event matches the event pattern. It checked ${named}, and the sample has them all.` : `The sample event does not match the event pattern. It checked ${named}; at least one of them is missing in the sample or has another value.`;
}

/** The pattern with `source` and `detail-type` set from the form's two fields (comma-separated); an emptied field leaves its member out. */
export function patternWithForm(pattern: string, source: string, detailType: string): string {
  let p: Record<string, unknown> = {};
  try {
    const v = JSON.parse(pattern) as unknown;
    if (v !== null && typeof v === "object" && !Array.isArray(v)) p = v as Record<string, unknown>;
  } catch {
    /* start again from the two fields */
  }
  const words = (t: string) => t.split(",").map((x) => x.trim()).filter(Boolean);
  const set = (key: string, t: string) => (words(t).length > 0 ? (p[key] = words(t)) : delete p[key]);
  set("source", source);
  set("detail-type", detailType);
  return JSON.stringify(p, null, 2);
}

/** The two form fields as the pattern holds them now. */
export function formOf(pattern: string): { source: string; detailType: string } {
  try {
    const p = JSON.parse(pattern) as Record<string, unknown>;
    const t = (k: string) => (Array.isArray(p[k]) ? (p[k] as unknown[]).filter((x): x is string => typeof x === "string").join(", ") : "");
    return { source: t("source"), detailType: t("detail-type") };
  } catch {
    return { source: "", detailType: "" };
  }
}
