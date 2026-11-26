// A bucket's Properties tab: what is set on it, in sections that each have their own Edit (a page of its own).
import { PencilIcon, PlusIcon } from "lucide-react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Button } from "@doze/ui/components/ui/button";
import { useAbout } from "../../../api/queries";
import { useTags } from "../../../kit";
import { Card } from "../deep/parts";
import { bucketOf, NONE, useSetting } from "./api";
import { encryptionOf, notificationRules, propertiesSentence, type Rule, SSE_WORDS, websiteOf } from "./config";
import { NoneYet, OnOff, Reading, Rows, Mono } from "./parts";
import { bucketArn, longDate, regionWords, type Versioning, versioningOf } from "./words";

/** The reads of the tab; the page above it uses them for its sentence too (react-query makes each call once). */
export function useProperties(name: string, enabled: boolean) {
  const versioning = useSetting<{ Status?: string }>(name, "GetBucketVersioning", [], {}, enabled);
  const encryption = useSetting(name, "GetBucketEncryption", NONE.encryption, {}, enabled);
  const notifications = useSetting(name, "GetBucketNotificationConfiguration", [], {}, enabled);
  const lock = useSetting<{ ObjectLockConfiguration?: { ObjectLockEnabled?: string; Rule?: { DefaultRetention?: { Mode?: string; Days?: number; Years?: number } } } }>(name, "GetObjectLockConfiguration", NONE.lock, {}, enabled);
  const website = useSetting(name, "GetBucketWebsite", NONE.website, {}, enabled);
  const tags = useTags(bucketOf(name), enabled);
  const all = [versioning, encryption, notifications, lock, website];
  return { versioning, encryption, notifications, lock, website, tags, ready: all.every((q) => q.isSuccess || q.absent) };
}
type Properties = ReturnType<typeof useProperties>;

export function sentenceOf(name: string, p: Properties): string | null {
  if (!p.ready) return null;
  return propertiesSentence(name, {
    versioning: versioningOf(p.versioning.data?.Status),
    notifications: notificationRules(p.notifications.data).length,
    encryption: encryptionOf(p.encryption.data)?.algorithm ?? null,
    website: websiteOf(p.website.data) !== null,
    lock: p.lock.data?.ObjectLockConfiguration?.ObjectLockEnabled === "Enabled",
  });
}

const edit = (go: (d: string) => void, what: string, label = "Edit") => (
  <Button variant="outline" size="sm" className="bg-card" onClick={() => go(what)} data-testid={`${what}-open`}>
    <PencilIcon /> {label}
  </Button>
);

const RULE_COLUMNS: DataColumn<Rule>[] = [
  { id: "name", header: "Event name", size: 200, min: 120, accessorFn: (r) => r.id, meta: { mono: true }, cell: (c) => c.row.original.id || "–" },
  { id: "events", header: "Event types", size: 220, min: 140, accessorFn: (r) => r.events.join(), meta: { mono: true }, cell: (c) => c.row.original.events.join(", ") },
  { id: "prefix", header: "Prefix", size: 110, min: 70, accessorFn: (r) => r.prefix, meta: { mono: true }, cell: (c) => c.row.original.prefix || "–" },
  { id: "suffix", header: "Suffix", size: 110, min: 70, accessorFn: (r) => r.suffix, meta: { mono: true }, cell: (c) => c.row.original.suffix || "–" },
  { id: "dest", header: "Destination", accessorFn: (r) => r.name, cell: (c) => <span>{c.row.original.kind} <Mono>{c.row.original.name}</Mono></span> },
];

