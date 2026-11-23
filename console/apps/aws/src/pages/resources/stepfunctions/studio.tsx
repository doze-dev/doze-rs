// Workflow Studio: the page that makes a state machine (stepfunctions__machine-create-design, create-code, create-config, create-role)
// and edits one (machine-edit, version-publish). Three modes over one draft: Design (the graph and an inspector), Code (the Amazon
// States Language, checked as it is typed) and Config (name, type, role, logging, optional settings). Nothing exists until Create.
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Callout } from "@doze/ui/components/app/callout";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { ApiFailure } from "@doze/ui/lib/api";
import { CallPanel, callKindOp, useServiceCrumbs } from "../../../kit";
import { useDebounced } from "../../../kit/debounce";
import { callOp, on } from "../deep/api";
import { CodeBlock } from "../deep/parts";
import { AslEditor } from "./asl-editor";
import { fieldOf } from "./details-card";
import { type Definition, parseDefinition } from "./graph";
import { GraphView } from "./graph-view";
import { machineHref, useGoTo, useVersions, useWhere } from "./hooks";
import { LOG_NAME, TRUST, permissionsOf, pretty, roleNameOf } from "./model";
import { PublishDialog } from "./publish-dialog";
import { ConfigMode } from "./studio-config";
import { DesignMode } from "./studio-design";
import { BLANK, type Config, FIRST_CONFIG, type Level, type Validation, createBody, createProblem, diagnosticWords, furtherProblems, madeRoleArn, problemLines, updateBody, wordsOf } from "./studio-model";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";

export type Mode = "design" | "code" | "config";
const MACHINES = { service: "stepfunctions", kind: "machines" } as const;
const MODES: { id: Mode; label: string }[] = [
  { id: "design", label: "Design" },
  { id: "code", label: "Code" },
  { id: "config", label: "Config" },
];

/** The definition checked by ValidateStateMachineDefinition, once the text has stopped changing. */
export function useValidation(text: string, type: string) {
  const settled = useDebounced(text, 350);
  const q = useQuery({
    queryKey: ["stepfunctions", "validate", type, settled.value],
    queryFn: ({ signal }) => callKindOp<Validation>(MACHINES, "ValidateStateMachineDefinition", { definition: settled.value, type }, signal),
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
  });
  return { validation: q.data, valid: settled.settled && q.data?.result === "OK", pending: !settled.settled || (q.isFetching && !q.data), failed: q.error };
}

/** The config a machine has now, for the editor to start from. */
function configOf(d: ResourceDetail): Config {
  const level = (fieldOf(d, "Logging") ?? "OFF") as Level;
  return { ...FIRST_CONFIG, type: fieldOf(d, "Type") === "EXPRESS" ? "EXPRESS" : "STANDARD", role: "arn", roleArn: fieldOf(d, "IAM role ARN") ?? "", level, data: fieldOf(d, "Include execution data") === "Yes", tracing: fieldOf(d, "Tracing") === "Enabled", publish: false };
}

/** Whether the settings are the ones the machine has (the tags and the publish switch are only for a new one). */
const sameConfig = (a: Config, b: Config): boolean => a.type === b.type && a.roleArn === b.roleArn && a.level === b.level && a.data === b.data && a.tracing === b.tracing && a.kms === b.kms && a.kmsKey === b.kmsKey && a.kmsPeriod === b.kmsPeriod;

const prettyText = (text: string): string => {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
};

type Props = { editing: false; name: string; taken: string[] } | { editing: true; detail: ResourceDetail; taken?: string[] };

