// The edit pages of Properties and Permissions that choose among a few things: Bucket Versioning, Object Ownership, Block Public
// Access, default encryption, static website hosting. Each reads what the bucket has, and sends the whole setting back.
import { useState } from "react";
import { Input } from "@doze/ui/components/ui/input";
import { Field, FormSection, LocalStatus } from "../../../kit";
import type { OwnProps } from "../deep/registry";
import { NONE, useSetting } from "./api";
import { BpaChoice, type Encryption, EncryptionChoice, OwnershipChoice, RadioCards, VersioningChoice } from "./controls";
import { Loaded, SettingForm } from "./edit";
import { BPA_ALL, bpaOf, encryptionConfig, encryptionOf, ownershipOf, type Ownership, websiteConfig, websiteOf, SSE_SHORT } from "./config";
import { firstEnableWords } from "./words";

export function VersioningEdit({ name }: OwnProps) {
  const q = useSetting<{ Status?: string }>(name!, "GetBucketVersioning", []);
  return (
    <Loaded q={q} what="Versioning">
      {(v) => <VersioningForm name={name!} current={v?.Status} />}
    </Loaded>
  );
}
function VersioningForm({ name, current }: { name: string; current?: string }) {
  const [status, setStatus] = useState<"Enabled" | "Suspended">(current === "Suspended" ? "Suspended" : "Enabled");
  return (
    <SettingForm name={name} tab="properties" title="Edit Bucket Versioning" sub={`Versioning keeps every version of every object in ${name}.`} operation="PutBucketVersioning" input={{ VersioningConfiguration: { Status: status } }} saved={`Bucket Versioning is now ${status.toLowerCase()}.`} disabled={status === current} disabledWhy={`Versioning is already ${current?.toLowerCase()}.`}>
      <FormSection title="Bucket Versioning">
        <VersioningChoice value={status} onChange={setStatus} current={current ?? "Disabled"} />
        <p className="text-[13px] text-muted-foreground" data-testid="versioning-delay">{firstEnableWords(name, current)}</p>
      </FormSection>
    </SettingForm>
  );
}

export function OwnershipEdit({ name }: OwnProps) {
  const q = useSetting(name!, "GetBucketOwnershipControls", NONE.ownership);
  return <Loaded q={q} what="Object Ownership">{(o) => <OwnershipForm name={name!} current={ownershipOf(o) ?? "ObjectWriter"} />}</Loaded>;
}
function OwnershipForm({ name, current }: { name: string; current: Ownership }) {
  const [v, setV] = useState<Ownership>(current);
  return (
    <SettingForm name={name} tab="permissions" title="Edit Object Ownership" sub={`Choose who owns the objects written to ${name} and whether ACLs apply.`} operation="PutBucketOwnershipControls" input={{ OwnershipControls: { Rules: [{ ObjectOwnership: v }] } }} saved={`Object Ownership is now ${v}.`} submitLabel="Save">
      <FormSection title="Object Ownership">
        <OwnershipChoice value={v} onChange={setV} edit />
        <p className="text-[13px] text-muted-foreground">Before you apply Bucket owner enforced, move any ACL permissions into bucket policies and reset the bucket ACL to the default private ACL. Changing the setting needs s3:PutBucketOwnershipControls.</p>
        <p className="text-[13px] text-muted-foreground">Ownership is applied here: with Bucket owner enforced, ACLs are off and a request that grants more than the owner&apos;s full control is refused (AccessControlListNotSupported); with the other two, ACLs are kept and an AllUsers grant makes the bucket or object readable by anyone, unless Block Public Access ignores public ACLs.</p>
      </FormSection>
    </SettingForm>
  );
}

export function BpaEdit({ name }: OwnProps) {
  const q = useSetting(name!, "GetPublicAccessBlock", NONE.bpa);
  return <Loaded q={q} what="Block public access">{(b) => <BpaForm name={name!} start={b ? bpaOf(b) : { BlockPublicAcls: false, IgnorePublicAcls: false, BlockPublicPolicy: false, RestrictPublicBuckets: false }} />}</Loaded>;
}
function BpaForm({ name, start }: { name: string; start: typeof BPA_ALL }) {
  const [b, setB] = useState(start);
  return (
    <SettingForm name={name} tab="permissions" title="Edit Block public access (bucket settings)" sub={`Four independent switches decide whether ${name} can ever be public.`} operation="PutPublicAccessBlock" input={{ PublicAccessBlockConfiguration: b }} saved="Block public access settings saved.">
      <FormSection title="Block public access (bucket settings)">
        <BpaChoice value={b} onChange={setB} edit />
        <p className="text-[13px] text-muted-foreground">All four are on by default for a new bucket. All four settings are applied here: a public bucket policy is refused while BlockPublicPolicy is on, a public ACL while BlockPublicAcls is on, and public ACLs and public policies are ignored for anonymous requests under IgnorePublicAcls and RestrictPublicBuckets.</p>
      </FormSection>
    </SettingForm>
  );
}

