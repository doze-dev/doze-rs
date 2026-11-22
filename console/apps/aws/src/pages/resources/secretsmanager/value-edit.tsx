// Edit secret value (secretsmanager__secret-value-edit): the key and value rows (or plaintext) of the current version, and the
// PutSecretValue call that saves them as a new version labeled AWSCURRENT.
import { useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { newRow, rowsFrom, rowsValid, type KeyValueRow } from "@doze/ui/components/app/key-values";
import { Callout } from "@doze/ui/components/app/callout";
import { Textarea } from "@doze/ui/components/ui/textarea";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { callKindOp, Field, FormPage, FormSection, resourceCrumbs, Segmented, useKindOpRead } from "../../../kit";
import { Note } from "../cfg-parts";
import { refusalOf } from "../kms/schedule";
import { SECRETS, type SecretValue, useVersions } from "./api";
import { rowsValue, savingWords, valueProblem, valueRows, versions } from "./model";

const RULES = { caseSensitive: true, maxKey: 256, emptyValue: true };

export function ValueEdit({ d, back }: { d: ResourceDetail; back: (done?: string) => void }) {
  const current = useKindOpRead<SecretValue>(SECRETS, "GetSecretValue", { SecretId: d.name });
  if (current.data) return <ValueForm d={d} back={back} initial={current.data.SecretString ?? ""} />;
  return <p className="text-[13.5px] text-muted-foreground" data-testid="value-reading">Reading the current value…</p>;
}

function ValueForm({ d, back, initial }: { d: ResourceDetail; back: (done?: string) => void; initial: string }) {
  const qc = useQueryClient();
  const name = d.name;
  const vs = useVersions(name);
  const start = useMemo(() => valueRows(initial), [initial]);
  const [mode, setMode] = useState<"pairs" | "text">(start ? "pairs" : "text");
  const [rows, setRows] = useState<KeyValueRow[]>(start ? rowsFrom(start) : [newRow()]);
  const [text, setText] = useState(initial);
  const [tried, setTried] = useState(false);
  const [pending, setPending] = useState(false);
  const [refused, setRefused] = useState<{ code: string; message: string } | null>(null);
  const token = useMemo(() => crypto.randomUUID(), []);
  const value = mode === "pairs" ? rowsValue(rows) : text;
  const problem = mode === "pairs" ? (rowsValid(rows, RULES) ? valueProblem(rows.some((r) => r.key !== "") ? value : "") : "Fix the rows marked.") : valueProblem(text);
  const save = async () => {
    setTried(true);
    if (problem) return;
    setPending(true);
    setRefused(null);
    try {
      await callKindOp(SECRETS, "PutSecretValue", { SecretId: name, ClientRequestToken: token, SecretString: value });
      await Promise.all([qc.invalidateQueries({ queryKey: ["resources", "kindop", "secretsmanager"] }), qc.invalidateQueries({ queryKey: ["resources", "one", "secretsmanager", "secrets"] })]);
      back("value");
    } catch (e) {
      setRefused(refusalOf(e));
    } finally {
      setPending(false);
    }
  };
  const rotating = d.fields.find((f) => f.label === "Rotation function")?.value;
  return (
    <FormPage
      title="Edit secret value"
      sub={<>Saving stores a new version; nothing is overwritten. You are editing <span className="font-mono text-bright">{name}</span>.</>}
      crumbs={resourceCrumbs(d, { label: "Edit secret value", to: "" })}
      request={{ service: "secretsmanager", operation: "PutSecretValue", input: { SecretId: name, ClientRequestToken: token, SecretString: "(masked)" } }}
      callNote="No VersionStages, so the label AWSCURRENT moves to the new version. The token makes a retry safe: the same token with the same value does nothing."
      showInput
      shown={(_, input) => ({ ...(input as object), ClientRequestToken: "(a new UUID)" })}
      refusal={refused}
      submitLabel="Save"
      pending={pending}
      disabled={tried && problem !== null}
      disabledWhy={problem}
      onSubmit={() => void save()}
      onCancel={() => back()}
      banner={refused ? <Alert tone="bad" title={`The value was not saved: ${refused.code}`} testId="value-refused">{refused.message}</Alert> : undefined}
      testId="secret-value-edit"
    >
      {rotating && <Callout tone="info" lead="Rotation is the recommended way to change a credential." testId="rotation-hint">It updates the secret and the service together, so clients keep getting a working set. This secret has a rotation function, <span className="font-mono">{rotating.split(":").pop()}</span>.</Callout>}
      <FormSection title="Secret value" description={mode === "pairs" ? "Edit the key/value pairs." : "Edit the plaintext."} actions={<Segmented label="Format" value={mode} onChange={(m) => { setMode(m); if (m === "text" && text === "") setText(rowsValue(rows)); }} options={[{ value: "pairs", label: "Key/value" }, { value: "text", label: "Plaintext" }]} testId="value-mode" />} testId="section-value">
        {mode === "pairs" ? (
          <KeyValueEditor rows={rows} onChange={setRows} rules={RULES} showErrors={tried} keyLabel="Key" valueLabel="Value" addLabel="Add row" removeLabel="Remove" testId="value-rows" />
        ) : (
          <Field label="Plaintext" error={tried ? (problem ?? undefined) : undefined}>{(id) => <Textarea id={id} value={text} onChange={(e) => setText(e.target.value)} rows={8} spellCheck={false} className="bg-card font-mono text-[13px]" data-testid="value-text" />}</Field>
        )}
        <Note testId="value-note">Up to 65,536 bytes. Saving creates a new version labeled AWSCURRENT; the version that held it becomes AWSPREVIOUS. {vs.data ? savingWords(versions(vs.data.Versions)) : ""}</Note>
      </FormSection>
      <Note testId="value-frequency">Each save creates a version, and a secret keeps only a few unlabeled ones, so avoid saving more often than you need to.</Note>
    </FormPage>
  );
}
