// The create and update wizards (cloudformation__stack-create* and stack-update*): four steps, each a page with the call it makes beside it.
// Step 1 reads the template (ValidateTemplate), step 2 its parameters (GetTemplateSummary), step 3 the options, step 4 the review, which
// makes the stack (CreateStack, UpdateStack) or a change set. At `?do=create` of the list and `?do=update` of a stack (`&mode=changeset` ends in a change set).
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useRef, useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { ApiFailure } from "@doze/ui/lib/api";
import { DetailError, FormPage, type PreviewRequest, RuleHelp } from "../../../kit";
import { callKindOp } from "../../../kit";
import type { OwnProps } from "../deep/registry";
import { at, JsonBlock } from "../deep/parts";
import { STACKS, useCfnPreview, useCfnRead, useCfnWrite, useStack, useStackNames } from "./api";
import { ChangeSetDialog } from "./changeset-dialog";
import type { Stack } from "./model";
import { makesWords } from "./sentences";
import { StepDetails } from "./step-details";
import { StepOptions } from "./step-options";
import { StepReview } from "./step-review";
import { StepTemplate } from "./step-template";
import { type Kept, keepWizard, takeWizard } from "./wizard-keep";
import { type Mode, type Summary, type Wizard, changeSetInput, emptyWizard, fromStack, nameProblem, optionsProblem, parameterProblem, previewName, stackInput, summaryInput, templateProblem, validateInput } from "./wizard-model";

const LABELS = ["Create stack", "Specify stack details", "Configure stack options", "Review and create"];

export const CreateStackPage = ({ service, kind }: OwnProps) => <WizardPage mode="create" service={service} kind={kind} stack={null} />;

export function UpdateStackPage({ service, kind, name, search }: OwnProps) {
  const stack = useStack(name ?? "");
  const s = stack.data?.Stacks?.[0];
  if (stack.isError) return <DetailError service={service} kind={kind} name={name ?? ""} error={stack.error} what="This stack could not be read" />;
  if (!s) return <Skeleton className="h-96 rounded-[var(--radius-card)]" />;
  return <WizardPage mode={search.mode === "changeset" ? "changeset" : "update"} service={service} kind={kind} stack={s} resume={search.mode === "resume"} />;
}

const req = (operation: string, input: object): PreviewRequest => ({ service: "cloudformation", operation, input });

