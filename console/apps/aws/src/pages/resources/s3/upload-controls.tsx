// The choices of an upload's Properties that are more than a field: the storage class (seven radios in two columns), the encryption of
// the objects (a tick to specify a key, then bucket default or an override), and the checksum (off, or on with a function).
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Field, LocalStatus } from "../../../kit";
import { Input } from "@doze/ui/components/ui/input";
import { CHECKSUMS, STORAGE_CLASSES } from "./upload-props";
import { storageWords } from "./words";

const Radio = ({ value, id, title, description, disabled }: { value: string; id: string; title: React.ReactNode; description?: React.ReactNode; disabled?: boolean }) => (
  <Label htmlFor={id} className={`flex items-start gap-3 font-normal leading-5 ${disabled ? "opacity-60" : "cursor-pointer"}`}>
    <RadioGroupItem id={id} value={value} disabled={disabled} className="mt-0.5" data-testid={id} />
    <span className="grid gap-0.5">
      <span className="text-bright">{title}</span>
      {description && <span className="text-[13px] text-muted-foreground">{description}</span>}
    </span>
  </Label>
);

export function StorageClassChoice({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <div className="grid gap-2">
      <span className="text-[14px] font-medium text-bright">Storage class</span>
      <RadioGroup value={value} onValueChange={onChange} className="grid grid-cols-1 gap-x-8 gap-y-3 sm:grid-cols-2" aria-label="Storage class">
        {STORAGE_CLASSES.map((c) => <Radio key={c} value={c} id={`storage-${c}`} title={`S3 ${storageWords(c)}`} />)}
      </RadioGroup>
      <LocalStatus state="stored" reason="Every object is a plain file whatever class you name; the class is kept on the object and listed, and no lifecycle transition moves it." />
    </div>
  );
}

export type UploadSse = { specify: boolean; override: boolean; sse: "AES256" | "aws:kms"; kmsKey: string };
export const NO_SSE: UploadSse = { specify: false, override: false, sse: "AES256", kmsKey: "" };
/** What the choice sends: nothing set (the bucket's default encryption) unless a key is specified and the bucket settings overridden. */
export const sseOf = (e: UploadSse): "bucket" | "AES256" | "aws:kms" => (e.specify && e.override ? e.sse : "bucket");

export function EncryptionChoice({ value, onChange, noun = "these files", heading = true, help }: { value: UploadSse; onChange: (v: UploadSse) => void; noun?: string; /** The title above the choice: left out where a section already has one. */ heading?: boolean; help?: React.ReactNode }) {
  const set = (p: Partial<UploadSse>) => onChange({ ...value, ...p });
  return (
    <div className="grid gap-3">
      {heading && <span className="text-[14px] font-medium text-bright">Server-side encryption settings</span>}
      <div className="flex items-start gap-3">
        <Checkbox id="specify-key" checked={value.specify} onCheckedChange={(v) => set({ specify: v === true })} className="mt-0.5" data-testid="specify-key" />
        <div className="grid gap-0.5">
          <Label htmlFor="specify-key" className="font-medium text-bright">Specify an encryption key</Label>
          <p className="text-[13px] leading-5 text-muted-foreground">{help ?? "Then choose Use bucket settings for default encryption, or Override bucket settings for default encryption."}</p>
        </div>
      </div>
      <RadioGroup value={value.override ? "override" : "bucket"} onValueChange={(v) => set({ override: v === "override" })} className="grid gap-3" aria-label="Encryption of the objects">
        <Radio value="bucket" id="sse-bucket" title="Use bucket settings for default encryption" disabled={!value.specify} />
        <Radio value="override" id="sse-override" title="Override bucket settings for default encryption" description={`Choose Amazon S3 managed key (SSE-S3) or AWS Key Management Service key (SSE-KMS) for ${noun} only.`} disabled={!value.specify} />
      </RadioGroup>
      {value.specify && value.override && (
        <div className="grid gap-3 rounded-xl border bg-well p-4" data-testid="sse-override-choice">
          <RadioGroup value={value.sse} onValueChange={(v) => set({ sse: v as UploadSse["sse"] })} className="grid gap-3" aria-label="Key to use">
            <Radio value="AES256" id="sse-s3" title="Amazon S3 managed key (SSE-S3)" />
            <Radio value="aws:kms" id="sse-kms" title="AWS Key Management Service key (SSE-KMS)" />
          </RadioGroup>
          {value.sse === "aws:kms" && <Field label="KMS key">{(id) => <Input id={id} value={value.kmsKey} onChange={(e) => set({ kmsKey: e.target.value })} placeholder="alias/larder-app or a key ARN" className="font-mono" data-testid="kms-key" />}</Field>}
        </div>
      )}
      <LocalStatus state="stored" reason={noun === "these files" ? "The encryption setting is echoed back on the object; the bytes are plain files either way." : "Objects are plain files under the data directory whatever key you name; the setting is echoed back on the object."} />
    </div>
  );
}

export function ChecksumChoice({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <div className="grid gap-3">
      <span className="text-[14px] font-medium text-bright">Additional checksums</span>
      <RadioGroup value={value ? "on" : "off"} onValueChange={(v) => onChange(v === "on" ? "CRC32" : "")} className="grid gap-3" aria-label="Additional checksums">
        <Radio value="off" id="checksum-off" title="Off" />
        <Radio value="on" id="checksum-on" title="On" description="Choose a checksum function. S3 calculates and stores it after receiving the whole object, and refuses the upload if a value you precalculated does not match." />
      </RadioGroup>
      {value && (
        <select aria-label="Checksum function" value={value} onChange={(e) => onChange(e.target.value)} className="h-9 w-full max-w-sm rounded-md border bg-card px-3 text-[14px]" data-testid="checksum">
          {CHECKSUMS.map((c) => <option key={c} value={c}>{c}</option>)}
        </select>
      )}
    </div>
  );
}