export function Studio(props: Props) {
  const go = useGoTo();
  const qc = useQueryClient();
  const where = useWhere();
  const base = useServiceCrumbs("stepfunctions");
  const detail = props.editing ? props.detail : null;
  const [name, setName] = useState(props.editing ? props.detail.name : props.name);
  const [text, setText] = useState(() => (props.editing ? prettyText(fieldOf(props.detail, "Definition") ?? "") : JSON.stringify(BLANK, null, 2)));
  const [cfg, setCfg] = useState<Config>(() => (props.editing ? configOf(props.detail) : FIRST_CONFIG));
  const [mode, setMode] = useState<Mode>(props.editing ? "code" : "design");
  const [tried, setTried] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [publishing, setPublishing] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const { validation, valid, pending } = useValidation(text, cfg.type);
  const definition = useMemo<Definition | null>(() => (valid ? parseDefinition(text) : null), [valid, text]);
  const set = (c: Partial<Config>) => setCfg((p) => ({ ...p, ...c }));
  const taken = props.taken ?? [];
  const savedText = detail ? prettyText(fieldOf(detail, "Definition") ?? "") : "";
  const dirty = props.editing && text !== savedText;
  // UpdateStateMachine makes a revision even when nothing in it differs (the AWS CLI does too), so a Save with nothing to save is not offered.
  const unchanged = props.editing && !dirty && detail !== null && sameConfig(cfg, configOf(detail));
  const cancel = () => (props.editing ? go.machine(name) : go.list());
  const sentence = (m: Mode): React.ReactNode => {
    if (props.editing) return <>Workflow Studio, {m === "design" ? "Design" : m === "code" ? "Code" : "Config"} mode. Running executions keep the definition and role they started with.</>;
    if (m === "design") return <>Workflow Studio, Design mode. Until now this is a draft: no resources have been created yet. <span className="text-muted-foreground">Name and type are set in Config mode.</span></>;
    if (m === "code") return <>Workflow Studio, Code mode. <span className="text-muted-foreground" data-testid="studio-code-words">{!valid && !pending ? (diagnosticWords(validation) ?? "The definition is not valid.") : "Write the Amazon States Language; the graph redraws as it is valid."}</span></>;
    return <>Workflow Studio, Config mode. <span className="text-muted-foreground">Name, type, permissions, logging and the optional settings. Review them, then choose Create.</span></>;
  };
  // The calls of a create, in order; the first call that is refused stops it and is the one the page names.
  const create = useMutation({
    mutationFn: async () => {
      const step = async <T,>(operation: string, input: object): Promise<T> => {
        try {
          return await callKindOp<T>(MACHINES, operation, input);
        } catch (e) {
          throw Object.assign(e instanceof ApiFailure ? e : new Error(String(e)), { operation });
        }
      };
      if (cfg.role === "create") {
        await step("CreateRole", { RoleName: roleNameOf(name), AssumeRolePolicyDocument: JSON.stringify(TRUST), Description: `Execution role of the ${name} state machine` });
        const policy = permissionsOf(definition ?? undefined, where);
        if (policy) await step("PutRolePolicy", { RoleName: roleNameOf(name), PolicyName: `${name}-tasks`, PolicyDocument: JSON.stringify(policy) });
      }
      if (cfg.level !== "OFF") {
        try {
          await step("CreateLogGroup", { logGroupName: `/aws/vendedlogs/states/${name}-Logs` });
        } catch (e) {
          if (!(e instanceof ApiFailure && e.code === "ResourceAlreadyExistsException")) throw e;
        }
      }
      return step<{ stateMachineArn: string }>("CreateStateMachine", createBody(name, text, cfg, where));
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["resources", "list", "stepfunctions", "machines"] });
      go.machine(name, { done: cfg.role === "create" ? "created-role" : "created" });
    },
  });
  const save = useMutation({
    mutationFn: () => callOp<{ updateDate: number }>(on.machine(name), "UpdateStateMachine", updateBody(name, text, cfg, where)),
    onSuccess: () => {
      setSaved(`${name} was saved. UpdateStateMachine made a new revision; running executions keep the definition and role they started with.`);
      void qc.invalidateQueries({ queryKey: ["resources", "one", "stepfunctions", "machines", name] });
    },
  });
  const problem = props.editing ? (valid ? null : "The definition is invalid: fix it first.") : createProblem(name, cfg, valid, taken) ?? (cfg.level !== "OFF" && !LOG_NAME.test(name) ? "To send logs to CloudWatch, the name may use only 0-9 A-Z a-z - _ ." : null);
  const onCreate = () => {
    setTried(true);
    if (problem) {
      setMode("config");
      return;
    }
    if (cfg.role === "create") setConfirm(true);
    else create.mutate();
  };
  const switchTo = (m: Mode) => {
    if (m !== mode && mode === "code" && !valid) return;
    setMode(m);
  };
  const failed = (create.error ?? save.error) as (Error & { operation?: string }) | null;
  const words = failed ? errorWords(failed) : null;
  const refusal = words ? { code: words.code ?? "Error", message: words.message } : null;
  const request = props.editing
    ? { service: "stepfunctions", operation: "UpdateStateMachine", input: { stateMachineArn: detail?.id ?? "", ...updateBody(name, text, cfg, where) } }
    : { service: "stepfunctions", operation: "CreateStateMachine", input: createBody(name, text, cfg, where) };
  const crumbs = [...base, { label: "State machines", to: "/resources/stepfunctions/machines" }, ...(props.editing ? [{ label: name, to: machineHref(name) }, { label: "Edit state machine", to: machineHref(name, { do: "edit" }) }] : [{ label: "Create state machine", to: "/resources/stepfunctions/machines" }])];
  return (
    <div data-testid="studio" data-mode={mode}>
      <PageHeader
        title={name}
        mono
        crumbs={crumbs}
        titleExtra={props.editing ? undefined : <Badge variant="secondary" className="font-normal text-muted-foreground" data-testid="studio-draft">Draft</Badge>}
        sub={<span data-testid="studio-sub">{sentence(mode)}</span>}
        actions={props.editing ? (
          <>
            <Button type="button" variant="outline" className="bg-card" onClick={cancel} data-testid="studio-cancel">Cancel</Button>
            <Button type="button" variant="outline" className="bg-card" onClick={() => setPublishing(true)} data-testid="studio-publish">Publish version</Button>
            <Button type="button" disabled={!valid || save.isPending || pending || unchanged} title={unchanged ? "Nothing has changed since the last save." : undefined} onClick={() => save.mutate()} data-testid="studio-save">{save.isPending ? "Saving…" : "Save"}</Button>
          </>
        ) : undefined}
      />
      {saved && <ResultBanner className="mb-6" title="Saved." returned={saved} onDismiss={() => setSaved(null)} testId="studio-saved" />}
      {failed && !words?.code?.startsWith("x") && <Alert tone="bad" className="mb-6" title={`${failed.operation ?? (props.editing ? "UpdateStateMachine" : "CreateStateMachine")} was refused${words?.code ? `: ${words.code}` : ""}`} testId="studio-refused">{words?.message}{failed.operation && failed.operation !== "CreateStateMachine" && failed.operation !== "UpdateStateMachine" ? " Nothing was created after it." : ""}</Alert>}
      <Tabs value={mode} onValueChange={(v) => switchTo(v as Mode)} className="mb-8 gap-0">
        <TabsList variant="line" className="h-11 w-full justify-start gap-2 overflow-x-auto rounded-none border-b p-0" aria-label="Workflow Studio modes">
          {MODES.map((m) => (
            <TabsTrigger key={m.id} value={m.id} className="h-11 flex-none px-3 text-[14px] after:bottom-[-1px]" data-testid={`mode-${m.id}`}>{m.label}</TabsTrigger>
          ))}
        </TabsList>
      </Tabs>
      {mode === "design" && <DesignMode text={text} setText={setText} definition={definition} valid={valid} pending={pending} />}
      {mode === "code" && <CodeMode text={text} setText={setText} validation={validation} valid={valid} pending={pending} definition={definition} editing={props.editing} name={name} />}
      {mode === "config" && (
        <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_22.5rem]">
          <form className="grid min-w-0 gap-6" noValidate onSubmit={(e) => { e.preventDefault(); if (!props.editing) onCreate(); else if (valid) save.mutate(); }}>
            <ConfigMode name={name} setName={setName} cfg={cfg} set={set} taken={taken} where={where} definition={definition ?? undefined} editing={props.editing} tried={tried} />
            <div className="flex flex-wrap items-center justify-end gap-x-4 gap-y-2 border-t pt-5" data-slot="form-footer">
              {tried && problem && <p className="mr-auto text-[13px] text-bad" data-testid="disabled-why">{problem}</p>}
              <Button type="button" variant="outline" className="bg-card" onClick={cancel} data-testid="form-cancel">Cancel</Button>
              <Button type="submit" disabled={create.isPending || save.isPending || unchanged} title={unchanged ? "Nothing has changed since the last save." : undefined} data-testid="form-submit">{create.isPending || save.isPending ? "Working…" : props.editing ? "Save" : "Create"}</Button>
            </div>
          </form>
          <div className="min-w-0 xl:sticky xl:top-6">
            <CallPanel request={request} refusal={refusal} showInput note={props.editing ? undefined : <><span className="font-mono">roleArn</span> is required. With <b className="font-semibold">Create new role</b> it is the role Step Functions creates for this state machine.</>} />
          </div>
        </div>
      )}
      {!props.editing && <RoleConfirm open={confirm} onOpenChange={setConfirm} name={name} definition={definition} pending={create.isPending} onConfirm={() => create.mutate(undefined, { onSettled: () => setConfirm(false) })} />}
      {props.editing && detail && <PublishDialog d={detail} unsaved={dirty} open={publishing} onOpenChange={setPublishing} done={() => { setPublishing(false); go.machine(name, { tab: "versions", done: "published" }); }} />}
    </div>
  );
}

