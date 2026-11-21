// The documents of policies, read for what a page says of them: a role's only permission, the resources a role is wired to.
import { useMemo } from "react";
import { POLICIES, useEach } from "./api";
import { type Doc, docText, isAwsManaged } from "./model";

/** The default version's document of each customer managed policy of `arns` (an AWS managed policy's is made by doze-aws from its name, and is not read). */
export function usePolicyDocs(arns: readonly string[]): Map<string, Doc> {
  const key = arns.join("\u0000");
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const mine = useMemo(() => arns.filter((a) => !isAwsManaged(a)), [key]);
  const heads = useEach<{ Policy: { DefaultVersionId: string } }>(POLICIES, "GetPolicy", mine.map((PolicyArn) => ({ PolicyArn })));
  const versions = useEach<{ PolicyVersion: { Document: Doc } }>(POLICIES, "GetPolicyVersion", mine.flatMap((PolicyArn, i) => (heads[i]?.data ? [{ PolicyArn, VersionId: heads[i]!.data!.Policy.DefaultVersionId }] : [])));
  const out = new Map<string, Doc>();
  let at = 0;
  mine.forEach((arn, i) => {
    if (!heads[i]?.data) return;
    const d = versions[at++]?.data?.PolicyVersion.Document;
    if (d !== undefined) out.set(arn, d);
  });
  return out;
}

export type Linked = { service: string; kind: string; name: string; label: string };

/** The resources the resources of a policy document name that the console has a page for: a bucket, a queue, a table, a topic, a function. */
export function docResources(docs: Iterable<Doc>): Linked[] {
  const out = new Map<string, Linked>();
  const add = (l: Linked) => out.set(`${l.service}/${l.name}`, l);
  for (const doc of docs) {
    const text = docText(doc);
    for (const m of text.matchAll(/arn:aws[\w-]*:([a-z0-9-]+):[a-z0-9-]*:\d*:([^"\s,\]]+)/g)) {
      const [, svc, rest] = m;
      if (svc === "sqs" && rest && !rest.includes("*")) add({ service: "sqs", kind: "queues", name: rest, label: `${rest} queue` });
      else if (svc === "sns" && rest && !rest.includes("*")) add({ service: "sns", kind: "topics", name: rest, label: `${rest} topic` });
      else if (svc === "dynamodb" && rest?.startsWith("table/")) add({ service: "dynamodb", kind: "tables", name: rest.slice(6).split("/")[0]!, label: `${rest.slice(6).split("/")[0]} table` });
      else if (svc === "lambda" && rest?.startsWith("function:") && !rest.includes("*")) add({ service: "lambda", kind: "functions", name: rest.slice(9).split(":")[0]!, label: `${rest.slice(9).split(":")[0]} function` });
    }
    for (const m of text.matchAll(/arn:aws[\w-]*:s3:::([^"\s,\]/*]+)/g)) if (m[1]) add({ service: "s3", kind: "buckets", name: m[1], label: `${m[1]} bucket` });
  }
  return [...out.values()];
}
