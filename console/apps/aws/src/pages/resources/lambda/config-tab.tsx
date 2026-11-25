// A function's Configuration tab (lambda__function-config, function-triggers, function-permissions, function-url, function-env
// and the boards of their empty states): the section nav (General configuration, Triggers, Permissions, Function URL, Environment
// variables) and the section chosen, each with the button that opens its form.
import { LinkIcon, PencilIcon, PlusIcon, ShieldIcon, Trash2Icon, WorkflowIcon, VariableIcon } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { cn } from "@doze/ui/lib/utils";
import { useAbout } from "../../../api/queries";
import { useResourceList } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { Card } from "../deep/parts";
import { type Go, usePolicy, useWhere } from "./hooks";
import { type Fn, type Trigger, urlAuthNote } from "./model";
import { PolicyTable } from "./policy-table";
import { useRemove } from "./remove";

export const SECTIONS = [
  ["general", "General configuration"],
  ["triggers", "Triggers"],
  ["permissions", "Permissions"],
  ["url", "Function URL"],
  ["env", "Environment variables"],
] as const;

function Row({ k, v, sub, mono = false }: { k: string; v: React.ReactNode; sub?: React.ReactNode; mono?: boolean }) {
  return (
    <div className="contents">
      <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{k}</dt>
      <dd className={cn("min-w-0 border-b py-3 text-[13.5px] break-words text-bright", mono && "font-mono text-[13px]")}>
        {v}
        {sub && <div className="mt-0.5 text-[12.5px] font-normal text-muted-foreground">{sub}</div>}
      </dd>
    </div>
  );
}
const Rows = ({ children }: { children: React.ReactNode }) => <dl className="grid grid-cols-[minmax(7rem,12rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0">{children}</dl>;
const edit = (label: string, onClick: () => void, testId: string, icon = <PencilIcon />, disabled = false) => (
  <Button type="button" variant="outline" className="bg-card" onClick={onClick} disabled={disabled} data-testid={testId}>{icon} {label}</Button>
);

export function ConfigTab({ d, f, section, sel, go, triggers }: { d: ResourceDetail; f: Fn; section: string; sel?: string; go: (to: Go) => void; triggers: readonly Trigger[] }) {
  const current = SECTIONS.some(([id]) => id === section) ? section : "general";
  const to = (id: string) => go({ tab: "configuration", p: id });
  return (
    <div className="grid gap-6" data-testid="config-tab">
      <nav aria-label="Configuration sections" className="flex flex-wrap gap-3" data-testid="config-nav">
        {SECTIONS.map(([id, label]) => (
          <button key={id} type="button" aria-current={id === current ? "page" : undefined} onClick={() => to(id)} className={cn("rounded-full border px-4 py-2 text-[14px] outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50", id === current ? "border-primary/30 bg-pick font-medium text-bright" : "bg-card text-foreground hover:bg-accent")} data-testid={`section-${id}`}>
            {label}
          </button>
        ))}
      </nav>
      {current === "general" ? <General f={f} go={go} /> : current === "triggers" ? <Triggers d={d} sel={sel} go={go} triggers={triggers} /> : current === "permissions" ? <Permissions d={d} f={f} go={go} /> : current === "url" ? <FunctionUrl d={d} f={f} go={go} /> : <Env d={d} f={f} go={go} />}
    </div>
  );
}

function General({ f, go }: { f: Fn; go: (to: Go) => void }) {
  return (
    <Card title="General configuration" lede="The settings that describe how the function runs." actions={edit("Edit", () => go({ tab: "configuration", p: "general", view: "config-edit" }), "config-edit")} testId="general-card">
      <Rows>
        <Row k="Description" v={f.description || <span className="text-muted-foreground">–</span>} />
        <Row k="Memory" v={`${f.memory} MB`} />
        <Row k="Ephemeral storage" v={`${f.ephemeral} MB`} />
        <Row k="Timeout" v={`${f.timeout} ${f.timeout === 1 ? "second" : "seconds"}`} />
        <Row k="Execution role" v={<Link to="/resources/$service/$kind/$name" params={{ service: "iam", kind: "roles", name: f.roleName }} className="text-link hover:underline">{f.roleName}</Link>} sub={<span className="font-mono">{f.role}</span>} />
      </Rows>
      <p className="mt-4 max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-foreground" data-testid="general-note">
        Memory and ephemeral storage are stored and read back as AWS does. <span className="text-muted-foreground">Stored, not applied locally: the function runs as a process on this machine, with no memory or /tmp limit. The execution role is applied: the process gets that role&apos;s credentials, so IAM judges what the function calls.</span>
      </p>
    </Card>
  );
}

