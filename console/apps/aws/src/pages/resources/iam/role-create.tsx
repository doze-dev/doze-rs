// Create role (iam__role-create, role-create-permissions, role-create-review): three steps on one page. Who may assume it (the trust policy
// the choice builds, shown beside), what it may do (policies), then its name and the review. Nothing is made until the last step:
// CreateRole with the trust policy, then one AttachRolePolicy per policy.
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { CodeBlock } from "@doze/ui/components/app/code-block";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { ApiFailure } from "@doze/ui/lib/api";
import { BracesIcon } from "lucide-react";
import { useAbout } from "../../../api/queries";
import { useResourceList } from "../../../api/resources";
import { Field, FormPage, FormSection, PolicyPicker, TAG_RULES, callKindOp } from "../../../kit";
import { ROLES, USERS, useRead, useRefresh } from "./api";
import { awsArn, list, SERVICES, trustDocument } from "./model";
import { CardChoice, crumbs, KINDS, Note, ReviewBlock, RowChoice } from "./parts";
import { PolicyChooser, usePolicyOptions } from "./policy-chooser";
import { PolicyJson } from "./policy-json";
import { blankRole, roleCalls, type RoleForm, stepProblem } from "./role-create-model";
import { SamlFields, WebIdentityFields } from "./role-federation";
import { TrustNote } from "./trust-note";

const STEPS = [{ id: "trust", label: "Select trusted entity" }, { id: "permissions", label: "Add permissions" }, { id: "review", label: "Name, review, and create" }];
const TITLES = ["Select trusted entity", "Add permissions", "Name, review, and create"];
const SUBS = [
  "Decide who can assume the role. The trusted entity becomes the role's trust policy; you set what the role may do on the next step.",
  "Choose the policies that anyone who assumes the role will get. You can also attach policies after the role exists.",
  "Give the role a name, check the two earlier steps, and create it. The name cannot be changed afterwards.",
];

