// Create bucket: a page, not a dialog. The name and its rules, a bucket to copy settings from, Object Ownership, Block Public
// Access, versioning, tags, default encryption and Object Lock, with the call it will make at the right. CreateBucket takes the
// name, the ownership and Object Lock; the rest are the calls that follow it, made in turn, and a refusal of any says which.
import { useQueries, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { filled, newRow, pairsOf, rowsFrom, type KeyValueRow } from "@doze/ui/components/app/key-values";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { ApiFailure } from "@doze/ui/lib/api";
import { client } from "../../../api/client";
import { useAbout } from "../../../api/queries";
import { useResourceList } from "../../../api/resources";
import { Field, FormPage, FormSection, LocalStatus, type PreviewRequest, ResourcePicker, TAG_RULES, callKindOp, tagsPath } from "../../../kit";
import { callOp } from "../deep/api";
import { BPA_ALL, bpaOf, bpaOn, type Bpa, encryptionConfig, encryptionOf, ownershipOf, type Ownership } from "./config";
import { bucketOf, useListNav } from "./api";
import { BpaChoice, type Encryption, EncryptionChoice, OwnershipChoice, RadioCards } from "./controls";
import { setFlash } from "./flash";
import { tagsHint, versioningHint } from "./create-hints";
import { useKmsKeys } from "./kms-hook";
import { NAME_RULE, listWords, nameRefusal, regionWords } from "./words";

const KIND = { service: "s3", kind: "buckets" };

/** What the settings chosen make of the bucket, in the words of the page that opens after: `versioning off, all four Block Public Access settings on, nothing in it yet`. */
export function madeWords(f: { versioning: boolean; bpa: Bpa; lock: boolean }): string {
  // Object Lock turns versioning on with it.
  const bits = [`versioning ${f.versioning || f.lock ? "on" : "off"}`, bpaOn(f.bpa) === 4 ? "all four Block Public Access settings on" : `${bpaOn(f.bpa)} of the four Block Public Access settings on`];
  if (f.lock) bits.push("Object Lock on");
  return `${listWords(bits)}, nothing in it yet`;
}

export function BucketCreate() {
  const about = useAbout();
  const qc = useQueryClient();
  const go = useListNav();
  const existing = useResourceList("s3", "buckets");
  const rows = existing.data?.pages.flatMap((p) => p.rows) ?? [];
  const names = rows.map((r) => r.name);
  // The tag keys the account's buckets use (the first few), for the line under Tags.
  const tagged = useQueries({ queries: rows.slice(0, 6).map((r) => ({ queryKey: ["resources", "tags", "s3", "buckets", r.name], queryFn: ({ signal }: { signal: AbortSignal }) => client.api<{ tags: { key: string; value: string }[] }>(tagsPath({ ...KIND, name: r.name }), {}, signal), retry: false })) });
  const tagKeys = tagged.map((q, i) => ({ bucket: rows[i]?.name ?? "", keys: q.data?.tags.map((t) => t.key) ?? [] }));
  const kms = useKmsKeys();
  const firstKey = kms.keys.usable[0]?.label;
  const [name, setName] = useState("");
  const [ownership, setOwnership] = useState<Ownership>("BucketOwnerEnforced");
  const [bpa, setBpa] = useState<Bpa>(BPA_ALL);
  const [versioning, setVersioning] = useState(false);
  const [tags, setTags] = useState<KeyValueRow[]>(() => [newRow()]);
  const [enc, setEnc] = useState<Encryption>({ algorithm: "AES256", keyMode: "list", key: "", bucketKey: false });
  const [lock, setLock] = useState(false);
  const [acknowledged, setAcknowledged] = useState(false);
  const [from, setFrom] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<{ step: string; error: unknown } | null>(null);

  const input = { Bucket: name, ObjectOwnership: ownership, ...(lock ? { ObjectLockEnabledForBucket: true } : {}) };
  const request: PreviewRequest | null = name ? { service: "s3", operation: "CreateBucket", input } : null;
  const refused = failure?.step === "CreateBucket" ? failure.error : null;
  const code = refused instanceof ApiFailure ? refused.code : "";
  const nameRefused = refused ? (code === "InvalidBucketName" ? nameRefusal(name) : code.startsWith("BucketAlready") ? { why: `${name} is taken: ${errorWords(refused).message}`, suggestion: "Choose another name." } : null) : null;
  const said = refused ? errorWords(refused) : null;

  /** Takes the settings of another bucket into this form (not its policy). */
  const copyFrom = async (source: string | null) => {
    setFrom(source);
    setCopied(null);
    if (!source) return;
    const t = bucketOf(source);
    const read = async <T,>(op: string): Promise<T | null> => callOp<T>(t, op, {}).catch(() => null);
    const [o, p, v, e, g] = await Promise.all([read("GetBucketOwnershipControls"), read("GetPublicAccessBlock"), read("GetBucketVersioning"), read("GetBucketEncryption"), client.api<{ tags: { key: string; value: string }[] }>(tagsPath({ ...KIND, name: source })).catch(() => null)]);
    const own = ownershipOf(o);
    if (own) setOwnership(own);
    if (p) setBpa(bpaOf(p));
    setVersioning((v as { Status?: string } | null)?.Status === "Enabled");
    const sse = encryptionOf(e);
    setEnc(sse ? { algorithm: sse.algorithm, keyMode: "arn", key: sse.key ?? "", bucketKey: sse.bucketKey } : { algorithm: "AES256", keyMode: "list", key: "", bucketKey: false });
    setTags(g && g.tags.length > 0 ? rowsFrom(g.tags) : []);
    setCopied(source);
  };

  const submit = async () => {
    setBusy(true);
    setFailure(null);
    const t = bucketOf(name);
    let step = "CreateBucket";
    try {
      await callKindOp(KIND, "CreateBucket", input);
      void qc.invalidateQueries({ queryKey: ["resources", "list", "s3", "buckets"] });
      const kept = filled(tags);
      if (bpaOn(bpa) !== 4) await callOp(t, (step = "PutPublicAccessBlock"), { PublicAccessBlockConfiguration: bpa });
      if (versioning) await callOp(t, (step = "PutBucketVersioning"), { VersioningConfiguration: { Status: "Enabled" } });
      if (kept.length > 0) {
        step = "PutBucketTagging";
        await client.api(tagsPath(t), { method: "PUT", body: JSON.stringify({ set: pairsOf(kept), remove: [] }), headers: { "Content-Type": "application/json" } });
      }
      if (enc.algorithm !== "AES256") await callOp(t, (step = "PutBucketEncryption"), { ServerSideEncryptionConfiguration: encryptionConfig(enc.algorithm, enc.key, enc.bucketKey) });
    } catch (e) {
      if (step === "CreateBucket") {
        setFailure({ step, error: e });
        setBusy(false);
        return;
      }
      // The bucket is there; the setting after it was refused. Its own page says so.
      setFlash({ tone: "warn", title: `Bucket ${name} was created, but ${step} was refused.`, returned: errorWords(e).message, left: "Set it from the bucket's pages." });
      go({});
      return;
    }
    setFlash({ title: `Bucket ${name} created.`, returned: `CreateBucket answered with its location. ${madeWords({ versioning, bpa, lock })}.` });
    go({});
  };

  return (
    <FormPage
      title="Create bucket"
      sub="Name the bucket and choose its starting settings. The defaults keep ACLs off and public access blocked."
      crumbs={[{ label: "Resources", to: "/resources" }, { label: "Storage and data", to: "/resources" }, { label: "S3", to: "/resources/s3/buckets" }, { label: "Create bucket", to: "/resources/s3/buckets" }]}
      request={request}
      showInput
      refusal={refused && said ? { code: code || "Error", message: said.message } : null}
      callNote={<>Keeping all four Block Public Access settings on and Bucket owner enforced needs only the CreateBucket permission. Object Lock On adds ObjectLockEnabledForBucket to this call. Versioning, tags, Block Public Access changes and a KMS key are made by the calls that follow it.</>}
      banner={
        refused && said ? (
          <Alert tone="bad" title="Bucket not created." testId="create-refused">
            CreateBucket answered {said.code?.split(" · HTTP ").reverse().join(" ")}: {said.message.replace(/\.$/, "")}. {nameRefused ? `${nameRefused.why} ${nameRefused.suggestion ?? ""}`.replace(/\s+$/, "") : ""} Choose Create bucket again.
          </Alert>
        ) : undefined
      }
      submitLabel="Create bucket"
      pending={busy}
      disabled={!name || (lock && !acknowledged)}
      disabledWhy={!name ? "Give the bucket a name." : "Acknowledge the Object Lock warning."}
      onCancel={() => go({})}
      onSubmit={() => void submit()}
    >
      <FormSection title="General configuration">
        <div className="grid gap-1.5" data-testid="region-line">
          <span className="text-[14px] font-medium text-bright">AWS Region</span>
          <span className="text-[14px] text-foreground">{about.data ? regionWords(about.data.region) : "…"}</span>
          <p className="text-[13px] text-muted-foreground">A bucket&apos;s Region cannot be changed after you create it. The Region comes from the Region selector in the top bar.</p>
        </div>
        <Field
          label="Bucket name"
          refusal={nameRefused && code === "InvalidBucketName" ? { ...nameRefused, why: `${NAME_RULE} ${nameRefused.why}` } : nameRefused}
          help={<>Between <b>3</b> and <b>63</b> characters: lowercase letters, numbers, periods and hyphens, beginning and ending with a letter or number. The name must be unique in the aws partition (all commercial Regions), cannot be changed later, and is visible in object URLs, so keep sensitive information out of it.</>}
          hint={names.length > 0 ? `This account already has ${listWords(names.slice(0, 6))}${names.length > 6 ? ` and ${names.length - 6} more` : ""}.` : undefined}
        >
          {(id) => <Input id={id} value={name} onChange={(e) => (setName(e.target.value), setFailure(null))} placeholder="larder-exports" autoComplete="off" spellCheck={false} className="font-mono" data-testid="bucket-name" />}
        </Field>
        <Field label="Copy settings from existing bucket (optional)" hint={copied ? `Copied from ${copied}: its ownership, Block Public Access, versioning, encryption and tags. Check them below.` : "Console only. The bucket policy is not copied."}>
          {(id) => <ResourcePicker id={id} service="s3" kind="buckets" noun="bucket" by="name" value={from} onChange={(v) => void copyFrom(v)} />}
        </Field>
      </FormSection>
      <FormSection title="Object Ownership">
        <OwnershipChoice value={ownership} onChange={setOwnership} />
        <p className="text-[13px] text-muted-foreground">With Bucket owner enforced, ACLs are off here, as on AWS: a request that grants more than the owner's full control is refused (AccessControlListNotSupported). Keeping the default needs only the CreateBucket permission.</p>
      </FormSection>
      <FormSection title="Block Public Access settings for this bucket">
        <BpaChoice value={bpa} onChange={setBpa} />
        <p className="text-[13px] text-muted-foreground">By default all four settings are on, and S3 recommends leaving them on unless you know you need one off. Turning any off needs s3:PutBucketPublicAccessBlock.</p>
      </FormSection>
      <FormSection title="Bucket Versioning">
        <RadioCards name="create-versioning" value={versioning ? "on" : "off"} onChange={(v) => setVersioning(v === "on")} options={[{ value: "off", title: "Disable", description: "The default. The bucket keeps only the latest object for each key." }, { value: "on", title: "Enable", description: "Keep every version of every object, so you can recover from unintended overwrites and deletes." }]} />
        <p className="text-[13px] text-muted-foreground" data-testid="versioning-hint">{versioningHint(rows) ?? "Versioning is off by default. With it on, every new version of an object notifies the bucket's event notification rules."}</p>
      </FormSection>
      <FormSection title="Tags" optional description={`Tags on a bucket label it for cost allocation: a tag is a key-value pair you choose.${tagsHint(tagKeys) ? ` ${tagsHint(tagKeys)}` : ""}`}>
        <KeyValueEditor rows={tags} onChange={setTags} rules={TAG_RULES} addLabel="Add Tag" testId="create-tags" />
      </FormSection>
      <FormSection title="Default encryption">
        <EncryptionChoice value={enc} onChange={setEnc} />
        <LocalStatus state="stored" reason={`Objects are plain files under the data directory whatever you choose, and the setting is echoed back on each object.${firstKey ? ` ${firstKey} is a symmetric key in this Region, so it would appear in the KMS key list.` : ""}`} />
      </FormSection>
      <FormSection title="Advanced settings" collapsible defaultOpen summary={lock ? "Object Lock is on." : "Object Lock is off."}>
        <RadioCards name="lock" value={lock ? "on" : "off"} onChange={(v) => setLock(v === "on")} options={[{ value: "off", title: "Disable" }, { value: "on", title: "Enable", description: "Helps protect new objects from being deleted or overwritten. You must acknowledge the warning that appears." }]} />
        {lock && (
          <Alert tone="warn" title="Enabling Object Lock cannot be undone for this bucket." testId="lock-warning">
            <span className="grid gap-3">
              <span>Object Lock turns versioning on, and it stays on: S3 refuses to suspend versioning on a bucket that has Object Lock (InvalidBucketState).</span>
              <span className="flex items-start gap-3">
                <Checkbox id="lock-ack" checked={acknowledged} onCheckedChange={(v) => setAcknowledged(v === true)} className="mt-0.5" data-testid="lock-ack" />
                <Label htmlFor="lock-ack" className="font-normal leading-5">I acknowledge that enabling Object Lock will permanently allow objects in this bucket to be locked.</Label>
              </span>
            </span>
          </Alert>
        )}
        <p className="text-[13px] text-muted-foreground">Enabling Object Lock automatically enables versioning for the bucket. Once the bucket is created, its Properties tab shows Object Lock; a default retention is set with PutObjectLockConfiguration (the AWS CLI or an SDK), and a legal hold on an object with PutObjectLegalHold. Creating a locked bucket needs s3:CreateBucket, s3:PutBucketVersioning and s3:PutBucketObjectLockConfiguration.</p>
      </FormSection>
    </FormPage>
  );
}


