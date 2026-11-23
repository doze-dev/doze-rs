// Store a new secret (secretsmanager__create-type, create-configure, create-rotation, create-review): four steps on one page. Nothing is
// made until Store: CreateSecret, then PutResourcePolicy and RotateSecret when the steps chose them.
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { filled, rowsValid } from "@doze/ui/components/app/key-values";
import { ReviewSection } from "@doze/ui/components/app/stepper";
import { useAbout } from "../../../api/queries";
import { callKindOp, FormPage, TAG_RULES } from "../../../kit";
import { useAllRows } from "../cfg-api";
import { Note } from "../cfg-parts";
import { crumbs } from "../kms/parts";
import { refusalOf } from "../kms/schedule";
import { SECRETS } from "./api";
import { StepConfigure, StepRotation, StepType } from "./create-steps";
import { blankSecret, durationProblem, nameProblem, replicaProblem, replicaRegions, scheduleProblem, type SecretForm, secretCalls, valueProblem, secretValue } from "./model";

const STEPS = [{ id: "type", label: "Choose secret type" }, { id: "configure", label: "Configure secret" }, { id: "rotation", label: "Configure rotation", optional: true }, { id: "review", label: "Review" }];
const SUBS = ["Step 1 of 4. Choose what kind of secret this is and which KMS key encrypts it.", "Step 2 of 4. Name the secret and, if you like, tag it or attach a policy.", "Step 3 of 4. Rotation is optional: keep it off for now and turn it on later if you want.", "Step 4 of 4. Check the details, then store the secret."];
const TITLES = ["Store a new secret", "Store a new secret", "Store a new secret", "Store a new secret"];
const NOTES = [
  "Nothing is sent until you choose Store on step 4. The name, tags and the rest are added on the next steps; the value is masked here.",
  "Because the call carries tags, the caller also needs secretsmanager:TagResource.",
  "With rotation off, this step adds no call: Store sends the CreateSecret call from step 2 and nothing else. Turning rotation on adds a RotateSecret call that carries the rotation function and the schedule.",
  "The answer carries the new secret's ARN, its name and the id of its first version, which gets the label AWSCURRENT. Store returns you to the list of secrets.",
];

