// The edit pages that hold a document or a list: CORS, the bucket policy and the tags.
import { useState } from "react";
import { JsonEditor, checkJson, formatJson } from "@doze/ui/components/app/json-editor";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { filled, pairsOf, rowsFrom, rowsValid, type KeyValueRow } from "@doze/ui/components/app/key-values";
import { FormSection, PolicyEditor, TAG_RULES, changeWords, diffTags, noChange, useSetTags, useTags } from "../../../kit";
import type { OwnProps } from "../deep/registry";
import { NONE, bucketOf, useSetting } from "./api";
import { Loaded, SettingForm } from "./edit";
import { useAbout } from "../../../api/queries";
import { Mono } from "./parts";
import { policyExamples } from "./policy-examples";
import { PolicyExamples } from "./policy-examples-dialog";
import { bucketArn } from "./words";

const CORS_START = `[\n  {\n    "AllowedMethods": ["GET", "HEAD"],\n    "AllowedOrigins": ["http://localhost:3000"],\n    "AllowedHeaders": ["*"],\n    "MaxAgeSeconds": 3000\n  }\n]`;

export function CorsEdit({ name }: OwnProps) {
  const q = useSetting<{ CORSRules?: unknown[] }>(name!, "GetBucketCors", NONE.cors);
  return <Loaded q={q} what="CORS">{(c) => <CorsForm name={name!} start={c?.CORSRules ? JSON.stringify(c.CORSRules, null, 2) : CORS_START} />}</Loaded>;
}
function CorsForm({ name, start }: { name: string; start: string }) {
  const [text, setText] = useState(start);
  const parsed = checkJson(text, "invalid");
  const rules = parsed.ok && Array.isArray(parsed.value) ? parsed.value : null;
  const why = !parsed.ok ? parsed.why : rules === null ? "The configuration is a list of rules." : rules.length === 0 ? "Give at least one rule." : null;
  return (
    <SettingForm name={name} tab="permissions" title="Edit cross-origin resource sharing (CORS)" sub={`Let a web app on another origin read objects from ${name}.`} operation="PutBucketCors" input={rules && !why ? { CORSConfiguration: { CORSRules: rules } } : null} saved="CORS configuration saved." disabled={Boolean(why)} disabledWhy={why}>
      <FormSection title="CORS configuration editor">
        <p className="text-[13px] text-muted-foreground">Bucket ARN <Mono>{bucketArn(name)}</Mono></p>
        <JsonEditor value={text} onChange={setText} label="CORS rules" rows={16} empty="invalid" testId="cors-json" />
        <p className="text-[13px] text-muted-foreground">In the console the configuration must be valid JSON. Each rule has AllowedOrigins and AllowedMethods (GET, PUT, HEAD, POST or DELETE), and optionally AllowedHeaders, ExposeHeaders, MaxAgeSeconds (how long a browser caches the preflight answer) and an ID of up to 255 characters. Existing ACLs and policies still apply on top of CORS.</p>
      </FormSection>
    </SettingForm>
  );
}

const POLICY_START = (name: string) => JSON.stringify({ Version: "2012-10-17", Statement: [{ Sid: "ReadObjects", Effect: "Allow", Principal: { AWS: "arn:aws:iam::111122223333:root" }, Action: "s3:GetObject", Resource: `${bucketArn(name)}/*` }] }, null, 2);

export function PolicyEdit({ name }: OwnProps) {
  const q = useSetting<{ Policy?: string }>(name!, "GetBucketPolicy", NONE.policy);
  return <Loaded q={q} what="The bucket policy">{(p) => <PolicyForm name={name!} start={p?.Policy ? formatJson(p.Policy) : POLICY_START(name!)} />}</Loaded>;
}
function PolicyForm({ name, start }: { name: string; start: string }) {
  const [text, setText] = useState(start);
  const account = useAbout().data?.account ?? "";
  const parsed = checkJson(text, "invalid");
  return (
    <SettingForm name={name} tab="permissions" title="Edit bucket policy" sub={`Grant other identities access to ${name} and its objects.`} operation="PutBucketPolicy" input={parsed.ok ? { Policy: JSON.stringify(parsed.value) } : null} saved="Bucket policy saved." disabled={!parsed.ok} disabledWhy="The policy must be valid JSON." note="The policy travels as one JSON string.">
      <FormSection title="Policy">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <p className="text-[13px] text-muted-foreground">Bucket ARN <Mono>{bucketArn(name)}</Mono></p>
          <PolicyExamples examples={policyExamples(name, account)} onUse={setText} />
        </div>
        <PolicyEditor value={text} onChange={setText} kind="resource" maxSize={20 * 1024} label="Policy" rows={16} help="A bucket policy can be at most 20 KB. A statement that allows a public principal is rejected while Block public policy is on for this bucket." />
      </FormSection>
    </SettingForm>
  );
}

export function TagsEdit({ name }: OwnProps) {
  const q = useTags(bucketOf(name!));
  return <Loaded q={{ ...q, failed: q.error }} what="Tags">{(a) => <TagsForm name={name!} before={a?.tags ?? []} />}</Loaded>;
}
function TagsForm({ name, before }: { name: string; before: { key: string; value: string }[] }) {
  const [rows, setRows] = useState<KeyValueRow[]>(() => rowsFrom(before));
  const set = useSetTags(bucketOf(name));
  const change = diffTags(before, rows);
  const kept = filled(rows);
  const valid = rowsValid(rows, TAG_RULES) && !kept.some((r) => r.key.toLowerCase().startsWith("aws:"));
  return (
    <SettingForm name={name} tab="properties" title="Add Tags" sub={`Tags label ${name} for cost allocation. You can enter up to 50 tag key value pairs, and edit the values of existing tags on this page.`} operation="PutBucketTagging" input={{ Tagging: { TagSet: kept.length > 0 ? pairsOf(kept).map((t) => ({ Key: t.key, Value: t.value })) : [] } }} saved={`Tags saved: ${changeWords(change, before)}`} disabled={!valid || noChange(change)} disabledWhy={!valid ? "A tag key must be filled in, unique, and not start with aws:." : "Nothing has changed."}
      save={() => set.mutateAsync(change)} note={<>This console writes the tag set with PutBucketTagging, which doze-aws supports; the S3 User Guide also lists the S3 Control TagResource call for adding bucket tags. An empty set is a DeleteBucketTagging.</>}>
      <FormSection title="Tags">
        <KeyValueEditor rows={rows} onChange={setRows} rules={TAG_RULES} addLabel="Add Tag" testId="tags-kv" />
        <p className="text-[13px] text-muted-foreground">If you add a new tag with the same key as an existing tag, the value of the new tag overrides the existing value. A tag key must not start with the AWS reserved prefix aws:.</p>
      </FormSection>
    </SettingForm>
  );
}
