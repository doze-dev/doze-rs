// What the alarm wizard says about the topic an alarm notifies (from its subscriptions). Pure.

export type Subscriber = { protocol: string; endpoint: string };

const last = (arn: string) => arn.split(":").pop() ?? arn;

/** `alerts delivers to the Lambda function send-email, which receives the alarm's JSON.` */
export function subscribers(topic: string, subs: readonly Subscriber[]): string {
  if (subs.length === 0) return `${topic} has no subscriptions, so a message published to it goes nowhere.`;
  const to = subs.map((s) => {
    if (s.protocol === "lambda") return `the Lambda function ${last(s.endpoint)}`;
    if (s.protocol === "sqs") return `the queue ${last(s.endpoint)}`;
    if (s.protocol === "email" || s.protocol === "email-json") return `the address ${s.endpoint}`;
    return `${s.protocol} ${s.endpoint}`;
  });
  const list = to.length === 1 ? to[0]! : `${to.slice(0, -1).join(", ")} and ${to[to.length - 1]}`;
  return `${topic} delivers to ${list}, which ${subs.length === 1 ? "receives" : "receive"} the alarm's JSON.`;
}
