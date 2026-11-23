// The controls of a bucket's settings, shared by Create bucket and each setting's own edit page: a choice among cards, Object
// Ownership, Block Public Access, Bucket Versioning, default encryption.
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { cn } from "@doze/ui/lib/utils";
import { Combobox } from "@doze/ui/components/app/combobox";
import { Field } from "../../../kit";
import { useKmsKeys } from "./kms-hook";
import { kmsHint } from "./kms-keys";
import { BPA_ALL, BPA_KEYS, type Bpa, bpaOn, isSse, type Ownership, OWNERSHIP_WORDS, type Sse, SSE_WORDS } from "./config";

/** A choice among cards: each has a title and a sentence under it. */
export function RadioCards<T extends string>({ value, onChange, options, name, className }: { value: T; onChange: (v: T) => void; options: { value: T; title: React.ReactNode; description?: React.ReactNode; disabled?: boolean }[]; name: string; className?: string }) {
  return (
    <RadioGroup value={value} onValueChange={(v) => onChange(v as T)} className={cn("grid gap-2.5", className)} aria-label={name}>
      {options.map((o) => (
        <Label key={o.value} htmlFor={`${name}-${o.value}`} className={cn("flex cursor-pointer items-start gap-3 rounded-xl border bg-card p-4 font-normal leading-5 has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40", o.disabled && "cursor-not-allowed opacity-60")}>
          <RadioGroupItem id={`${name}-${o.value}`} value={o.value} disabled={o.disabled} className="mt-0.5" data-testid={`${name}-${o.value}`} />
          <span className="grid gap-0.5">
            <span className="font-medium text-bright">{o.title}</span>
            {o.description && <span className="text-[13px] text-muted-foreground">{o.description}</span>}
          </span>
        </Label>
      ))}
    </RadioGroup>
  );
}

export function OwnershipChoice({ value, onChange, edit = false }: { value: Ownership; onChange: (v: Ownership) => void; edit?: boolean }) {
  return (
    <RadioCards
      name="ownership"
      value={value}
      onChange={onChange}
      options={[
        { value: "BucketOwnerEnforced", title: `ACLs disabled: ${OWNERSHIP_WORDS.BucketOwnerEnforced}${edit ? "" : " (default)"}`, description: `ACLs are disabled, and the bucket owner automatically owns and has full control over every object${edit ? " in the bucket" : ""}. ACLs no longer affect ${edit ? "permissions to" : "access permissions to"} data in the bucket; the bucket uses ${edit ? "policies" : "policies exclusively"} to define access control.` },
        { value: "BucketOwnerPreferred", title: `ACLs enabled: ${OWNERSHIP_WORDS.BucketOwnerPreferred}`, description: "The bucket owner owns and has full control over new objects that other accounts write to the bucket with the bucket-owner-full-control canned ACL." },
        { value: "ObjectWriter", title: `ACLs enabled: ${OWNERSHIP_WORDS.ObjectWriter}`, description: "The AWS account that uploads an object owns the object, has full control over it, and can grant other users access to it through ACLs." },
      ]}
    />
  );
}

const BPA_WORDS: Record<(typeof BPA_KEYS)[number], [string, string]> = {
  BlockPublicAcls: ["PutBucketAcl and PutObjectAcl calls fail if the specified ACL is public, and PutObject calls fail if the request includes a public ACL.", " Existing policies and ACLs are not modified."],
  IgnorePublicAcls: ["S3 ignores all public ACLs on the bucket and any objects it contains.", " Existing ACLs are kept."],
  BlockPublicPolicy: ["S3 rejects calls to PutBucketPolicy if the specified bucket policy allows public access.", " Existing policies are not modified."],
  RestrictPublicBuckets: ["Restricts access to a bucket with a public policy to AWS service principals and authorized users within the bucket owner's account.", ""],
};

function Tick({ id, checked, onChange, title, description }: { id: string; checked: boolean | "indeterminate"; onChange: (on: boolean) => void; title: string; description: React.ReactNode }) {
  return (
    <div className="flex items-start gap-3">
      <Checkbox id={id} checked={checked} onCheckedChange={(v) => onChange(v === true)} className="mt-0.5" data-testid={id} />
      <div className="grid gap-0.5">
        <Label htmlFor={id} className="font-medium text-bright">{title}</Label>
        <p className="text-[13px] leading-5 text-muted-foreground">{description}</p>
      </div>
    </div>
  );
}

