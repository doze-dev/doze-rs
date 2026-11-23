// The small edits of a secret, each a dialog over its page with the call it makes beside the fields: the description
// (UpdateSecret), the encryption key (UpdateSecret), the resource permissions (PutResourcePolicy, DeleteResourcePolicy) and the
// deletion (DeleteSecret, with its waiting period).
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { callKindOp, DeletionWindow, Field, PolicyEditor, SectionEdit, windowDays } from "../../../kit";
import { Callout } from "@doze/ui/components/app/callout";
import { KeyAliasPicker } from "../key-picker";
import { SECRETS } from "./api";

/** After a write: the page's reads of the secret are read again. */
function useReread() {
  const qc = useQueryClient();
  return () => Promise.all([qc.invalidateQueries({ queryKey: ["resources", "kindop", "secretsmanager"] }), qc.invalidateQueries({ queryKey: ["resources", "one", "secretsmanager", "secrets"] }), qc.invalidateQueries({ queryKey: ["resources", "list", "secretsmanager"] })]);
}

type Edit<T = unknown> = { open: boolean; onClose: () => void; name: string; /** What the dialog was saved as, for the page to say. */ onSaved?: (answer: T) => void };

function useSave<T>(call: () => Promise<T>, o: Edit<T>) {
  const reread = useReread();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const save = async () => {
    setPending(true);
    setError(null);
    try {
      const answer = await call();
      await reread();
      o.onSaved?.(answer);
      o.onClose();
    } catch (e) {
      setError(e);
    } finally {
      setPending(false);
    }
  };
  return { pending, error, save, clear: () => setError(null) };
}

export function EditDescription({ open, onClose, name, current, onSaved }: Edit & { current: string }) {
  const [text, setText] = useState<string | null>(null);
  const value = text ?? current;
  const s = useSave(() => callKindOp(SECRETS, "UpdateSecret", { SecretId: name, Description: value }), { open, onClose, name, onSaved });
  return (
    <SectionEdit open={open} onOpenChange={(o) => { if (!o) { onClose(); setText(null); s.clear(); } }} title="Edit description" description={`The description of ${name}. It is shown in the list and may appear in logs: keep secrets out of it.`} request={{ service: "secretsmanager", operation: "UpdateSecret", input: { SecretId: name, Description: value } }} onSave={() => void s.save()} pending={s.pending} error={s.error} disabled={value.length > 2048} disabledWhy="A description is at most 2,048 characters." testId="edit-description">
      <Field label="Description" help="Up to 2,048 characters. Empty it to remove the description.">
        {(id) => <Textarea id={id} value={value} onChange={(e) => setText(e.target.value)} rows={3} className="bg-card" data-testid="description-input" />}
      </Field>
    </SectionEdit>
  );
}

export function EditKey({ open, onClose, name, current, onSaved }: Edit & { current: string | null }) {
  const [key, setKey] = useState<string | null | undefined>(undefined);
  const value = key === undefined ? current : key;
  const input = { SecretId: name, KmsKeyId: value ?? "alias/aws/secretsmanager" };
  const s = useSave(() => callKindOp(SECRETS, "UpdateSecret", input), { open, onClose, name, onSaved });
  return (
    <SectionEdit open={open} onOpenChange={(o) => { if (!o) { onClose(); setKey(undefined); s.clear(); } }} title="Edit encryption key" description={`The KMS key that encrypts the value of ${name}. A new version is encrypted with the new key; existing versions keep the key they were made with.`} request={{ service: "secretsmanager", operation: "UpdateSecret", input }} onSave={() => void s.save()} pending={s.pending} error={s.error} testId="edit-key">
      <Field label="Encryption key" help="Without a choice, Secrets Manager uses aws/secretsmanager.">
        {(id) => <KeyAliasPicker id={id} value={value} onChange={setKey} defaultLabel="aws/secretsmanager" />}
      </Field>
    </SectionEdit>
  );
}

export function EditPolicy({ open, onClose, name, current, onSaved }: Edit<"saved" | "removed"> & { current: string | null }) {
  const [text, setText] = useState<string | null>(null);
  const value = text ?? (current ? pretty(current) : "");
  const remove = value.trim() === "";
  const s = useSave<"saved" | "removed">(async () => { await callKindOp(SECRETS, remove ? "DeleteResourcePolicy" : "PutResourcePolicy", remove ? { SecretId: name } : { SecretId: name, ResourcePolicy: value }); return remove ? "removed" : "saved"; }, { open, onClose, name, onSaved });
  return (
    <SectionEdit wide open={open} onOpenChange={(o) => { if (!o) { onClose(); setText(null); s.clear(); } }} title="Resource permissions" description={`Edit the policy attached to ${name}. Clear the field and save to delete the policy.`} request={{ service: "secretsmanager", operation: remove ? "DeleteResourcePolicy" : "PutResourcePolicy", input: remove ? { SecretId: name } : { SecretId: name, ResourcePolicy: "{ ... }" } }} onSave={() => void s.save()} saveLabel="Save" pending={s.pending} error={s.error} testId="edit-policy">
      <PolicyEditor kind="resource" value={value} onChange={setText} label="Resource policy" rows={14} maxSize={20480} help="Up to 20,480 characters. doze-aws evaluates this policy on every call that names the secret, in IAM soft and enforce modes." testId="policy-editor" />
    </SectionEdit>
  );
}

const pretty = (s: string): string => { try { return JSON.stringify(JSON.parse(s), null, 2); } catch { return s; } };

export function DeleteSecretDialog({ open, onClose, name, readers, onSaved }: Edit<{ DeletionDate?: number }> & { readers: string[] }) {
  const [days, setDays] = useState("30");
  const [now] = useState(() => Date.now());
  const n = windowDays(days);
  const s = useSave(() => callKindOp<{ DeletionDate?: number }>(SECRETS, "DeleteSecret", { SecretId: name, RecoveryWindowInDays: n ?? 30 }), { open, onClose, name, onSaved });
  return (
    <SectionEdit open={open} onOpenChange={(o) => { if (!o) { onClose(); s.clear(); } }} title="Disable secret and schedule deletion" description={`Secrets Manager does not delete ${name} right away. It makes the secret inaccessible now and deletes it for good when the waiting period ends. Until then you can cancel the deletion.`} request={{ service: "secretsmanager", operation: "DeleteSecret", input: { SecretId: name, RecoveryWindowInDays: n ?? 30 } }} saveLabel="Schedule deletion" onSave={() => void s.save()} pending={s.pending} error={s.error} disabled={n === null} disabledWhy="Enter a whole number of days from 7 to 30." testId="delete-secret">
      <DeletionWindow mode="choose" noun="secret" days={days} onDays={setDays} now={now} />
      {readers.length > 0 && <Callout tone="warn" testId="readers-warning">{readers.join(", ")} read{readers.length === 1 ? "s" : ""} this secret. Once the deletion is scheduled their GetSecretValue calls fail until you cancel it.</Callout>}
    </SectionEdit>
  );
}