export function SecretCreate() {
  const navigate = useNavigate();
  const qc = useQueryClient();
  const about = useAbout().data;
  const account = about?.account ?? "";
  const region = about?.region ?? "us-east-1";
  const lambdaServed = about?.services.some((s) => s.name === "lambda" && s.served) ?? false;
  const [f, setF] = useState<SecretForm>(blankSecret);
  const [step, setStep] = useState(0);
  const [furthest, setFurthest] = useState(0);
  const [tried, setTried] = useState(false);
  const [openPolicy, setOpenPolicy] = useState(false);
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState<{ operation: string; code: string; message: string; made: boolean } | null>(null);
  const list = useAllRows("secretsmanager", "secrets", { params: { deleted: "yes" } });
  const taken = useMemo(() => list.rows.map((r) => r.name), [list.rows]);
  const first = list.first;
  const set = (patch: Partial<SecretForm>) => { setF((x) => ({ ...x, ...patch })); setFailed(null); };
  const problem = (s: number): string | null => {
    if (s === 0) return rowsValid(f.pairs, { caseSensitive: true, maxKey: 256 }) ? valueProblem(f.mode === "pairs" ? (filled(f.pairs).length > 0 ? secretValue(f) : "") : f.text) : "Fix the rows marked.";
    if (s === 1) return nameProblem(f.name, taken) ?? (f.description.length > 2048 ? "The description is too long." : null) ?? (rowsValid(f.tags, TAG_RULES) ? null : "Fix the tags marked.") ?? (f.policy.trim() !== "" && !validJson(f.policy) ? "The resource policy is not JSON." : null) ?? (f.replicas.some((r, i) => replicaProblem(r, i, f.replicas, region) !== null) ? "Fix the replica Regions marked." : null);
    if (s === 2 && f.rotate) return !f.fn ? "Choose the rotation function." : (scheduleProblem(f.schedule) ?? durationProblem(f.duration));
    return null;
  };
  const go = (to: number) => { setStep(to); setFurthest((x) => Math.max(x, to)); setTried(false); };
  const next = () => { setTried(true); if (problem(step)) return; go(step + 1); };
  const fnArn = f.fn ? `arn:aws:lambda:${region}:${account}:function:${f.fn}` : null;
  const form = { ...f, fn: fnArn };
  const submit = async () => {
    for (const s of [0, 1, 2]) if (problem(s)) { setStep(s); setTried(true); return; }
    setPending(true);
    setFailed(null);
    let at = "CreateSecret";
    let made = false;
    try {
      for (const c of secretCalls(form, { masked: false })) {
        at = c.operation;
        await callKindOp(SECRETS, c.operation, c.input);
        made = true;
      }
      void qc.invalidateQueries({ queryKey: ["resources", "list", "secretsmanager"] });
      void navigate({ to: "/resources/$service/$second", params: { service: "secretsmanager", second: "secrets" }, search: { do: "stored", name: f.name } });
    } catch (e) {
      void qc.invalidateQueries({ queryKey: ["resources", "list", "secretsmanager"] });
      setFailed({ operation: at, ...refusalOf(e), made });
    } finally {
      setPending(false);
    }
  };
  const preview = secretCalls({ ...form, name: f.name || "(set on step 2)" }, { masked: true });
  // Step 1 sets only the value and the key: the name, tags and the rest come on the next steps.
  const calls = step === 0 ? preview.slice(0, 1).map((c) => ({ ...c, input: { Name: c.input.Name, SecretString: c.input.SecretString, ...(c.input.KmsKeyId ? { KmsKeyId: c.input.KmsKeyId } : {}) } })) : preview;
  const back = () => void navigate({ to: "/resources/$service/$second", params: { service: "secretsmanager", second: "secrets" }, search: {} });
  const tags = filled(f.tags);
  return (
    <FormPage
      title={TITLES[step]!}
      sub={SUBS[step]}
      crumbs={[...crumbs(first?.group, first?.service_title).slice(0, -1), { label: "Secrets Manager", to: "/resources/secretsmanager" }, { label: "Secrets", to: "/resources/secretsmanager/secrets" }]}
      stepsAfterTitle
      request={null}
      requests={calls}
      callTitle={calls.length > 1 ? "The calls this will make" : "The call this will make"}
      callNote={NOTES[step]}
      showInput
      refusal={failed ? { code: `${failed.operation}: ${failed.code}`, message: failed.message } : null}
      wizard={{ steps: STEPS, current: step, furthest, onStep: go, onBack: () => go(step - 1), onNext: next, nextDisabled: tried && problem(step) !== null }}
      backLabel="Previous"
      submitLabel="Store"
      pending={pending}
      onSubmit={() => void submit()}
      onCancel={back}
      banner={failed ? <Alert tone="bad" title={failed.made ? `${f.name} was stored, but ${failed.operation} was refused.` : "The secret was not stored."} testId="create-refused">{failed.code}: {failed.message}</Alert> : undefined}
      testId="secret-create"
    >
      {step === 0 && <StepType f={f} set={set} show={tried} />}
      {step === 1 && <StepConfigure f={f} set={set} show={tried} taken={taken} openPolicy={openPolicy} setOpenPolicy={setOpenPolicy} />}
      {step === 2 && <StepRotation f={f} set={set} show={tried} lambdaServed={lambdaServed} />}
      {step === 3 && (
        <>
          <ReviewSection title="Choose secret type" onEdit={() => go(0)} rows={[{ label: "Secret type", value: "Other type of secret" }, { label: "Encryption key", value: <span className="font-mono text-[13px]">{f.key ?? "aws/secretsmanager"}</span> }, { label: "Secret value", value: f.mode === "pairs" ? `${filled(f.pairs).length} key/value pair${filled(f.pairs).length === 1 ? "" : "s"}, masked` : "Plaintext, masked" }]} />
          <ReviewSection title="Configure secret" onEdit={() => go(1)} rows={[{ label: "Secret name", value: <span className="font-mono text-[13px]" data-testid="review-name">{f.name}</span> }, { label: "Description", value: f.description || <span className="text-muted-foreground">None</span> }, { label: "Tags", value: tags.length === 0 ? <span className="text-muted-foreground">None</span> : <span className="grid gap-0.5">{tags.map((t) => <span key={t.key}>{t.key}: {t.value}</span>)}</span> }, { label: "Resource permissions", value: f.policy.trim() ? "A policy is attached after the secret is stored" : "None" }, { label: "Replicas", value: replicaRegions(f.replicas).length > 0 ? <span className="font-mono text-[13px]">{replicaRegions(f.replicas).join(", ")}</span> : <span className="text-muted-foreground">None</span> }]} />
          <ReviewSection title="Configure rotation" onEdit={() => go(2)} rows={[{ label: "Automatic rotation", value: f.rotate ? `On, ${f.schedule}${f.fn ? `, with ${f.fn}` : ""}` : "Off" }]} />
          <Note testId="review-note">Nothing has been sent yet. Store makes the calls on the right, in that order.</Note>
        </>
      )}
    </FormPage>
  );
}

function validJson(s: string): boolean {
  try { JSON.parse(s); return true; } catch { return false; }
}