function Triggers({ d, sel, go, triggers }: { d: ResourceDetail; sel?: string; go: (to: Go) => void; triggers: readonly Trigger[] }) {
  const mappings = triggers.filter((t) => t.how === "mapping");
  // The page opens with the mapping selected (the board's), and Edit and Delete act on the one that is.
  const chosen = mappings.find((m) => m.id === sel) ?? (sel === undefined ? mappings[0] : undefined);
  const rm = useRemove(d.name, "DeleteEventSourceMapping", "trigger-delete-dialog");
  const columns: DataColumn<Trigger>[] = [
    { id: "trigger", header: "Trigger", size: 460, min: 260, accessorFn: (t) => t.name, cell: (c) => <span className="grid leading-5"><span className="font-mono text-[13.5px] font-semibold text-bright">{c.row.original.name}</span><span className="truncate font-mono text-[12px] text-muted-foreground">{c.row.original.source}</span></span> },
    { id: "state", header: "State", size: 130, accessorFn: (t) => (t.enabled ? 1 : 0), cell: (c) => <StatusDot tone={c.row.original.enabled ? "ok" : "idle"}>{c.row.original.enabled ? "Enabled" : "Disabled"}</StatusDot> },
    { id: "details", header: "Details", accessorFn: (t) => t.batch ?? 0, cell: (c) => <span className="grid leading-5"><span className="text-[13.5px]">{c.row.original.kind === "SQS queue" ? "Amazon SQS" : c.row.original.kind} event source mapping, batch size {c.row.original.batch}</span><span className="truncate font-mono text-[12px] text-muted-foreground">UUID {c.row.original.id}</span></span> },
  ];
  const add = <Button type="button" onClick={() => go({ tab: "configuration", p: "triggers", view: "trigger-add" })} data-testid="trigger-add"><PlusIcon /> Add trigger</Button>;
  const policyOnes = triggers.filter((t) => t.how === "policy");
  return (
    <Card title="Triggers" lede="Each trigger invokes the function on its own; an event carries data from one trigger only." actions={mappings.length > 0 ? <>{edit("Edit", () => chosen && go({ tab: "configuration", p: "triggers", view: "trigger-edit", sel: chosen.id }), "trigger-edit", undefined, !chosen)}<Button type="button" variant="outline" className="bg-card" disabled={!chosen} onClick={() => chosen && rm.ask({ title: "Delete trigger", input: { UUID: chosen.id }, text: <>Delete the trigger <b className="font-mono font-semibold text-bright">{chosen.name}</b>? {d.name} is no longer invoked from it. The source keeps what it holds; only the event source mapping goes.</>, then: () => go({ tab: "configuration", p: "triggers", done: "trigger-deleted" }) })} data-testid="trigger-delete"><Trash2Icon /> Delete</Button>{add}</> : undefined} testId="triggers-card">
      {mappings.length === 0 ? (
        <EmptyTab icon={WorkflowIcon} title="No triggers" action={<Button type="button" variant="outline" className="bg-card" onClick={() => go({ tab: "configuration", p: "triggers", view: "trigger-add" })} data-testid="trigger-add-empty">Add trigger</Button>} testId="triggers-none">
          Nothing invokes {d.name} automatically. Add a trigger such as a queue, a stream or a schedule.
        </EmptyTab>
      ) : (
        <>
          <DataTable aria-label="Triggers" data={mappings} columns={columns} rowId={(t) => t.id ?? t.name} rowHeight={56} maxHeight={360} onRowClick={(t) => go({ tab: "configuration", p: "triggers", sel: t.id })} rowClassName={(t) => (t.id === chosen?.id ? "bg-pick/60 shadow-[inset_3px_0_0_var(--primary)]" : "cursor-pointer")} />
          <p className="mt-3 text-[13px] text-muted-foreground" data-testid="triggers-hint">The list shows Lambda event source mappings (ListEventSourceMappings). Select a trigger, then choose Edit or Delete.</p>
        </>
      )}
      {rm.dialog}
      <p className="mt-4 max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-foreground" data-testid="triggers-teach">
        S3, SNS and API Gateway triggers are stored and managed by the service that generates the events, not by Lambda; when you create one in the console, Lambda adds the permission to the function&apos;s resource-based policy.{" "}
        <span className="text-muted-foreground">
          {policyOnes.length > 0 ? <>The <b className="font-semibold text-foreground">{policyOnes[0]?.name}</b> {policyOnes[0]?.kind === "SNS topic" ? "topic" : "source"} is such a trigger: its statement is on the Permissions page. </> : null}
          SQS mappings are Lambda resources: Lambda polls the queue and invokes the function with batches.
        </span>
      </p>
    </Card>
  );
}

