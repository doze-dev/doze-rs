// Where to see what a forced event did, from a kept entry (its kind and subject): the same addresses the server gives a fresh result
// (`lab_once::act::link`). Pure.

const segment = (s: string): string => encodeURIComponent(s);

/** The address of the page that shows the result of `kind` on `subject`, or null for a kind that has none. */
export function onceLink(kind: string, subject: string): string | null {
  switch (kind) {
    case "rest-service":
      return `/live?console=shown&service=${segment(subject)}`;
    case "fire-schedule":
      return `/resources/scheduler/schedules/${segment(subject)}`;
    case "expire-visibility":
      return `/resources/sqs/queues/${segment(subject)}`;
    case "sweep-ttl":
      return `/resources/dynamodb/tables/${segment(subject)}`;
    case "cold-start":
      return `/resources/lambda/functions/${segment(subject)}`;
    default:
      return null;
  }
}

/** An address of the console split into the path and the search a router link takes (`/live?service=sqs` is `/live` and `{ service: "sqs" }`). */
export function splitAddress(address: string): { path: string; search: Record<string, string> } {
  const [path = "/", query = ""] = address.split("?");
  return { path, search: Object.fromEntries(new URLSearchParams(query)) };
}
