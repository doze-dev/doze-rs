// "Additional copy settings > Specify settings" of Copy, Move and Rename: the board's six (storage class, ACLs, object tags, metadata,
// server-side encryption, additional checksums). Each starts as the source has it, so choosing to specify changes only what is
// changed. What doze-aws applies and what it only keeps is said under each (`LocalStatus` where it is kept).
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import type { KeyValueRow } from "@doze/ui/components/app/key-values";
import { Field, TAG_RULES } from "../../../kit";
import { aclChoices } from "./copy-props";
import { RadioCards } from "./controls";
import { ChecksumChoice, EncryptionChoice, StorageClassChoice, type UploadSse } from "./upload-controls";

/** What the person has set; the source's own values stand in for what they have not. */
export type SpecifyState = { metadata: "copy" | "replace"; metaRows: KeyValueRow[]; storageClass: string; acl: string; tags: KeyValueRow[]; sse: UploadSse; checksum: string };

export function SpecifySettings({ state, set, enforced, blockPublicAcls }: { state: SpecifyState; set: (patch: Partial<SpecifyState>) => void; /** The destination's Object Ownership is Bucket owner enforced: its ACLs are disabled. */ enforced: boolean; blockPublicAcls: boolean }) {
  const acls = aclChoices(enforced, blockPublicAcls);
  return (
    <div className="mt-2 grid gap-7 border-t pt-6" data-testid="copy-specify">
      <StorageClassChoice value={state.storageClass} onChange={(storageClass) => set({ storageClass })} />
      <Field label="Access control list (ACL)" help={enforced ? "The destination has ACLs disabled (Bucket owner enforced), so it takes only the bucket owner full control ACL; change Object Ownership on its Permissions tab to use another." : "A copy does not keep the source's ACL. With none chosen it gets the destination's default: the owner's full control."} hint={!enforced && blockPublicAcls ? "The destination blocks public ACLs, so the ones that grant access to anyone or to every AWS account are not offered." : undefined}>
        {(id) => (
          <select id={id} value={state.acl} onChange={(e) => set({ acl: e.target.value })} className="h-9 w-full max-w-md rounded-md border bg-card px-3 text-[14px]" data-testid="copy-acl">
            <option value="">None: the destination's default</option>
            {acls.map((a) => <option key={a.value} value={a.value} disabled={Boolean(a.why)} title={a.why}>{a.title}</option>)}
          </select>
        )}
      </Field>
      <Field label="Object tags" help="Each tag is a key-value pair. The tags start as the source's; the copy has exactly the ones listed here.">
        <KeyValueEditor rows={state.tags} onChange={(tags) => set({ tags })} rules={{ ...TAG_RULES, maxRows: 10 }} addLabel="Add tag" testId="copy-tags" />
      </Field>
      <div className="grid gap-3">
        <span className="text-[14px] font-medium text-bright">Metadata</span>
        <RadioCards name="copy-metadata" value={state.metadata} onChange={(metadata) => set({ metadata })} options={[{ value: "copy", title: "Copy the source's metadata", description: "The default." }, { value: "replace", title: "Replace with the metadata below", description: "Starts as the source's. System defined (Content-Type, Cache-Control, Content-Disposition, Content-Encoding, Content-Language) or user defined (a key that starts with x-amz-meta-, or any other key, stored as user metadata)." }]} />
        {state.metadata === "replace" && <KeyValueEditor rows={state.metaRows} onChange={(metaRows) => set({ metaRows })} addLabel="Add metadata" testId="copy-meta" />}
      </div>
      <EncryptionChoice value={state.sse} onChange={(sse) => set({ sse })} noun="the copy" help="Then choose Use bucket settings for default encryption (the destination's), or Override bucket settings for default encryption." />
      <ChecksumChoice value={state.checksum} onChange={(checksum) => set({ checksum })} />
    </div>
  );
}
