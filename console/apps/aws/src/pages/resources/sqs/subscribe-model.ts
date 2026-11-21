// Subscribing a queue to a topic: the statement that lets the topic send to it, added to the queue's access policy (pure).
const statements = (doc: Record<string, unknown>): unknown[] => (Array.isArray(doc.Statement) ? doc.Statement : doc.Statement ? [doc.Statement] : []);

const parse = (policy: string | null): Record<string, unknown> => {
  try {
    const v: unknown = policy ? JSON.parse(policy) : {};
    return typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : {};
  } catch {
    return {};
  }
};

/** How many statements the policy has now. */
export const statementCount = (policy: string | null): number => statements(parse(policy)).length;

/** The policy with the statement that lets `topicArn` send to the queue; the same policy when it is already there. */
export function withTopic(policy: string | null, queueArn: string, topicArn: string): { policy: string; count: number; added: boolean } {
  const doc = parse(policy);
  const had = statements(doc);
  const present = had.some((s) => JSON.stringify(s).includes(topicArn) && JSON.stringify(s).includes("sns.amazonaws.com"));
  if (present) return { policy: JSON.stringify(doc), count: had.length, added: false };
  const sid = `topic-${topicArn.split(":").pop()}`;
  const next = [...had, { Sid: sid, Effect: "Allow", Principal: { Service: "sns.amazonaws.com" }, Action: "sqs:SendMessage", Resource: queueArn, Condition: { ArnEquals: { "aws:SourceArn": topicArn } } }];
  return { policy: JSON.stringify({ Version: "2012-10-17", ...doc, Statement: next }), count: next.length, added: true };
}

/** What the other topics are and what they already deliver, drawn from the list: `alerts is the other topic here; orders-created already has 3 subscriptions.` */
export function topicHint(others: readonly { name: string; cells: (string | null)[] }[], chosen?: { name: string; cells: (string | null)[] } | null): string {
  const base = "The topics in this account and Region are listed. If yours isn't, enter its ARN.";
  const count = (r: { cells: (string | null)[] }) => Number(r.cells[1]);
  const has = (r: { name: string; cells: (string | null)[] }) => `${r.name} already has ${count(r)} ${count(r) === 1 ? "subscription" : "subscriptions"}`;
  if (others.length === 0) return chosen && count(chosen) > 0 ? `${base} ${has(chosen)}.` : base;
  const names = others.map((r) => r.name);
  const subs = [...(chosen && count(chosen) > 0 ? [chosen] : []), ...others.filter((r) => count(r) > 0)].map(has);
  return `${base} ${names.join(", ")} ${names.length === 1 ? "is the other topic" : "are the other topics"} here${subs.length > 0 ? `; ${subs.join("; ")}` : ""}.`;
}