function WizardPage({ mode, service, kind, stack, resume = false }: { mode: Mode; service: string; kind: string; stack: Stack | null; resume?: boolean }) {
  const navigate = useNavigate();
  // Back from reading the preview's change set: the choices, the step and the preview's name are as they were left.
  const [back0] = useState<Kept | null>(() => (stack ? takeWizard(stack.StackName, resume) : null));
  const [w, setW] = useState<Wizard>(() => back0?.w ?? (stack ? fromStack(mode, stack) : emptyWizard(mode)));
  const [step, setStep] = useState(back0?.step ?? 0);
  const [furthest, setFurthest] = useState(back0?.furthest ?? 0);
  const [summary, setSummary] = useState<Summary | undefined>(back0?.summary);
  const [tried, setTried] = useState(false);
  const [refusal, setRefusal] = useState<{ code: string; message: string; op: string } | null>(null);
  const [checking, setChecking] = useState(false);
  const [asking, setAsking] = useState(false);
  const names = useStackNames().data?.Stacks ?? [];
  const taken = names.map((s) => s.StackName);
  const set = (p: Partial<Wizard>) => setW((x) => ({ ...x, ...p }));
  const update = mode !== "create";
  const stackName = stack?.StackName ?? w.name;
  const make = useCfnWrite<{ StackId?: string }>(update ? "UpdateStack" : "CreateStack");
  const existingSets = useCfnRead<{ Summaries?: { ChangeSetName: string }[] }>("ListChangeSets", { StackName: stackName }, { enabled: update });
  const makeSet = useCfnWrite<{ Id?: string; StackId?: string }>("CreateChangeSet");
  const current = useMemo(() => Object.fromEntries((stack?.Parameters ?? []).map((p) => [p.ParameterKey, p.ParameterValue ?? ""])), [stack]);
  const labels = update ? [...LABELS.slice(0, 3), `Review ${stackName}`].map((l, i) => (i === 0 ? "Update stack" : l)) : LABELS;
  const back = () => void navigate({ to: update ? "/resources/$service/$kind/$name" : "/resources/$service/$second", params: update ? { service, kind, name: stackName } as never : { service, second: kind } as never, search: {} as never });
  const problems = [templateProblem(w), summary ? (nameProblem(w.name, taken) && !update ? nameProblem(w.name, taken) : (summary.Parameters ?? []).map((d) => parameterProblem(w, d)).find(Boolean) ?? null) : null, summary ? optionsProblem(w, summary) : null];
  // The preview of an update: a change set the console makes when the review step opens, and removes when the page is left.
  const [previewId] = useState(() => back0?.previewId ?? previewName(Date.now()));
  const previewInput = mode === "update" && step === 3 && summary ? { ...changeSetInput(w, summary, previewId), ChangeSetType: "UPDATE" } : null;
  const previewKey = previewInput ? JSON.stringify(previewInput) : "";
  const keepPreview = useRef(false);
  const preview = useCfnPreview(stackName, previewId, previewKey, previewInput, keepPreview, back0 !== null);
  const next = async () => {
    setTried(true);
    setRefusal(null);
    if (step === 0) {
      if (templateProblem(w)) return;
      setChecking(true);
      const v = validateInput(w);
      try {
        if (v) await callKindOp(STACKS, "ValidateTemplate", v);
      } catch (e) {
        setChecking(false);
        const f = e instanceof ApiFailure ? e : new ApiFailure(0, { code: "Error", message: (e as Error).message });
        setRefusal({ code: f.code, message: f.message, op: "ValidateTemplate" });
        return;
      }
      try {
        const sum = await callKindOp<Summary>(STACKS, "GetTemplateSummary", summaryInput(w) ?? {});
        setSummary(sum);
        const values = Object.fromEntries((sum.Parameters ?? []).flatMap((d) => (update && !d.NoEcho && current[d.ParameterKey] !== undefined ? [[d.ParameterKey, current[d.ParameterKey]!]] : [])));
        const keep = Object.fromEntries((sum.Parameters ?? []).flatMap((d) => (update && d.NoEcho ? [[d.ParameterKey, true]] : [])));
        set({ values: { ...values, ...w.values }, keep: { ...keep, ...w.keep } });
      } catch (e) {
        setChecking(false);
        const f = e instanceof ApiFailure ? e : new ApiFailure(0, { code: "Error", message: (e as Error).message });
        setRefusal({ code: f.code, message: f.message, op: "GetTemplateSummary" });
        return;
      }
      setChecking(false);
    } else if (problems[step]) return;
    setTried(false);
    setStep(step + 1);
    setFurthest(Math.max(furthest, step + 1));
  };
  const viewPreview = async () => {
    keepPreview.current = true;
    keepWizard({ stack: stackName, w, step, furthest, summary, previewId });
    // This page's own entry in the history says it is the one to come back to, so Back lands here with the choices; the change set is a new entry.
    await navigate({ to: "/resources/$service/$kind/$name", params: { service, kind, name: stackName }, search: { do: "update", mode: "resume" }, replace: true });
    await navigate({ to: "/resources/$service/$kind/$name", params: { service, kind, name: stackName }, search: { do: "changeset", name: previewId } });
  };
  const go = (i: number) => { setStep(i); setTried(false); setRefusal(null); };
  // The token the update goes out under (the events it makes carry it); one for the whole wizard, so the call shown is the call made.
  const [token] = useState(() => `Console-UpdateStack-${crypto.randomUUID()}`);
  const stackCall = () => (mode === "update" ? { ...stackInput(w, summary), ClientRequestToken: token } : stackInput(w, summary));
  const submit = () => {
    setRefusal(null);
    const input = stackCall();
    make.mutate(input, { onSuccess: () => void navigate({ to: "/resources/$service/$kind/$name", params: { service, kind, name: stackName }, search: update ? { tab: "events", done: "updated" } : { tab: "events", done: "created" } }) });
  };
  const makeChangeSet = (name: string) => makeSet.mutate(changeSetInput(w, summary, name), { onSuccess: () => { setAsking(false); void navigate({ to: "/resources/$service/$kind/$name", params: { service, kind, name: stackName }, search: update ? { do: "changeset", name } : {} }); } });
  const lead = [
    update ? `Step 1 of 4 for ${stackName}. Keep the template or replace it. A direct update deploys right away; the last step previews it as a change set first.` : "Step 1 of 4. Choose the template that describes the resources. Nothing is created until you submit on the last step.",
    update ? `Step 2 of 4 for ${stackName}. Each parameter starts at the value the stack has now. Change the ones you want to change.` : "Step 2 of 4. Name the stack and answer the parameters its template declares.",
    update ? `Step 3 of 4 for ${stackName}. The same options as at creation, except the ones that only apply when a stack is first made.` : "Step 3 of 4. Tags, permissions, what to do when creation fails, and termination protection. Every option here has a default; you can go straight on.",
    update ? `Step 4 of 4. Check what you chose and what it would change. Choose Edit on a section to change it.` : "Step 4 of 4. Check what you chose. Choose Edit on a section to change it before you submit.",
  ][step]!;
  const titles = update ? ["Update stack", "Specify stack details", "Configure stack options", `Review ${stackName}`] : ["Create stack", "Specify stack details", "Configure stack options", "Review and create"];
  const reached = refusal ? { code: refusal.code, message: refusal.message } : make.error ? (() => { const e = errorWords(make.error); return { code: e.code ?? "Error", message: e.message }; })() : null;
  const call: PreviewRequest | null =
    step === 0 ? (update && w.keepTemplate ? req("GetTemplateSummary", { StackName: stackName }) : validateInput(w) ? req("ValidateTemplate", validateInput(w)!) : null)
    : step === 1 && !update ? req("GetTemplateSummary", summaryInput(w) ?? {})
    : req(update ? "UpdateStack" : "CreateStack", stackCall());
  const asideStep = step === 1 && !update && summary ? (
    <div className="grid gap-2" data-testid="summary-panel">
      <span className="font-mono text-[12.5px] text-muted-foreground">returns</span>
      <div><JsonBlock value={{ ...(summary.Description ? { Description: summary.Description } : {}), ResourceTypes: summary.ResourceTypes ?? [] }} testId="summary-json" /></div>
      <p className="text-[13px] text-muted-foreground">{(summary.Parameters ?? []).length === 0 ? "No parameters are declared." : `${(summary.Parameters ?? []).length} parameter${(summary.Parameters ?? []).length === 1 ? " is" : "s are"} declared.`} The template makes {makesWords(summary.ResourceTypes, summary.ResourceIdentifierSummaries)}.</p>
    </div>
  ) : undefined;
  return (
    <>
      <FormPage
        title={titles[step]!}
        sub={lead}
        crumbs={[{ label: "Resources", to: "/resources" }, { label: "CloudFormation", to: `/resources/${service}` }, ...(update ? [{ label: stackName, to: `/resources/${service}/${kind}/${encodeURIComponent(stackName)}` }] : [{ label: "Stacks", to: `/resources/${service}/${kind}` }]), { label: titles[step]!, to: "#" }]}
        stepsAfterTitle
        backLabel="Previous"
        showInput
        wizard={{ steps: labels.map((label, i) => ({ id: String(i), label })), current: step, furthest, onStep: go, onBack: () => go(step - 1), onNext: () => void next() }}
        banner={refusal ? <Alert tone="bad" title={refusal.op === "ValidateTemplate" ? "Template validation error." : `${refusal.op} was refused.`} testId="wizard-refused">{refusal.code}: {refusal.message}</Alert> : undefined}
        request={step === 3 ? call : call}
        callTitle={refusal ? "The call that was refused" : step === 1 && !update ? "The call that reads the template" : step >= 1 && step < 3 ? `The ${update ? "UpdateStack" : "CreateStack"} call so far` : undefined}
        refusal={reached}
        callNote={step === 1 && !update && summary ? asideStep : step === 0 ? (update && w.keepTemplate ? "Choosing Use existing template sends UsePreviousTemplate true to UpdateStack instead of a URL. Never both." : "Next reads the template and lists its parameters. Making the stack is the last step, and the call is shown there.") : step === 2 ? "Left blank, the role, the rollback configuration and the stack policy are left out of the call." : step === 3 ? <>{mode === "update" ? "The preview above comes from CreateChangeSet and DescribeChangeSet, made for you. It is separate from this call. " : ""}{summary && (summary.Capabilities ?? []).length > 0 ? `Capabilities sent: ${(summary.Capabilities ?? []).join(", ")}.` : "No capabilities are sent: the template holds no IAM resources."}</> : undefined}
        submitLabel={update ? (mode === "changeset" ? "Create change set" : "Update stack") : "Submit"}
        onSubmit={mode === "changeset" ? () => setAsking(true) : submit}
        onCancel={back}
        pending={make.isPending || checking}
        disabled={step < 3 && tried && Boolean(problems[step])}
        disabledWhy={problems[step]}
        footerExtra={step === 3 ? <>{update && mode !== "changeset" && <Button type="button" variant="outline" className="bg-card" disabled={!preview.data || preview.data.Status === "FAILED"} onClick={() => void viewPreview()} data-testid="view-changeset">View change set</Button>}{!update && <Button type="button" variant="outline" className="bg-card" onClick={() => setAsking(true)} data-testid="changeset-open">Create change set</Button>}</> : undefined}
        testId="stack-wizard"
      >
        {step === 0 && <StepTemplate w={w} set={set} problem={tried ? problems[0] : null} refused={refusal?.op === "ValidateTemplate"} stackName={stackName} />}
        {step === 1 && summary && <StepDetails w={w} set={set} summary={summary} taken={taken} tried={tried} current={current} born={stack ? { created: stack.CreationTime ? at(stack.CreationTime) : undefined, updated: Boolean(stack.LastUpdatedTime) } : undefined} />}
        {step === 2 && summary && <StepOptions w={w} set={set} summary={summary} tried={tried} was={stack?.Tags ?? []} />}
        {step === 3 && summary && (
          <>
            <StepReview w={w} summary={summary} edit={go} stackName={stackName} preview={update ? { data: preview.data, pending: preview.pending } : undefined} previewError={preview.error} />
            <RuleHelp className="max-w-[56rem]" >{update ? "Submit calls UpdateStack and opens the stack on its events. The update runs in the background: the stack is UPDATE_IN_PROGRESS first." : <>Submit calls CreateStack and opens the new stack on its events. The stack is made in the background: it is CREATE_IN_PROGRESS first and the page follows it. Create change set makes the stack but deploys nothing yet. This template makes {makesWords(summary.ResourceTypes, summary.ResourceIdentifierSummaries)}.</>}</RuleHelp>
          </>
        )}
      </FormPage>
      <ChangeSetDialog open={asking} onOpenChange={(o) => { setAsking(o); if (!o) makeSet.reset(); }} stack={stackName} creating={!update} request={(n) => req("CreateChangeSet", changeSetInput(w, summary, n))} taken={(existingSets.data?.Summaries ?? []).map((s) => s.ChangeSetName)} pending={makeSet.isPending} error={makeSet.error} onMake={makeChangeSet} />
    </>
  );
}
