// Automatic key rotation (kms__key-rotation-edit): Enable with a period of 90 to 2,560 days, or Disable. EnableKeyRotation or
// DisableKeyRotation is made on Save.
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Input } from "@doze/ui/components/ui/input";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { callKindOp, Field, FormPage, FormSection, resourceCrumbs } from "../../../kit";
import { Note, RowChoice } from "../cfg-parts";
import { KEYS, type RotationStatus } from "./api";
import { stateWords } from "./model";
import { StateDot } from "./parts";
import { refusalOf } from "./schedule";

/** The period as a whole number of days from 90 to 2,560; `null` for none typed, `undefined` for what is not a period. */
export function periodDays(text: string): number | null | undefined {
  if (text.trim() === "") return null;
  if (!/^\d+$/.test(text.trim())) return undefined;
  const n = Number(text);
  return n >= 90 && n <= 2560 ? n : undefined;
}

export function RotationEdit({ d, id, state, current, back }: { d: ResourceDetail; id: string; state: string; current: RotationStatus | null; back: () => void }) {
  const qc = useQueryClient();
  const [on, setOn] = useState<"enable" | "disable">("enable");
  const [days, setDays] = useState(current?.RotationPeriodInDays ? String(current.RotationPeriodInDays) : "");
  const [tried, setTried] = useState(false);
  const [pending, setPending] = useState(false);
  const [refused, setRefused] = useState<{ code: string; message: string } | null>(null);
  const n = periodDays(days);
  const bad = on === "enable" && n === undefined;
  const input = on === "enable" ? { KeyId: id, ...(n ? { RotationPeriodInDays: n } : {}) } : { KeyId: id };
  const save = async () => {
    setTried(true);
    if (bad) return;
    setPending(true);
    setRefused(null);
    try {
      await callKindOp(KEYS, on === "enable" ? "EnableKeyRotation" : "DisableKeyRotation", input);
      await qc.invalidateQueries({ queryKey: ["resources", "kindop", "kms", "keys"] });
      back();
    } catch (e) {
      setRefused(refusalOf(e));
    } finally {
      setPending(false);
    }
  };
  return (
    <FormPage
      title={d.name}
      mono
      titleExtra={<StateDot state={state} words={stateWords(state)} />}
      sub="Automatic key rotation. Choose Enable or Disable, and the days between rotations."
      crumbs={resourceCrumbs({ ...d, key: d.id }, { label: "Automatic key rotation", to: "" })}
      request={{ service: "kms", operation: on === "enable" ? "EnableKeyRotation" : "DisableKeyRotation", input }}
      callNote={on === "enable" ? "If Disable is selected, Save calls DisableKeyRotation with the same KeyId instead." : "If Enable is selected, Save calls EnableKeyRotation, with the period if there is one."}
      showInput
      refusal={refused}
      submitLabel="Save"
      pending={pending}
      disabled={tried && bad}
      disabledWhy="Enter a period of 90 to 2,560 days, or leave it empty."
      onSubmit={() => void save()}
      onCancel={back}
      banner={refused ? <Alert tone="bad" title={`The rotation was not changed: ${refused.code}`} testId="rotation-refused">{refused.message}</Alert> : undefined}
      testId="rotation-edit-page"
    >
      <FormSection title="Automatic key rotation" testId="section-rotation">
        <div className="grid gap-3">
          <span className="text-[14px] leading-none font-medium text-bright">Key rotation</span>
          <RowChoice name="rotation" label="Key rotation" value={on} onChange={setOn} options={[{ value: "enable", title: "Enable" }, { value: "disable", title: "Disable" }]} />
        </div>
        {on === "enable" && (
          <Field label="Rotation period" optional error={tried && n === undefined ? "Enter a whole number of days from 90 to 2,560." : undefined} testId="field-period" help={<><b>90 to 2,560 days, default 365.</b> KMS rotates this many days after you enable it, then every this many days. Stored, not applied locally: doze-aws keeps the period and reports it, but nothing fires on that schedule.</>}>
            {(fid) => (
              <div className="relative w-44">
                <Input id={fid} inputMode="numeric" value={days} onChange={(e) => setDays(e.target.value)} placeholder="365" className="bg-card pr-12 font-mono tabular-nums" data-testid="rotation-days" />
                <span aria-hidden className="pointer-events-none absolute inset-y-0 right-3 flex items-center text-[13px] text-muted-foreground">days</span>
              </div>
            )}
          </Field>
        )}
      </FormSection>
      <Note testId="rotation-edit-note">On AWS, a key that is Disabled or Pending deletion is not rotated. doze-aws refuses to change the rotation of a key that is pending deletion, and accepts the change for a Disabled one. Only symmetric encryption keys with key material from KMS can rotate automatically.</Note>
    </FormPage>
  );
}

