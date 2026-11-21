// Create policy (iam__policy-create, policy-create-review, policy-create-error): two steps. The editor, which stops a document that breaks the
// grammar before any call (and says what to add), then the name, description, the permissions the policy grants and the tags. Nothing is made
// until the review step: CreatePolicy, which starts the policy at version v1.
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { filled } from "@doze/ui/components/app/key-values";
import { type KeyValueRow } from "@doze/ui/components/app/key-values";
import { Input } from "@doze/ui/components/ui/input";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { ApiFailure } from "@doze/ui/lib/api";
import { BracesIcon } from "lucide-react";
import { useAbout } from "../../../api/queries";
import { useResourceList } from "../../../api/resources";
import { Field, FormPage, FormSection, TAG_RULES, callKindOp } from "../../../kit";
import { POLICIES, useRefresh } from "./api";
import { lacksResource, list, nameProblem, type Permission, permissions, starterPolicy, suggestedResource } from "./model";
import { crumbs, KINDS, Note } from "./parts";
import { PolicyJson, policyErrors } from "./policy-json";

const STEPS = [{ id: "editor", label: "Policy editor" }, { id: "review", label: "Review and create" }];

export function PolicyCreate() {
  const navigate = useNavigate();
  const refresh = useRefresh();
  const about = useAbout().data;
  const account = about?.account ?? "000000000000";
  const [doc, setDoc] = useState(() => starterPolicy(`arn:aws:sqs:${about?.region ?? "us-east-1"}:${account}:pick-lists`));
  const [step, setStep] = useState(0);
  const [tried, setTried] = useState(false);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [tags, setTags] = useState<KeyValueRow[]>([]);
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState<{ code: string; message: string } | null>(null);
  const existing = useResourceList("iam", "policies");
  const taken = useMemo(() => existing.data?.pages.flatMap((p) => p.rows).map((r) => r.label) ?? [], [existing.data]);
  const errors = policyErrors(doc, "identity");
  const blocked = tried && step === 0 && errors.length > 0;
  const problem = nameProblem(name, "policy") ?? (taken.includes(name) ? `A policy named ${name} already exists.` : null);
  const suggestion = suggestedResource(doc, account, about?.region ?? "us-east-1");
  const back = () => void navigate({ to: "/resources/$service/$second", params: { service: "iam", second: "policies" }, search: {} });
  const input = { PolicyName: name || "(next step)", PolicyDocument: doc, ...(description.trim() ? { Description: description.trim() } : {}), ...(filled(tags).length > 0 ? { Tags: filled(tags).map((t) => ({ Key: t.key, Value: t.value })) } : {}) };
  const submit = async () => {
    setTried(true);
    if (problem) return;
    setPending(true);
    setFailed(null);
    try {
      const made = (await callKindOp(POLICIES, "CreatePolicy", input)) as { Policy: { Arn: string } };
      refresh();
      void navigate({ to: "/resources/$service/$second", params: { service: "iam", second: "policies" }, search: { do: "created", name: made.Policy.Arn } });
    } catch (e) {
      setFailed({ code: e instanceof ApiFailure ? e.code : "Error", message: e instanceof Error ? e.message : "No reason was given." });
    } finally {
      setPending(false);
    }
  };
  const columns: DataColumn<Permission>[] = [
    { id: "service", header: "Service", size: 180, accessorFn: (r) => r.service, cell: (c) => c.row.original.service },
    { id: "actions", header: "Actions", size: 320, accessorFn: (r) => r.actions, cell: (c) => <span className="font-mono text-[13px] break-all">{c.row.original.actions}</span> },
    { id: "resource", header: "Resource", accessorFn: (r) => r.resource, cell: (c) => <span className="font-mono text-[13px] break-all">{c.row.original.resource}</span> },
  ];
  const blockedPanel = (
    <aside className="min-w-0 rounded-[14px] border bg-card p-5" aria-label="Nothing was created" data-testid="nothing-created">
      <h2 className="flex items-center gap-2 text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase"><BracesIcon aria-hidden className="size-3.5" /> Nothing was created</h2>
      <p className="mt-3 text-[13.5px] leading-[21px] text-foreground">CreatePolicy only runs on the review step, and it answers MalformedPolicyDocument for a document that breaks the grammar. The editor stops you before that.</p>
    </aside>
  );
  return (
    <FormPage
      title="Create policy"
      sub={step === 0 ? "Write the policy in the JSON editor. You name it on the next step." : "Name the policy and check the permissions it grants. Then create it; you attach it to users, groups or roles afterwards."}
      crumbs={[...crumbs(KINDS.policies), { label: "Create policy", to: "" }]}
      request={{ service: "iam", operation: "CreatePolicy", input }}
      panel={blocked ? blockedPanel : undefined}
      callNote={step === 0 ? "Nothing is created until Create policy on the review step. A customer managed policy keeps up to five versions." : "The new policy starts at version v1, which is its default version. Attach it afterwards from the policy's Actions menu or from a user, group or role."}
      refusal={failed ? { code: failed.code, message: failed.message } : null}
      banner={failed ? <Alert tone="bad" title={`CreatePolicy was refused: ${failed.code}`} testId="create-refused">{failed.message}</Alert> : undefined}
      wizard={{ steps: STEPS, current: step, furthest: step, nextDisabled: blocked, onStep: (i) => { if (i < step) setStep(i); }, onBack: () => setStep(0), onNext: () => { setTried(true); if (errors.length > 0 || doc.trim() === "") return; setTried(false); setStep(1); } }}
      backLabel="Previous"
      submitLabel="Create policy"
      pending={pending}
      onSubmit={() => void submit()}
      onCancel={back}
      testId="policy-create"
    >
      {step === 0 && (
        <FormSection title="Policy editor" description={blocked ? undefined : "Write the policy in the JSON editor. IAM validates the document as you edit it, and you must fix its errors before you continue."} testId="section-editor">
          {blocked && <p className="text-[14px] font-medium text-bad" role="alert" data-testid="must-fix">You must fix the problem before you can continue.</p>}
          <PolicyJson value={doc} onChange={(v) => { setDoc(v); }} kind="identity" label="JSON" rows={16} extra={blocked && lacksResource(doc) ? (
            <div className="grid gap-1 text-[13.5px] leading-[21px]" data-testid="resource-hint">
              <p className="font-mono text-[12.5px] font-medium text-bad">MalformedPolicyDocument</p>
              <p className="text-foreground">An identity policy statement names what it applies to: add a Resource (or NotResource) element.{suggestion ? <> For pick-lists that is <span className="font-mono break-all">{suggestion}</span>.</> : null}</p>
            </div>
          ) : undefined} />
        </FormSection>
      )}
      {step === 1 && (
        <>
          <FormSection title="Policy details" testId="section-details">
            <Field label="Policy name" error={tried ? (problem ?? undefined) : undefined} help={<><b>Up to 128 characters</b>: letters, digits and + = , . @ _ -. {taken.length > 0 ? <><span className="font-mono">{list(taken.slice(0, 4))}</span> already {taken.length === 1 ? "exists" : "exist"}; a name must be unique in the account.</> : "A name must be unique in the account."}</>} testId="field-name">
              {(id) => <Input id={id} value={name} onChange={(e) => { setName(e.target.value); setFailed(null); }} placeholder="pick-lists-send-only" autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="policy-name" />}
            </Field>
            <Field label="Description" optional help="Up to 1,000 characters.">
              {(id) => <Textarea id={id} value={description} maxLength={1000} onChange={(e) => setDescription(e.target.value)} rows={3} className="bg-card" data-testid="policy-description" />}
            </Field>
          </FormSection>
          <FormSection title="Permissions defined in this policy" description="Check that these are the permissions you intend to grant." testId="section-permissions">
            <DataTable aria-label="Permissions" data={permissions(doc)} columns={columns} rowId={(r) => `${r.service}|${r.actions}|${r.resource}`} maxHeight={320} empty="This policy grants nothing: it has no Allow statement." />
          </FormSection>
          <FormSection title="Tags" optional description="Tags are key-value pairs that label the policy." testId="section-tags">
            {tags.length === 0 && <p className="text-[13.5px] text-muted-foreground">No tags.</p>}
            <KeyValueEditor rows={tags} onChange={setTags} rules={TAG_RULES} testId="create-tags" />
          </FormSection>
          <Note testId="v1-note">A policy is created at version v1, its default; a customer managed policy keeps up to five versions.</Note>
        </>
      )}
    </FormPage>
  );
}