function Permissions({ d, f, go }: { d: ResourceDetail; f: Fn; go: (to: Go) => void }) {
  const policy = usePolicy(d.name);
  const rm = useRemove(d.name, "RemovePermission", "permission-delete-dialog");
  const add = <Button type="button" onClick={() => go({ tab: "configuration", p: "permissions", view: "permission-add" })} data-testid="permission-add"><PlusIcon /> Add permissions</Button>;
  return (
    <div className="grid gap-6" data-testid="permissions-section">
      <Card title="Execution role" lede="The role Lambda assumes to run the function." testId="role-card">
        <Rows>
          <Row k="Role name" v={<Link to="/resources/$service/$kind/$name" params={{ service: "iam", kind: "roles", name: f.roleName }} className="font-mono text-link hover:underline" data-testid="role-link">{f.roleName}</Link>} sub={<span className="font-mono">{f.role}</span>} />
        </Rows>
      </Card>
      <Card title="Resource-based policy statements" lede="Who may call the function. The console adds a statement when you create a trigger for an AWS service." actions={policy.statements.length > 0 ? add : undefined} testId="policy-card">
        {policy.isPending ? (
          <Skeleton className="h-24 rounded-[var(--radius-panel)]" />
        ) : policy.statements.length === 0 ? (
          <EmptyTab icon={ShieldIcon} title="No resource-based policy statements" action={<Button type="button" variant="outline" className="bg-card" onClick={() => go({ tab: "configuration", p: "permissions", view: "permission-add" })}>Add permissions</Button>} testId="policy-none">No other service or account has been given permission to invoke {d.name}.</EmptyTab>
        ) : (
          <>
            <PolicyTable statements={policy.statements} onDelete={(sid) => rm.ask({ title: "Delete permission statement", input: { StatementId: sid }, text: <>Delete the statement <b className="font-mono font-semibold text-bright">{sid}</b> from the resource-based policy of {d.name}? What it let in can no longer invoke the function.</>, then: () => go({ tab: "configuration", p: "permissions", done: "permission-deleted" }) })} />
            <p className="mt-3 text-[13px] text-muted-foreground">The columns are the elements of the policy statements that GetPolicy returns.</p>
          </>
        )}
        <p className="mt-4 max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-foreground" data-testid="policy-teach">
          Individual permissions are Allow statements, and support the conditions aws:SourceArn, aws:SourceAccount and aws:PrincipalOrgID.{" "}
          <span className="text-muted-foreground">When IAM is in soft or enforce mode, doze-aws checks this policy on every request that names the function.</span>
        </p>
      </Card>
      {rm.dialog}
    </div>
  );
}