function CodeMode({ text, setText, validation, valid, pending, definition, editing, name }: { text: string; setText: (t: string) => void; validation: Validation | undefined; valid: boolean; pending: boolean; definition: Definition | null; editing: boolean; name: string }) {
  const bad = !valid && !pending;
  const marked = bad ? problemLines(text, validation) : [];
  const further = bad ? furtherProblems(validation) : [];
  const versions = useVersions(name, editing);
  const newest = [...(versions.data ?? [])].sort((x, y) => Number(y.number) - Number(x.number))[0];
  const graph = (
    <section className="rounded-[var(--radius-card)] border bg-card p-6 lg:p-7" data-testid="studio-graph">
      <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Graph</h2>
      {editing && <p className="mt-1 text-[13.5px] text-muted-foreground">Redrawn as you edit.</p>}
      <div className="mt-4">
        {bad ? (
          <Callout tone="note" testId="studio-graph-paused">Rendering of the graph is paused while the definition is invalid. It draws again once the definition is valid.</Callout>
        ) : definition ? (
          <GraphView definition={definition} />
        ) : (
          <Skeleton className="h-40 rounded-[var(--radius-panel)]" />
        )}
      </div>
    </section>
  );
  const editor = (
    <>
      <AslEditor value={text} onChange={setText} marked={marked} testId="asl-editor" />
      {further.length > 0 && (
        <ul className="mt-4 grid gap-1 text-[13px]" data-testid="studio-diagnostics">
          {further.map((d, i) => (
            <li key={i} className="flex gap-2"><span className={d.severity === "ERROR" ? "font-semibold text-bad" : "font-semibold text-warn"}>{d.severity === "ERROR" ? "Error" : "Warning"}</span><span className="min-w-0 break-words">{wordsOf(d)}</span></li>
          ))}
        </ul>
      )}
    </>
  );
  return (
    <div className="grid gap-6" data-testid="studio-code">
      {bad && <Callout tone="bad" lead="The definition is invalid." testId="studio-invalid">You can't switch modes while it is. Fix the definition to switch modes.</Callout>}
      {editing ? (
        <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_22rem]">
          <div className="min-w-0">{editor}</div>
          <div className="grid min-w-0 gap-5">
            {graph}
            <p className="border-l-2 border-input pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="studio-edit-note">
              <b className="font-medium text-foreground">Save</b> calls UpdateStateMachine. On AWS, executions started in the next few seconds may still use the previous definition; doze-aws applies it at once. <b className="font-medium text-foreground">Publish version</b> snapshots the saved revision as a new version.{newest ? <> <b className="font-medium text-foreground">Version {newest.number}</b> is the newest, and keeps the definition it was published with.</> : null}
            </p>
          </div>
        </div>
      ) : (
        <>
          {editor}
          {graph}
        </>
      )}
    </div>
  );
}

