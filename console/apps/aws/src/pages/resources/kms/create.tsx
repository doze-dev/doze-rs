// Create key (kms__key-create, key-create-labels, key-create-admins, key-create-users, key-create-policy, key-create-review): six
// steps on one page. Nothing is made until Finish: CreateKey with the policy the steps wrote, then CreateAlias for the key's alias.
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { rowsValid } from "@doze/ui/components/app/key-values";
import { Button } from "@doze/ui/components/ui/button";
import { useAbout } from "../../../api/queries";
import { callKindOp, FormPage, TAG_RULES } from "../../../kit";
import { useAllRows } from "../cfg-api";
import { ALIASES, KEYS } from "./api";
import { aliasInput, aliasProblem, blankKey, callsOf, createInput, descriptionProblem, type KeyForm, othersWords, policySent, shownInput } from "./create-model";
import { StepAdmins, StepPolicy, StepUsers } from "./create-perms";
import { StepReview } from "./create-review";
import { StepConfigure, StepLabels } from "./create-steps";
import { keyRows } from "./model";
import { crumbs } from "./parts";
import { refusalOf } from "./schedule";

const STEPS = [{ id: "configure", label: "Configure key" }, { id: "labels", label: "Add labels" }, { id: "admins", label: "Define key administrative permissions" }, { id: "users", label: "Define key usage permissions" }, { id: "policy", label: "Review and edit key policy" }, { id: "review", label: "Review" }];
const SUBS = [
  "Step 1. Choose the kind of key. The key type, key usage, key spec, origin and Regionality cannot be changed after the key is created.",
  "Step 2. An alias is required in the console. The description and tags are optional.",
  "Step 3. Choose the IAM users and roles that can administer this key. They manage it; they cannot use it to encrypt or decrypt.",
  "Step 4. Choose who can use this key in cryptographic operations.",
  "Step 5. Review the key policy statements for the key. To make changes to the key policy, choose Edit. You can also change the key policy at any time after the key exists.",
  "Step 6. Review the key settings that you chose. You can still go back and change all settings. Choose Finish to create the key.",
];
const TITLES = ["Configure key", "Add labels", "Define key administrative permissions", "Define key usage permissions", "Review and edit key policy", "Review"];
const PANEL = ["The call this will make", "The calls this will make", "Sent on the last step", "Sent on the last step", "Sent as the policy", "The calls Finish will make"];
const NOTES = [
  "The console picks SYMMETRIC_DEFAULT when you choose Symmetric and Encrypt and decrypt. Nothing is sent until Finish on the last step.",
  "CreateKey cannot take an alias, so the console creates the alias with a second call.",
  "So far: the account statement and the administrators statement. Users come next.",
  "The account, the administrators, the key users, and grants for AWS services.",
  "doze-aws stores the policy as written. With IAM in soft or enforce mode it checks the policy on every request that names the key.",
  "The answer carries the new key's id, which the alias call then names.",
];

