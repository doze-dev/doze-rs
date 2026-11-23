// A queue's access policy read as who it lets in (sqs__queue-access-policy): one line a principal, from the statements.
export type Reading = { who: string; what: string };

const SERVICES: Record<string, string> = { "events.amazonaws.com": "EventBridge", "sns.amazonaws.com": "Amazon SNS", "s3.amazonaws.com": "Amazon S3", "lambda.amazonaws.com": "Lambda", "scheduler.amazonaws.com": "EventBridge Scheduler", "logs.amazonaws.com": "CloudWatch Logs" };

const list = (v: unknown): string[] => (typeof v === "string" ? [v] : Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);

/** `arn:aws:events:us-east-1:1:rule/larder/payment-settled` as `the rule payment-settled`; a topic, queue or bucket likewise. */
export function sourceWords(arn: string): string {
  const m = /^arn:aws[^:]*:([a-z0-9-]+):[^:]*:[^:]*:(.+)$/.exec(arn);
  if (!m) return arn;
  const [, service, rest] = m;
  const last = rest!.split(/[/:]/).pop()!;
  if (service === "events" && rest!.startsWith("rule/")) return `the rule ${last}`;
  if (service === "sns") return `the topic ${last}`;
  if (service === "sqs") return `the queue ${last}`;
  if (service === "s3") return `the bucket ${last}`;
  return arn;
}

const actionWords = (actions: string[]): string => {
  const short = actions.map((a) => a.replace(/^sqs:/, ""));
  if (short.includes("*")) return "every action";
  if (short.length === 0) return "nothing";
  const rw = ["ReceiveMessage", "DeleteMessage"].every((x) => short.includes(x)) && short.includes("SendMessage");
  return rw ? "send and receive" : short.join(", ");
};

/** The owner first (the account always may), then each Allow statement: who, and what, and from where. A Deny statement is said as a refusal. */
export function whoLetsIn(policy: string | null, account: string): Reading[] {
  const out: Reading[] = [{ who: "Queue owner", what: `Account ${account}: send and receive, by default` }];
  if (!policy) return out;
  let doc: { Statement?: unknown };
  try {
    doc = JSON.parse(policy) as { Statement?: unknown };
  } catch {
    return out;
  }
  const raw = Array.isArray(doc.Statement) ? doc.Statement : doc.Statement ? [doc.Statement] : [];
  for (const s of raw as Record<string, unknown>[]) {
    const principal = s.Principal;
    const ids: string[] = principal === "*" ? ["*"] : typeof principal === "object" && principal !== null ? Object.values(principal as Record<string, unknown>).flatMap(list) : [];
    const cond = s.Condition as Record<string, Record<string, unknown>> | undefined;
    const sources = cond ? Object.values(cond).flatMap((c) => ["aws:SourceArn", "aws:sourceArn"].flatMap((k) => list(c[k]))) : [];
    const deny = s.Effect === "Deny";
    const verb = actionWords(list(s.Action));
    for (const id of ids.length > 0 ? ids : ["?"]) {
      const who = id === "*" ? "Everyone" : (SERVICES[id] ?? (id.startsWith("arn:") ? id.split(":").slice(5).join(":") : id));
      const from = sources.length > 0 ? `, only from ${sources.map(sourceWords).join(" and ")}` : "";
      out.push({ who, what: deny ? `Refused: ${verb}${from}` : `${verb[0]!.toUpperCase()}${verb.slice(1)}${from}` });
    }
  }
  return out;
}