/** Block all public access and the four settings under it, each with its sentence. */
export function BpaChoice({ value, onChange, edit = false }: { value: Bpa; onChange: (b: Bpa) => void; edit?: boolean }) {
  const on = bpaOn(value);
  return (
    <div className="grid gap-4">
      <Tick id="bpa-all" checked={on === 4 ? true : on === 0 ? false : "indeterminate"} onChange={(v) => onChange(v ? BPA_ALL : { BlockPublicAcls: false, IgnorePublicAcls: false, BlockPublicPolicy: false, RestrictPublicBuckets: false })} title="Block all public access" description="Turns on all four settings below. The four settings are independent and can be used in any combination." />
      <div className="grid gap-4 border-l pl-6">
        {BPA_KEYS.map((k) => (
          <Tick key={k} id={`bpa-${k}`} checked={value[k]} onChange={(v) => onChange({ ...value, [k]: v })} title={k} description={BPA_WORDS[k][0] + (edit ? BPA_WORDS[k][1] : "")} />
        ))}
      </div>
    </div>
  );
}

export function VersioningChoice({ value, onChange, current }: { value: "Enabled" | "Suspended"; onChange: (v: "Enabled" | "Suspended") => void; current?: string }) {
  return (
    <RadioCards
      name="versioning"
      value={value}
      onChange={onChange}
      options={[
        { value: "Suspended", title: "Suspend", description: "Stop creating new versions. Existing versions are kept, and new objects get the version ID null." },
        { value: "Enabled", title: "Enable", description: `Every object written from now on gets a unique version ID.${current ? ` The bucket is ${current} today.` : ""}` },
      ]}
    />
  );
}

export type Encryption = { algorithm: Sse; keyMode: "list" | "arn"; key: string; bucketKey: boolean };

/** Default encryption: the type, and for KMS the key (from the keys in this Region, or an ARN typed) and the Bucket Key. */
export function EncryptionChoice({ value, onChange }: { value: Encryption; onChange: (e: Encryption) => void }) {
  const set = (patch: Partial<Encryption>) => onChange({ ...value, ...patch });
  const kms = useKmsKeys();
  return (
    <div className="grid gap-5">
      <RadioCards
        name="encryption"
        value={value.algorithm}
        onChange={(v) => isSse(v) && set({ algorithm: v, bucketKey: v === "aws:kms" ? value.bucketKey : false })}
        options={(["AES256", "aws:kms", "aws:kms:dsse"] as Sse[]).map((a) => ({ value: a, title: SSE_WORDS[a], description: a === "aws:kms" ? "Choosing it adds a KMS key and Bucket Key choice." : undefined }))}
      />
      {value.algorithm !== "AES256" && (
        <div className="grid gap-4 rounded-xl border bg-well p-4" data-testid="kms-choice">
          <div className="grid gap-1.5">
            <Label>AWS KMS key</Label>
            <RadioGroup value={value.keyMode} onValueChange={(v) => set({ keyMode: v as "list" | "arn" })} className="flex flex-wrap gap-6" aria-label="AWS KMS key source">
              {[["list", "Choose from your AWS KMS keys"], ["arn", "Enter AWS KMS key ARN"]].map(([v, t]) => (
                <Label key={v} className="flex items-center gap-2 font-normal">
                  <RadioGroupItem value={v!} data-testid={`kms-${v}`} /> {t}
                </Label>
              ))}
            </RadioGroup>
          </div>
          <Field label="KMS key" hint={value.keyMode === "list" && !kms.pending && !kms.failed ? kmsHint(kms.keys) : undefined}>
            {(id) => (value.keyMode === "list" ? <Combobox id={id} options={kms.keys.usable} value={value.key || null} onChange={(k) => set({ key: k ?? "" })} placeholder={kms.failed ? "The keys could not be read" : kms.pending ? "Reading keys…" : kms.keys.usable.length === 0 ? "There is no key S3 can use yet" : "Choose a key"} search="Search keys…" empty="No key matches." aria-label="Choose a KMS key" className="w-full" /> : <Input id={id} value={value.key} onChange={(e) => set({ key: e.target.value })} placeholder="arn:aws:kms:us-east-1:111122223333:key/…" className="font-mono" spellCheck={false} data-testid="kms-arn-input" />)}
          </Field>
          {value.algorithm === "aws:kms" && (
            <div className="grid gap-1.5">
              <Label>Bucket Key</Label>
              <RadioGroup value={value.bucketKey ? "on" : "off"} onValueChange={(v) => set({ bucketKey: v === "on" })} className="flex gap-6" aria-label="Bucket Key">
                {[["off", "Disable"], ["on", "Enable"]].map(([v, t]) => (
                  <Label key={v} className="flex items-center gap-2 font-normal">
                    <RadioGroupItem value={v!} data-testid={`bucket-key-${v}`} /> {t}
                  </Label>
                ))}
              </RadioGroup>
              <p className="text-[13px] text-muted-foreground">A Bucket Key lowers the cost of encryption by reducing request traffic from S3 to AWS KMS. It is not supported for DSSE-KMS.</p>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