function FunctionUrl({ d, f, go }: { d: ResourceDetail; f: Fn; go: (to: Go) => void }) {
  const about = useAbout().data;
  const { region } = useWhere();
  const id = f.url?.match(/^https?:\/\/([^.]+)\.lambda-url\./)?.[1] ?? null;
  const rm = useRemove(d.name, "DeleteFunctionUrlConfig", "url-delete-dialog");
  const also = id && about ? `${about.endpoint}/_aws/lambda-url/${region}/${id}/` : null;
  return (
    <Card title="Function URL" lede="A function URL is a dedicated HTTP(S) endpoint for the function." testId="url-card">
      {!f.url ? (
        <EmptyTab icon={LinkIcon} title="No function URL" action={<Button type="button" onClick={() => go({ tab: "configuration", p: "url", view: "url-create" })} data-testid="url-create">Create function URL</Button>} testId="url-none">
          {d.name} has no URL. A URL can be added to $LATEST or to an alias, but not to another version.
        </EmptyTab>
      ) : (
        <>
          <Rows>
            <Row k="Function URL" v={f.url} mono />
            {also && <Row k="Also at" v={also} mono sub="The doze-aws endpoint, for clients that cannot set the Host header." />}
            <Row k="Auth type" v={f.urlAuth ?? "–"} />
            <Row k="Applies to" v="$LATEST, the unpublished version" />
            {f.cors?.AllowOrigins && <Row k="AllowOrigins" v={f.cors.AllowOrigins.join(", ")} mono />}
            {f.cors?.AllowMethods && <Row k="AllowMethods" v={f.cors.AllowMethods.join(", ")} mono />}
            {f.cors?.AllowHeaders && <Row k="AllowHeaders" v={f.cors.AllowHeaders.join(", ")} mono />}
            {f.cors?.MaxAge !== undefined && <Row k="MaxAge" v={`${f.cors.MaxAge} ${f.cors.MaxAge === 1 ? "second" : "seconds"}`} />}
          </Rows>
          <div className="mt-5 flex flex-wrap gap-3"><CopyButton text={f.url} label="Copy URL" what="URL copied" className="bg-card" /><Button type="button" variant="outline" className="bg-card" onClick={() => rm.ask({ title: "Delete function URL", input: {}, text: <>Delete the function URL of {d.name}? <span className="font-mono">{f.url}</span> stops answering, and a new URL gets another address.</>, then: () => go({ tab: "configuration", p: "url", done: "url-deleted" }) })} data-testid="url-delete"><Trash2Icon /> Delete function URL</Button></div>
          {rm.dialog}
          <p className="mt-4 max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-foreground" data-testid="url-teach">
            Once you create a function URL, its URL endpoint never changes. Any HTTP client can invoke the function through it. <span className="text-muted-foreground">{urlAuthNote(f.urlAuth)}</span>
          </p>
        </>
      )}
    </Card>
  );
}

function Env({ d, f, go }: { d: ResourceDetail; f: Fn; go: (to: Go) => void }) {
  const secrets = useResourceList("secretsmanager", "secrets");
  const example = secrets.data?.pages.flatMap((p) => p.rows)[0]?.name;
  const entries = Object.entries(f.env);
  const open = () => go({ tab: "configuration", p: "env", view: "env-edit" });
  return (
    <Card title="Environment variables" lede="Pairs of strings your code reads at run time. Values are literal: they are not expanded." actions={entries.length > 0 ? edit("Edit", open, "env-edit") : undefined} testId="env-card">
      {entries.length === 0 ? (
        <EmptyTab icon={VariableIcon} title="No environment variables" action={edit("Edit", open, "env-edit")} testId="env-none">{d.name} has no environment variables.</EmptyTab>
      ) : (
        <>
          <table className="w-full text-[13.5px]" data-testid="env-table">
            <thead><tr className="text-left text-[12px] tracking-[0.06em] text-muted-foreground uppercase"><th className="rounded-l-lg bg-well px-4 py-2.5 font-medium">Key</th><th className="rounded-r-lg bg-well px-4 py-2.5 font-medium">Value</th></tr></thead>
            <tbody>{entries.map(([k, v]) => <tr key={k} className="border-b last:border-b-0"><td className="px-4 py-3 font-mono text-[13px] font-medium text-bright" data-testid="env-key">{k}</td><td className="px-4 py-3 font-mono text-[13px] break-all" data-testid="env-value">{v}</td></tr>)}</tbody>
          </table>
        </>
      )}
      <p className="mt-4 max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-foreground" data-testid="env-teach">
        They are stored with the unpublished version and locked when you publish. <span className="text-muted-foreground">Keep secrets in Secrets Manager{example ? <>, such as <span className="font-mono">{example}</span>,</> : ""} rather than here.</span>
      </p>
    </Card>
  );
}