/** Confirm role creation (stepfunctions__machine-create-role): what is about to be made, and a look at the role before it is. */
function RoleConfirm({ open, onOpenChange, name, definition, pending, onConfirm }: { open: boolean; onOpenChange: (o: boolean) => void; name: string; definition: Definition | null; pending: boolean; onConfirm: () => void }) {
  const [view, setView] = useState(false);
  const where = useWhere();
  const policy = permissionsOf(definition ?? undefined, where);
  return (
    <Dialog open={open} onOpenChange={(o) => { onOpenChange(o); if (!o) setView(false); }}>
      <DialogContent className="sm:max-w-[600px]" data-testid="role-dialog">
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">Confirm role creation</DialogTitle>
          <DialogDescription className="text-[14px] leading-[22px] text-foreground">
            Step Functions creates a new IAM role for <b className="font-mono font-semibold text-bright">{name}</b> and then creates the state machine.
          </DialogDescription>
        </DialogHeader>
        <Callout tone="warn" testId="role-warning">If you delete this role, Step Functions can't recreate it. If you change it, for example by removing Step Functions from its principals, Step Functions can't restore it.</Callout>
        {view && (
          <div className="grid gap-3" data-testid="role-settings">
            <p className="text-[13px] text-muted-foreground"><span className="font-mono text-foreground">{roleNameOf(name)}</span> ({madeRoleArn(name, where)}) trusts Step Functions:</p>
            <CodeBlock text={pretty(TRUST)} max="max-h-48" />
            {policy ? <CodeBlock text={pretty(policy)} max="max-h-60" /> : <p className="text-[13px] text-muted-foreground">The definition has no task that calls a service, so it has no permissions policy.</p>}
          </div>
        )}
        <DialogFooter className="sm:justify-between">
          <Button type="button" variant="outline" className="bg-card" onClick={() => setView(!view)} data-testid="role-view">{view ? "Hide role settings" : "View role settings"}</Button>
          <Button type="button" disabled={pending} onClick={onConfirm} data-testid="role-confirm">{pending ? "Creating…" : "Confirm"}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
