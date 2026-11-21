// The sentence under a topic's name, which says something different on each tab (sns__topic-detail, topic-alerts, topic-access,
// topic-delivery-policy, topic-logging, topic-encryption, topic-tags, topic-archive, topic-created). Built from what the page has read;
// what doze-aws stores and does not act on is said so. Pure.
import { b, type Part } from "./sentences";
import { endpointName, type SubRow } from "./sub-model";
import { archiveOf, daysWords, hasOwnDelivery, LOGGED, type Logged, roleName, type Topic } from "./topic-model";
import { count, list } from "./words";

const NAMES: Record<Logged, string> = { HTTP: "HTTP", Lambda: "AWS Lambda", SQS: "Amazon SQS" };

const join = (parts: Part[][], last: string): Part[] => parts.flatMap((p, i) => (i === 0 ? p : [i === parts.length - 1 ? last : ", ", ...p]));

/** One subscription, as the sentence of a topic with some waiting says it. */
function who(s: SubRow): Part[] {
  const name = endpointName(s.protocol, s.endpoint);
  const state = s.pending ? (s.protocol === "email" || s.protocol === "email-json" ? null : " waits for its confirmation") : " is confirmed";
  if (s.protocol === "email" || s.protocol === "email-json") return [b(s.endpoint), s.pending ? " is stored and stays pending because doze-aws sends no email" : " is confirmed"];
  const noun = s.protocol === "lambda" ? "the Lambda function " : s.protocol === "sqs" ? "the queue " : "the webhook ";
  return [noun, b(name), state ?? ""];
}

/** The Subscriptions tab's sentence. `made`: the topic was just created (and the FIFO queue of the same name, if there is one). */
export function topicSentence(t: Pick<Topic, "name" | "fifo">, subs: readonly SubRow[], raw: string | null, made: { queue: string | null } | null): Part[] {
  if (subs.length === 0) {
    if (made) {
      const next = t.fifo ? (made.queue ? ["Subscribe the FIFO queue ", b(made.queue), " from SQS to start delivering."] : ["Subscribe a FIFO queue from SQS to start delivering."]) : ["Subscribe a queue, a function or an endpoint to start delivering."];
      return [b(t.name), " has no subscriptions yet, so a publish would reach nobody. ", ...next];
    }
    return [b(t.name), " has no subscriptions, so anything published to it goes nowhere."];
  }
  const confirmed = subs.filter((s) => !s.pending);
  if (confirmed.length === subs.length) {
    const names = confirmed.map((s) => endpointName(s.protocol, s.endpoint)).sort();
    const out: Part[] = [b(t.name), ` sends every publish to its ${confirmed.length === 1 ? "1 confirmed subscription" : `${confirmed.length} confirmed subscriptions`}`];
    if (names.length <= 5) out.push(": ", ...join(names.map((n) => [b(n)]), " and "));
    out.push(".");
    if (raw) out.push(` ${raw}`);
    return out;
  }
  const ordered = [...confirmed, ...subs.filter((s) => s.pending)];
  const reach = confirmed.length === 0 ? "A publish reaches nobody yet." : confirmed.length === 1 ? "A publish reaches only the confirmed one." : `A publish reaches only the ${confirmed.length} confirmed ones.`;
  return [b(t.name), ` has ${count(subs.length, "subscription")}: `, ...join(ordered.map(who), ", and "), `. ${reach}`];
}

/** The Access policy tab's sentence: the default policy said short; another one by its statements. */
export function accessSentence(name: string, policy: string | null, account: string): Part[] {
  let doc: { Statement?: unknown };
  try {
    doc = JSON.parse(policy ?? "{}") as { Statement?: unknown };
  } catch {
    return [b(name), " has an access policy that is not valid JSON."];
  }
  const statements = (Array.isArray(doc.Statement) ? doc.Statement : doc.Statement ? [doc.Statement] : []) as Record<string, unknown>[];
  if (statements.length === 0) return [b(name), " has no access policy statement, so only its owner can use it."];
  const owner = statements.length === 1 && statements[0]!.Effect === "Allow" && JSON.stringify(statements[0]!.Condition ?? {}).includes(account);
  if (owner) return [b(name), ` is open to its owner only: one statement allows any principal in account ${account} to publish, subscribe and manage the topic.`];
  return [b(name), ` has ${count(statements.length, "statement")} in its access policy: ${list(statements.map((s, i) => `${String(s.Sid ?? `statement ${i + 1}`)} ${String(s.Effect ?? "Allow").toLowerCase()}s ${typeof s.Principal === "string" ? "everyone" : "its principals"}`))}.`];
}

/** The delivery retry tab's sentence: whether the topic sets a policy of its own, and what that applies to. */
export function retrySentence(name: string, own: string | null, http: number): Part[] {
  const mine = hasOwnDelivery(own);
  const policy = mine ? [" sets a delivery retry policy of its own"] : [" sets no delivery policy of its own"];
  const reach = http === 0 ? [", and it has no HTTP/S subscriptions. "] : [`, so its ${count(http, "HTTP/S subscription")} ${http === 1 ? "is" : "are"} retried under ${mine ? "it" : "the defaults"}. `];
  return [b(name), ...policy, ...reach, "Amazon SQS and AWS Lambda deliveries are one attempt."];
}

/** The delivery status logging tab's sentence: for which protocols the roles are recorded. Nothing is written to CloudWatch Logs. */
export function loggingSentence(t: Pick<Topic, "name" | "feedback">): Part[] {
  const on = LOGGED.filter((l) => t.feedback[l].success !== null || t.feedback[l].failure !== null);
  if (on.length === 0) return [b(t.name), " records no delivery status logging."];
  const rates = on.map((l) => t.feedback[l].rate).filter((r): r is number => r !== null);
  return [b(t.name), ` records delivery status logging for ${list(on.map((l) => NAMES[l]))} only${rates.length > 0 ? `, with ${rates[0]}% of successful deliveries sampled` : ""}. Nothing is written to CloudWatch Logs locally; the roles and rates round-trip for templates and apps that read them.`];
}

export function encryptionSentence(name: string, kms: string | null): Part[] {
  return kms ? [b(name), " records the KMS key ", b(kms), " as its encryption key. Nothing is encrypted locally; the setting round-trips for templates and apps that read it."] : [b(name), " records no KMS key. Nothing is encrypted locally either way."];
}

export function tagsSentence(name: string, keys: readonly string[]): Part[] | null {
  if (keys.length === 0) return null;
  return [b(name), " carries ", b(count(keys.length, "tag")), ": ", ...join(keys.map((k) => [b(k)]), " and "), ". Tags change nothing about how it delivers; they feed cost allocation, IAM conditions and search."];
}

export function archiveSentence(name: string, archive: string | null): Part[] {
  const a = archiveOf(archive);
  return a.active ? [b(name), " has an archive policy of ", b(daysWords(a.days ?? 0)), ", set on the Edit topic page after the topic was created. Nothing is archived or replayable locally; the setting round-trips for templates and apps that read it."] : [b(name), " has no archive policy. Nothing is archived or replayable locally; a policy set on the Edit topic page round-trips for templates and apps that read it."];
}

export const roleWords = roleName;