export function RoleCreate() {
  const navigate = useNavigate();
  const refresh = useRefresh();
  const about = useAbout().data;
  const account = about?.account ?? "000000000000";
  const [f, setF] = useState<RoleForm>(blankRole);
  const [step, setStep] = useState(0);
  const [furthest, setFurthest] = useState(0);
  const [tried, setTried] = useState(false);
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState<{ operation: string; code: string; message: string } | null>(null);
  const aliases = useRead<{ AccountAliases: string[] }>(USERS, "ListAccountAliases", {});
  const alias = aliases.data?.AccountAliases[0];
  const policyCount = usePolicyOptions().options.length;
  const roles = useResourceList("iam", "roles");
  const taken = useMemo(() => roles.data?.pages.flatMap((p) => p.rows).map((r) => r.name) ?? [], [roles.data]);
  const set = (patch: Partial<RoleForm>) => { setF((x) => ({ ...x, ...patch })); setFailed(null); };
  const setTrust = (patch: Partial<RoleForm["trust"]>) => set({ trust: { ...f.trust, ...patch } });
  const t = f.trust;
  const problem = stepProblem(f, step, taken);
  const go = (to: number) => { setStep(to); setFurthest((x) => Math.max(x, to)); };
  const next = () => { if (step !== 1) setTried(true); if (stepProblem(f, step, taken)) return; setTried(false); go(step + 1); };
  const doc = trustDocument(t, account);
  const calls = roleCalls(f, account);
  const shown = step === 1 ? roleCalls(f, account, false).slice(0, 2) : calls;
  const submit = async () => {
    setTried(true);
    if (stepProblem(f, 2, taken)) return;
    setPending(true);
    setFailed(null);
    let at = "";
    try {
      for (const c of calls) {
        at = c.operation;
        await callKindOp(ROLES, c.operation, c.input as object);
      }
      refresh();
      void navigate({ to: "/resources/$service/$second", params: { service: "iam", second: "roles" }, search: { do: "created", name: f.name } });
    } catch (e) {
      setFailed({ operation: at, code: e instanceof ApiFailure ? e.code : "Error", message: e instanceof Error ? e.message : "No reason was given." });
    } finally {
      setPending(false);
    }
  };
  const trustPanel = (
    <aside className="min-w-0 rounded-[14px] border bg-card p-5" aria-label="The trust policy this builds" data-testid="trust-panel">
      <h2 className="flex items-center gap-2 text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase"><BracesIcon aria-hidden className="size-3.5" /> The trust policy this builds</h2>
      <CodeBlock text={doc} className="mt-3 max-h-[34rem]" testId="trust-json" json />
      <p className="mt-3 text-[13px] leading-5 text-muted-foreground">It becomes the AssumeRolePolicyDocument of CreateRole on the last step.</p>
      <TrustNote className="mt-3" federated={t.type === "web" || t.type === "saml"} />
    </aside>
  );
  const names = (p: ReadonlySet<string>) => [...p].map((a) => a.split("/").pop() ?? a);
  return (
    <FormPage
      title={TITLES[step]!}
      sub={SUBS[step]}
      crumbs={[...crumbs(KINDS.roles), { label: "Create role", to: "" }]}
      request={null}
      requests={shown}
      panel={step === 0 ? trustPanel : undefined}
      callTitle={step === 1 ? "The calls this will add" : "The calls this will make"}
      callNote={step === 1 ? "One call per policy, after CreateRole succeeds." : "CreateRole carries the trust policy; each policy you chose is then attached with its own AttachRolePolicy call."}
      refusal={failed ? { code: `${failed.operation}: ${failed.code}`, message: failed.message } : null}
      wizard={{ steps: STEPS, current: step, furthest, onStep: go, onBack: () => go(step - 1), onNext: next }}
      backLabel="Previous"
      submitLabel="Create role"
      pending={pending}
      onSubmit={() => void submit()}
      onCancel={() => void navigate({ to: "/resources/$service/$second", params: { service: "iam", second: "roles" }, search: {} })}
      banner={failed ? <Alert tone="bad" title={failed.operation === "CreateRole" ? "The role was not created." : `${f.name} was created, but ${failed.operation} was refused.`} testId="create-refused">{failed.code}: {failed.message}</Alert> : undefined}
      testId="role-create"
    >
      {step === 0 && (
        <>
          <FormSection title="Trusted entity type" testId="section-type">
            <CardChoice name="type" value={t.type} onChange={(type) => setTrust(type === "custom" && !t.custom ? { type, custom: trustDocument({ ...t, type: t.type === "custom" ? "account" : t.type }, account) } : { type })} className="sm:grid-cols-6" options={[
              { value: "service", title: "AWS service", description: "Let Lambda or another service act on your behalf.", span: "sm:col-span-2" },
              { value: "account", title: "AWS account", description: "Let identities in this or another account assume the role.", span: "sm:col-span-2" },
              { value: "custom", title: "Custom trust policy", description: "Write the trust policy yourself.", span: "sm:col-span-2" },
              { value: "web", title: "Web identity", description: "Let users signed in with an OpenID Connect provider assume the role.", span: "sm:col-span-3" },
              { value: "saml", title: "SAML 2.0 federation", description: "Let users signed in with a SAML provider assume the role.", span: "sm:col-span-3" },
            ]} />
          </FormSection>
          {t.type === "service" && (
            <FormSection title="An AWS service" description="Choose the service that assumes the role." testId="section-service">
              <Field label="Service">
                {(id) => (
                  <Select value={t.service} onValueChange={(service) => setTrust({ service })}>
                    <SelectTrigger id={id} className="w-full max-w-sm bg-card" data-testid="trust-service"><SelectValue /></SelectTrigger>
                    <SelectContent>{SERVICES.map((s) => <SelectItem key={s.value} value={s.value}>{s.label} ({s.value})</SelectItem>)}</SelectContent>
                  </Select>
                )}
              </Field>
            </FormSection>
          )}
          {t.type === "account" && (
            <FormSection title="An AWS account" testId="section-account">
              <RowChoice name="account" value={t.account} onChange={(account) => setTrust({ account })} options={[
                { value: "this", title: <>This account ({account})</>, description: <>Identities in {alias ? <span className="font-mono">{alias}</span> : "this account"} are the trusted entity.</> },
                { value: "other", title: "Another AWS account", description: "Enter the Account ID of the account that gets access." },
              ]}>
                {{ other: <Field label="Account ID" error={tried && step === 0 && t.account === "other" ? (stepProblem(f, 0, taken) ?? undefined) : undefined}>{(id) => <Input id={id} value={t.other} inputMode="numeric" maxLength={12} onChange={(e) => setTrust({ other: e.target.value })} placeholder="210987654321" className="max-w-sm bg-card font-mono" data-testid="trust-other" />}</Field> }}
              </RowChoice>
              <div className="grid gap-2">
                <Label className="items-start gap-3 font-normal leading-5"><Checkbox checked={t.external} onCheckedChange={(v) => setTrust({ external: v === true })} className="mt-0.5" data-testid="trust-external" /><span className="grid gap-0.5"><span className="font-medium text-bright">Require external ID</span><span className="text-[13px] text-muted-foreground">For users from an account you do not control who assume the role from the CLI or an API.</span></span></Label>
                {t.external && <div className="grid gap-3 pl-7"><Input value={t.externalId} onChange={(e) => setTrust({ externalId: e.target.value })} placeholder="an external ID" aria-label="External ID" className="max-w-sm bg-card font-mono" data-testid="trust-external-id" /><Note>An external ID adds a condition on <span className="font-mono">sts:ExternalId</span> to the trust policy. The console cannot switch to such a role: call AssumeRole with ExternalId from the CLI or an SDK.</Note></div>}
              </div>
              <Label className="items-start gap-3 font-normal leading-5"><Checkbox checked={t.mfa} onCheckedChange={(v) => setTrust({ mfa: v === true })} className="mt-0.5" data-testid="trust-mfa" /><span className="grid gap-0.5"><span className="font-medium text-bright">Require MFA</span><span className="text-[13px] text-muted-foreground">Adds a condition on <span className="font-mono">aws:MultiFactorAuthPresent</span>: AssumeRole then needs an MFA code (SerialNumber and TokenCode) from a virtual device registered in doze-aws.</span></span></Label>
            </FormSection>
          )}
          {t.type === "web" && <WebIdentityFields t={t} set={setTrust} tried={tried} problem={problem} />}
          {t.type === "saml" && <SamlFields t={t} set={setTrust} tried={tried} problem={problem} />}
          {t.type === "custom" && (
            <FormSection title="Custom trust policy" description="Write who may assume the role: every statement needs a Principal and none may name a Resource." testId="section-custom">
              <PolicyJson value={t.custom} onChange={(custom) => setTrust({ custom })} kind="trust" label="Trust policy" rows={14} />
            </FormSection>
          )}
          {tried && problem && <p className="text-[13px] text-bad" role="alert" data-testid="step-problem">{problem}</p>}
        </>
      )}
      {step === 1 && (
        <>
          <FormSection title={`Permissions policies (${policyCount})`} description="Select one or more policies to attach to the role." testId="section-policies">
            <PolicyChooser value={f.policies} onChange={(policies) => set({ policies })} entities filter footnote={<p className="max-w-[46rem] text-[13.5px] leading-[21px] text-muted-foreground" data-testid="policy-note">By default a role has no permissions. Selecting none is allowed; attach policies to the role later. IAM lists a customer managed policy always, and an AWS managed one once something has it attached; the other AWS managed names are ones doze-aws makes a document for when they are attached by ARN, so they show 0.</p>} />
          </FormSection>
          <FormSection title="Set permissions boundary" optional collapsible defaultOpen={false} summary="An advanced feature. Open it to use a policy as the most the role can ever do." description="A boundary sets the most the role can ever do, and grants nothing by itself." testId="section-boundary">
            <Field label="Permissions boundary policy">{(id) => <PolicyPicker id={id} value={f.boundary} onChange={(boundary) => set({ boundary })} />}</Field>
          </FormSection>
        </>
      )}
      {step === 2 && (
        <>
          <FormSection title="Role details" testId="section-details">
            <Field label="Role name" error={tried ? (problem ?? undefined) : undefined} help={<><b>Up to 64 characters</b>: letters, digits and + = , . @ _ -. The name must be unique in the account. In a policy or an ARN it is case sensitive. Names that differ only in case, such as PRODROLE and prodrole, are the same name: a second one is refused.{taken.length > 0 && <> <span className="font-mono">{list(taken.slice(0, 5))}</span> already {taken.length === 1 ? "exists" : "exist"}.</>}</>} testId="field-name">
              {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="larder-readonly" autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="role-name" />}
            </Field>
            <Field label="Description" optional help="Up to 1,000 characters. You can edit it later from the role's Summary.">
              {(id) => <Textarea id={id} value={f.description} maxLength={1000} onChange={(e) => set({ description: e.target.value })} rows={3} className="bg-card" data-testid="role-description" />}
            </Field>
          </FormSection>
          <ReviewBlock title="Step 1: Select trusted entities" onEdit={() => go(0)} testId="review-trust"><CodeBlock text={doc} wrap={false} className="max-h-72" json /><TrustNote federated={t.type === "web" || t.type === "saml"} /></ReviewBlock>
          <ReviewBlock title="Step 2: Add permissions" onEdit={() => go(1)} testId="review-policies">
            {f.policies.size === 0 ? <p className="text-[13.5px] text-muted-foreground">No policies: the role starts with no permissions.</p> : (
              <ul className="divide-y rounded-lg border">
                <li className="grid grid-cols-2 bg-muted/50 px-4 py-2 text-[12px] font-medium tracking-[0.04em] text-muted-foreground uppercase"><span>Policy name</span><span>Type</span></li>
                {[...f.policies].map((a, i) => <li key={a} className="grid grid-cols-2 px-4 py-2.5 text-[13.5px]"><span className="font-mono text-bright">{names(f.policies)[i]}</span><span>{a.startsWith(awsArn("")) ? "AWS managed" : "Customer managed"}</span></li>)}
              </ul>
            )}
          </ReviewBlock>
          <FormSection title="Tags" optional description="Tags are key-value pairs that label the role. A role can carry up to 50 tags." testId="section-tags">
            {f.tags.length === 0 && <p className="text-[13.5px] text-muted-foreground">No tags.</p>}
            <KeyValueEditor rows={f.tags} onChange={(tags) => set({ tags })} rules={TAG_RULES} testId="create-tags" />
          </FormSection>
        </>
      )}
    </FormPage>
  );
}