export function KeyCreate() {
  const navigate = useNavigate();
  const qc = useQueryClient();
  const about = useAbout().data;
  const account = about?.account ?? "000000000000";
  const region = about?.region ?? "us-east-1";
  const [f, setF] = useState<KeyForm>(blankKey);
  const [step, setStep] = useState(0);
  const [furthest, setFurthest] = useState(0);
  const [tried, setTried] = useState(false);
  const [editing, setEditing] = useState(false);
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState<{ operation: string; code: string; message: string; keyId?: string } | null>(null);
  const keys = useAllRows("kms", "keys");
  const aliases = useAllRows("kms", "aliases");
  const first = keys.first;
  const taken = useMemo(() => aliases.rows.map((a) => a.name.replace(/^alias\//, "")), [aliases.rows]);
  const others = useMemo(() => othersWords(keyRows(keys.rows, keys.columns, aliases.rows.map((a) => ({ name: a.name, key: a.cells[aliases.columns.findIndex((c) => c.title === "Key")] ?? "" })))), [keys.rows, keys.columns, aliases.rows, aliases.columns]);
  const set = (patch: Partial<KeyForm>) => { setF((x) => ({ ...x, ...patch })); setFailed(null); };
  const problem = (s: number): string | null => {
    if (s === 1) return aliasProblem(f.alias, taken) ?? descriptionProblem(f.description) ?? (rowsValid(f.tags, TAG_RULES) ? null : "Fix the tags marked.");
    if (s === 4 && f.policy !== null) { try { const p = JSON.parse(f.policy) as unknown; return typeof p === "object" && p !== null && !Array.isArray(p) ? null : "The policy must be a JSON object."; } catch { return "The policy is not JSON."; } }
    return null;
  };
  const go = (to: number) => { setStep(to); setFurthest((x) => Math.max(x, to)); setTried(false); };
  const next = () => { setTried(true); if (problem(step)) return; go(step + 1); };
  const submit = async () => {
    for (const s of [1, 4]) if (problem(s)) { setStep(s); setTried(true); return; }
    setPending(true);
    setFailed(null);
    let keyId: string | undefined;
    let at = "CreateKey";
    try {
      const made = await callKindOp<{ KeyMetadata: { KeyId: string } }>(KEYS, "CreateKey", createInput(f, { account, withPolicy: true, withLabels: true }));
      keyId = made.KeyMetadata.KeyId;
      at = "CreateAlias";
      await callKindOp(ALIASES, "CreateAlias", aliasInput(f, keyId));
      void qc.invalidateQueries({ queryKey: ["resources", "list", "kms"] });
      void navigate({ to: "/resources/$service/$kind/$name", params: { service: "kms", kind: "keys", name: keyId }, search: { done: "created" } });
    } catch (e) {
      void qc.invalidateQueries({ queryKey: ["resources", "list", "kms"] });
      setFailed({ operation: at, ...refusalOf(e), keyId });
    } finally {
      setPending(false);
    }
  };
  // Steps 3 to 5 show what they add to the call Finish makes: the policy.
  const requests = step >= 2 && step <= 4 ? [{ service: "kms", operation: "CreateKey", input: { Policy: policySent(f, account) } }] : callsOf(f, { account, withPolicy: step >= 5, withLabels: step >= 1 });
  const back = () => void navigate({ to: "/resources/$service/$second", params: { service: "kms", second: "keys" }, search: {} });
  return (
    <FormPage
      title={TITLES[step]!}
      sub={SUBS[step]}
      crumbs={[...crumbs(first?.group, first?.service_title), { label: "Create key", to: "/resources/kms/keys?create=yes" }]}
      stepsAfterTitle
      request={null}
      requests={requests}
      callTitle={PANEL[step]}
      callNote={NOTES[step]}
      showInput
      shown={shownInput}
      refusal={failed ? { code: `${failed.operation}: ${failed.code}`, message: failed.message } : null}
      wizard={{ steps: STEPS, current: step, furthest, onStep: go, onBack: () => go(step - 1), onNext: next, nextDisabled: tried && problem(step) !== null }}
      backLabel="Previous"
      submitLabel="Finish"
      pending={pending}
      disabled={Boolean(problem(1) && tried)}
      onSubmit={() => void submit()}
      onCancel={back}
      banner={failed ? (
        <Alert tone="bad" title={failed.keyId ? `The key was created, but ${failed.operation} was refused.` : "The key was not created."} testId="create-refused"
          action={failed.keyId ? <Button size="sm" variant="outline" className="bg-card" onClick={() => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "kms", kind: "keys", name: failed.keyId! } })}>Open the key</Button> : undefined}>
          {failed.code}: {failed.message}
        </Alert>
      ) : undefined}
      testId="key-create"
    >
      {step === 0 && <StepConfigure f={f} set={set} others={others} region={region} />}
      {step === 1 && <StepLabels f={f} set={set} show={tried} taken={taken} />}
      {step === 2 && <StepAdmins f={f} set={set} account={account} />}
      {step === 3 && <StepUsers f={f} set={set} account={account} />}
      {step === 4 && <StepPolicy f={f} set={set} account={account} editing={editing} onEdit={setEditing} />}
      {step === 5 && <StepReview f={f} account={account} onEdit={go} />}
    </FormPage>
  );
}
