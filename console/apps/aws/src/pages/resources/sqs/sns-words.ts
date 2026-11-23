// What an empty SNS tab says of the topics there are: the busiest one and where it delivers. Pure.

/** `a, b and c`. */
export const listOf = (names: readonly string[]): string => (names.length <= 1 ? (names[0] ?? "") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`);

/** `The topic orders-created delivers to invoice-queue, inventory-queue and analytics-queue today.` When the endpoints are not known
 * (not read yet, or not queues) it counts them: `delivers to 3 subscriptions today.` */
export function subscribedWords(topic: string, count: number, endpoints: readonly string[]): string {
  const to = endpoints.length > 0 && endpoints.length === count ? listOf(endpoints) : `${count} ${count === 1 ? "subscription" : "subscriptions"}`;
  return `The topic ${topic} delivers to ${to} today.`;
}

/** The queue names among a topic's subscription endpoints (`arn:aws:sqs:...:name`), by name (the subscriptions come in no order worth keeping). */
export const queueEndpoints = (endpoints: readonly (string | null | undefined)[]): string[] => endpoints.filter((e): e is string => Boolean(e) && e!.startsWith("arn:aws:sqs:")).map((e) => e.split(":").pop() ?? e).sort();