export function PropertiesTab({ name, region, created, p, go }: { name: string; region: string; created?: string; p: Properties; go: (action: string) => void }) {
  const about = useAbout();
  const versioning: Versioning = versioningOf(p.versioning.data?.Status);
  const enc = encryptionOf(p.encryption.data);
  const rules = notificationRules(p.notifications.data);
  const site = websiteOf(p.website.data);
  const lock = p.lock.data?.ObjectLockConfiguration;
  const retention = lock?.Rule?.DefaultRetention;
  const tags = p.tags.data?.tags ?? [];
  return (
    <div className="grid gap-6" data-testid="properties">
      <Card title="Bucket">
        <Rows rows={[["AWS Region", regionWords(region)], ["Amazon Resource Name (ARN)", <Mono key="a">{bucketArn(name)}</Mono>], ["Creation date", longDate(created)]]} />
      </Card>
      <Card title="Bucket Versioning" lede="Keep multiple versions of an object in one bucket." actions={edit(go, "versioning-edit")}>
        <Reading q={p.versioning} what="Versioning" height="h-16">
          <Rows rows={[["Bucket Versioning", versioning]]} />
        </Reading>
      </Card>
      <Card title="Tags" lede="Tags on a bucket label it for cost allocation." actions={tags.length > 0 ? edit(go, "tags-edit", "Add new Tag") : undefined}>
        <Reading q={{ ...p.tags, failed: p.tags.error }} what="Tags" height="h-16">
          {tags.length === 0 ? (
            <NoneYet title="No tags" testId="tags-none" action={<Button variant="outline" className="bg-card" onClick={() => go("tags-edit")} data-testid="tags-edit-open"><PencilIcon /> Edit tags</Button>}>
              {name} has no tags. Tags are key-value labels for organising buckets and tracking their cost.
            </NoneYet>
          ) : (
            <DataTable aria-label="Tags" data={tags} rowId={(t) => t.key} maxHeight={320} columns={[{ id: "key", header: "Key", size: 320, accessorKey: "key", meta: { mono: true } }, { id: "value", header: "Value", accessorKey: "value", meta: { mono: true } }]} />
          )}
        </Reading>
      </Card>
      <Card title="Default encryption" lede="The encryption type new objects in the bucket are meant to get. Stored, not applied locally." actions={edit(go, "encryption-edit")}>
        <Reading q={p.encryption} what="Default encryption" height="h-16">
          <Rows rows={[["Encryption type", enc ? SSE_WORDS[enc.algorithm] : "None set: new objects get SSE-S3, the default", ...(enc?.key ? [["AWS KMS key", <Mono key="k">{enc.key}</Mono>] as [string, React.ReactNode]] : []), ...(enc?.algorithm === "aws:kms" ? [["Bucket Key", enc.bucketKey ? "Enabled" : "Disabled"] as [string, React.ReactNode]] : [])] as [string, React.ReactNode]]} />
        </Reading>
      </Card>
      <Card title="Event notifications" lede="Send a notification message to a Lambda function, an SNS topic or an SQS queue whenever the events you choose occur in this bucket." actions={rules.length > 0 ? <Button onClick={() => go("notification-create")} data-testid="notification-create-open"><PlusIcon /> Create event notification</Button> : undefined}>
        <Reading q={p.notifications} what="Event notifications" height="h-16">
          {rules.length === 0 ? (
            <NoneYet title="No event notifications" testId="notifications-none" action={<Button variant="outline" className="bg-card" onClick={() => go("notification-create")} data-testid="notification-create-open"><PlusIcon /> Create event notification</Button>}>
              {name} sends no notifications when objects are created or removed.
            </NoneYet>
          ) : (
            <DataTable aria-label="Event notifications" data={rules} columns={RULE_COLUMNS} rowId={(r) => `${r.kind}|${r.id}|${r.arn}`} maxHeight={320} />
          )}
        </Reading>
      </Card>
      <Card title="Object Lock" lede="Prevent an object from being deleted or overwritten for a fixed time or indefinitely.">
        <Reading q={p.lock} what="Object Lock" height="h-16">
          <Rows rows={[["Object Lock", <OnOff key="lock" on={lock?.ObjectLockEnabled === "Enabled"} />], ...(retention ? [["Default retention", `${retention.Mode ?? ""} ${retention.Days ? `${retention.Days} days` : retention.Years ? `${retention.Years} years` : ""}`.trim()] as [string, React.ReactNode]] : [])]} />
        </Reading>
      </Card>
      <Card title="Static website hosting" lede="Host a static website from this bucket." actions={edit(go, "website-edit")}>
        <Reading q={p.website} what="Static website hosting" height="h-16">
          <Rows rows={[["Static website hosting", <OnOff key="site" on={Boolean(site)} />], ...(site?.index ? ([["Index document", <Mono key="i">{site.index}</Mono>], ...(site.error ? [["Error document", <Mono key="e">{site.error}</Mono>]] : []), ["Bucket website endpoint", <Mono key="u">{`${about.data?.endpoint ?? ""}/_aws/s3-website/${name}/`}</Mono>]] as [string, React.ReactNode][]) : site ? [["Redirects every request to", <Mono key="r">{`${site.redirectProtocol ? `${site.redirectProtocol}://` : ""}${site.redirectHost}`}</Mono>] as [string, React.ReactNode]] : [])]} />
        </Reading>
      </Card>
    </div>
  );
}