export function EncryptionEdit({ name }: OwnProps) {
  const q = useSetting(name!, "GetBucketEncryption", NONE.encryption);
  return <Loaded q={q} what="Default encryption">{(e) => { const c = encryptionOf(e); return <EncryptionForm name={name!} start={{ algorithm: c?.algorithm ?? "AES256", keyMode: c?.key ? "arn" : "list", key: c?.key ?? "", bucketKey: c?.bucketKey ?? false }} />; }}</Loaded>;
}
function EncryptionForm({ name, start }: { name: string; start: Encryption }) {
  const [e, setE] = useState(start);
  const noKey = e.algorithm !== "AES256" && !e.key;
  return (
    <SettingForm name={name} callBelow tab="properties" title="Edit default encryption" sub={`New objects in ${name} are encrypted with the type you choose.`} operation="PutBucketEncryption" input={{ ServerSideEncryptionConfiguration: encryptionConfig(e.algorithm, e.key, e.bucketKey) }} saved={`Default encryption is now ${SSE_SHORT[e.algorithm]}.`} disabled={noKey} disabledWhy="Choose or enter the AWS KMS key." note="Amazon S3 does not validate the KMS key ID provided in PutBucketEncryption requests, so check that it is correct.">
      <FormSection title="Encryption type">
        <EncryptionChoice value={e} onChange={setE} />
        <p className="text-[13px] text-muted-foreground">Buckets and new objects are encrypted with SSE-S3 as the base level, at no cost. Choosing SSE-KMS or DSSE-KMS puts you under the requests-per-second quota of AWS KMS. Objects already in the bucket keep the encryption they have.</p>
        <LocalStatus state="stored" reason="Objects are plain files under the data directory whatever you choose; the setting is echoed back on each object." />
      </FormSection>
    </SettingForm>
  );
}

export function WebsiteEdit({ name }: OwnProps) {
  const q = useSetting(name!, "GetBucketWebsite", NONE.website);
  return <Loaded q={q} what="Static website hosting">{(w) => <WebsiteForm name={name!} start={websiteOf(w)} />}</Loaded>;
}
function WebsiteForm({ name, start }: { name: string; start: ReturnType<typeof websiteOf> }) {
  const [on, setOn] = useState(start !== null);
  const [mode, setMode] = useState<"host" | "redirect">(start && !start.index ? "redirect" : "host");
  const [index, setIndex] = useState(start?.index ?? "index.html");
  const [error, setError] = useState(start?.error ?? "");
  const [host, setHost] = useState(start?.redirectHost ?? "");
  const [protocol, setProtocol] = useState(start?.redirectProtocol ?? "");
  const bad = on && (mode === "host" ? !index || index.includes("/") : !host);
  const input = on ? { WebsiteConfiguration: websiteConfig({ mode, index, error, host, protocol }) } : null;
  return (
    <SettingForm name={name} callBelow tab="properties" title="Edit static website hosting" sub={`Serve ${name} as a website. Visitors get the index document at the root and in every folder.`} operation={on ? "PutBucketWebsite" : "DeleteBucketWebsite"} input={on ? (bad ? null : input) : {}} saved={on ? "Static website hosting is on." : "Static website hosting is off."} disabled={bad} disabledWhy={mode === "host" ? "The index document cannot be empty or contain a slash." : "Name the host to redirect to."}
      note="Turning on hosting is not enough on AWS: to make the site public you must also turn off Block Public Access and add a bucket policy that grants everyone s3:GetObject. doze-aws refuses a public bucket policy while BlockPublicPolicy is on.">
      <FormSection title="Static website hosting">
        <RadioCards name="website-on" value={on ? "on" : "off"} onChange={(v) => setOn(v === "on")} options={[{ value: "off", title: "Disable" }, { value: "on", title: "Enable" }]} />
      </FormSection>
      {on && (
        <FormSection title="Hosting type">
          <RadioCards name="website-mode" value={mode} onChange={setMode} options={[{ value: "host", title: "Use this bucket to host a website", description: "Serve an index document and an optional error document." }, { value: "redirect", title: "Redirect requests for an object", description: "Answer every request with a redirect to another host." }]} />
          {mode === "host" ? (
            <>
              <Field label="Index document" help="S3 returns this file for requests to the root and to any folder. The name is case sensitive, must match the file you upload exactly, and cannot be empty or contain a slash." error={index.includes("/") ? "The index document cannot contain a slash." : undefined}>
                {(id) => <Input id={id} value={index} onChange={(e) => setIndex(e.target.value)} className="font-mono" data-testid="website-index" />}
              </Field>
              <Field label="Error document (optional)" help="S3 returns this file for 4XX errors; without one it returns its default error page. The name is case sensitive.">
                {(id) => <Input id={id} value={error} onChange={(e) => setError(e.target.value)} placeholder="error.html" className="font-mono" data-testid="website-error" />}
              </Field>
              <LocalStatus state="inactive" reason="Redirection rules are kept if you put them with the AWS CLI, but the local website endpoint does not apply them: it serves the index and error documents, folder redirects and whole-site redirects." />
            </>
          ) : (
            <>
              <Field label="Host name">{(id) => <Input id={id} value={host} onChange={(e) => setHost(e.target.value)} placeholder="example.com" className="font-mono" data-testid="website-host" />}</Field>
              <Field label="Protocol (optional)">{(id) => <Input id={id} value={protocol} onChange={(e) => setProtocol(e.target.value)} placeholder="https" className="font-mono" />}</Field>
            </>
          )}
        </FormSection>
      )}
    </SettingForm>
  );
}
