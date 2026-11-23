// The actions the Visual policy editor offers: the operations each service has built, written as IAM writes them (`sqs:SendMessage`).
// An operation is nearly always its own IAM action; the few that are not are renamed here (a batch call is authorised as the single call,
// a list or a head of S3 as ListBucket or GetObject). Anything else may be typed by name, so no action is out of reach. Pure.

/** A built operation of a service, by the name the Lab lists it under, and how IAM names the service. */
export type Target = { target: string; title: string; served: boolean; operations: readonly string[] };
export type ServiceActions = { prefix: string; title: string; actions: string[] };

/** The IAM prefix of a service whose name on the bus (its signing name) is not it. */
const PREFIX: Readonly<Record<string, string>> = { monitoring: "cloudwatch", apigatewayv2: "apigateway", "execute-api": "execute-api" };

/** Operations IAM authorises under another action (`null`: no action of its own, it is the one a person already has). */
const RENAMED: Readonly<Record<string, Readonly<Record<string, string | null>>>> = {
  sqs: { SendMessageBatch: "SendMessage", DeleteMessageBatch: "DeleteMessage", ChangeMessageVisibilityBatch: "ChangeMessageVisibility" },
  s3: { ListObjects: "ListBucket", ListObjectsV2: "ListBucket", HeadBucket: "ListBucket", HeadObject: "GetObject", ListBuckets: "ListAllMyBuckets", ListObjectVersions: "ListBucketVersions", DeleteObjects: "DeleteObject", CreateMultipartUpload: "PutObject", UploadPart: "PutObject", CompleteMultipartUpload: "PutObject", UploadPartCopy: "PutObject", CopyObject: "PutObject", ListMultipartUploads: "ListBucketMultipartUploads", ListParts: "ListMultipartUploadParts" },
  lambda: { Invoke: "InvokeFunction", InvokeWithResponseStream: "InvokeFunction" },
  dynamodb: {},
};

export const prefixOf = (target: string): string => PREFIX[target] ?? target;

/** The services and their actions, from what the Lab lists: sorted by name; services that share a prefix (DynamoDB and its streams, the two
 * API Gateways) are one. A service with nothing built is left out. */
export function catalogue(targets: readonly Target[]): ServiceActions[] {
  const by = new Map<string, { title: string; actions: Set<string> }>();
  for (const t of targets) {
    if (!t.served || t.operations.length === 0) continue;
    const prefix = prefixOf(t.target);
    const renames = RENAMED[prefix] ?? {};
    const got = by.get(prefix) ?? { title: t.title, actions: new Set<string>() };
    for (const op of t.operations) {
      const name = op in renames ? renames[op] : op;
      if (name) got.actions.add(name);
    }
    by.set(prefix, got);
  }
  return [...by.entries()].map(([prefix, v]) => ({ prefix, title: v.title, actions: [...v.actions].sort() })).sort((a, b) => a.prefix.localeCompare(b.prefix));
}

/** An action that only reads: what a person means by "read only". */
export const readsOnly = (action: string): boolean => /^(Get|List|Describe|Query|Scan|Batch ?Get|Search|Select|Simulate|Test|Lookup|Read)/.test(action);
