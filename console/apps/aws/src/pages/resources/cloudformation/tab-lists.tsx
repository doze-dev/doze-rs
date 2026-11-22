// A stack's Resources, Outputs and Parameters tabs (cloudformation__stack-resources, stack-outputs, stack-parameters, and the
// outputs-none and parameters-none states of a stack that has none).
import { BracesIcon, LogOutIcon, SlidersHorizontalIcon } from "lucide-react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { RuleHelp } from "../../../kit";
import { Loaded } from "../deep/parts";
import { ResourceTo } from "../value";
import { nameList, resourceTarget, type Stack, type StackResource } from "./model";
import { useResources, useTemplate, templateText } from "./api";
import { retainedIn } from "./model";
import { Section, Status } from "./parts";

const mono = "font-mono text-[13px]";

const RESOURCE_COLUMNS: DataColumn<StackResource>[] = [
  { id: "l", header: "Logical ID", size: 230, accessorKey: "LogicalResourceId", cell: (c) => <span className={`${mono} font-medium text-bright`}>{c.row.original.LogicalResourceId}</span> },
  {
    id: "p",
    header: "Physical ID",
    size: 360,
    min: 200,
    accessorFn: (r) => r.PhysicalResourceId ?? "",
    cell: (c) => {
      const to = resourceTarget(c.row.original);
      const id = c.row.original.PhysicalResourceId;
      if (!id) return <span className="text-muted-foreground">–</span>;
      return to ? <ResourceTo to={to} className={`${mono} [overflow-wrap:anywhere] whitespace-normal`}>{id}</ResourceTo> : <span className={`${mono} [overflow-wrap:anywhere] whitespace-normal`}>{id}</span>;
    },
  },
  { id: "t", header: "Type", size: 250, accessorKey: "ResourceType", cell: (c) => <span className={mono}>{c.row.original.ResourceType}</span> },
  { id: "s", header: "Status", size: 220, accessorKey: "ResourceStatus", cell: (c) => <Status status={c.row.original.ResourceStatus} /> },
];

export function ResourcesTab({ name }: { name: string }) {
  const q = useResources(name);
  const template = useTemplate(name);
  const kept = retainedIn(templateText(template.data?.TemplateBody));
  return (
    <Section title="Resources" lede={`What this stack owns. Deleting the stack deletes them${kept.length > 0 ? `, except ${nameList(kept.map((k) => k.id))}, which ${kept.length === 1 ? "has" : "have"} DeletionPolicy Retain` : ""}.`} testId="stack-resources">
      <Loaded q={q} what="The stack's resources">
        {(p) => ((p.StackResourceSummaries ?? []).length === 0 ? <EmptyTab icon={BracesIcon} title="No resources" testId="resources-none">This stack owns no resources.</EmptyTab> : <DataTable aria-label="Stack resources" data={p.StackResourceSummaries ?? []} columns={RESOURCE_COLUMNS} rowId={(r) => r.LogicalResourceId} rowHeight={52} maxHeight={560} />)}
      </Loaded>
      <RuleHelp className="max-w-[56rem]">A resource of type <span className={mono}>AWS::CloudFormation::Stack</span> is a nested stack, listed here with the others. A resource with DeletionPolicy Retain is left in place when the stack is deleted. The physical ID is the identifier CloudFormation gives back for the resource: a queue's URL, a topic's ARN, a table's or bucket's name.</RuleHelp>
    </Section>
  );
}

export function OutputsTab({ stack, importers }: { stack: Stack; importers: Readonly<Record<string, readonly string[]>> | undefined }) {
  const outputs = stack.Outputs ?? [];
  if (outputs.length === 0) return <EmptyTab icon={LogOutIcon} title="No outputs" testId="outputs-none">This stack's template declares no outputs.</EmptyTab>;
  const exported = outputs.filter((o) => o.ExportName);
  const users = [...new Set(Object.values(importers ?? {}).flat())];
  const columns: DataColumn<(typeof outputs)[number]>[] = [
    { id: "k", header: "Key", size: 220, accessorKey: "OutputKey", cell: (c) => <span className={`${mono} font-medium text-bright`}>{c.row.original.OutputKey}</span> },
    { id: "v", header: "Value", size: 400, min: 200, accessorFn: (r) => r.OutputValue ?? "", cell: (c) => <span className={`${mono} [overflow-wrap:anywhere] whitespace-normal`}>{c.row.original.OutputValue}</span> },
    { id: "d", header: "Description", size: 260, accessorFn: (r) => r.Description ?? "", cell: (c) => <span className="whitespace-normal">{c.row.original.Description ?? <span className="text-muted-foreground">–</span>}</span> },
    { id: "e", header: "Export name", accessorFn: (r) => r.ExportName ?? "", cell: (c) => (c.row.original.ExportName ? <ResourceTo to={{ service: "cloudformation", kind: "exports", name: c.row.original.ExportName }} className={`${mono} [overflow-wrap:anywhere] whitespace-normal`}>{c.row.original.ExportName}</ResourceTo> : <span className="text-muted-foreground">Not exported</span>) },
  ];
  return (
    <Section title="Outputs" lede="Values the template declares in its Outputs section. An output with an export name is visible to other stacks." testId="stack-outputs">
      <DataTable aria-label="Stack outputs" data={outputs} columns={columns} rowId={(r) => r.OutputKey} rowHeight={52} maxHeight={560} />
      <RuleHelp className="max-w-[56rem]" >
        Export names are unique per account and Region. Once another stack imports one, you can't delete this stack or change that output until the import is gone.{" "}
        {exported.length === 0 ? "None of these outputs is exported." : importers === undefined ? "" : users.length === 0 ? `No stack imports ${exported.length === 1 ? "this" : "these"} ${exported.length === 1 ? "one" : exported.length} today.` : `${nameList(users)} ${users.length === 1 ? "imports" : "import"} ${nameList(Object.entries(importers).filter(([, v]) => v.length > 0).map(([k]) => k))}.`}
      </RuleHelp>
    </Section>
  );
}

export function ParametersTab({ stack }: { stack: Stack }) {
  // By key, as the board lists them (DescribeStacks answers in the template's order).
  const params = [...(stack.Parameters ?? [])].sort((a, b) => a.ParameterKey.localeCompare(b.ParameterKey));
  if (params.length === 0) return <EmptyTab icon={SlidersHorizontalIcon} title="No parameters" testId="parameters-none">This stack's template declares no parameters.</EmptyTab>;
  const hidden = params.filter((p) => p.ParameterValue === "****").map((p) => p.ParameterKey);
  const columns: DataColumn<(typeof params)[number]>[] = [
    { id: "k", header: "Key", size: 280, accessorKey: "ParameterKey", cell: (c) => <span className={`${mono} font-medium text-bright`}>{c.row.original.ParameterKey}</span> },
    { id: "v", header: "Value", accessorFn: (r) => r.ParameterValue ?? "", cell: (c) => <span className={`${mono} [overflow-wrap:anywhere] whitespace-normal`}>{c.row.original.ParameterValue}</span> },
  ];
  return (
    <Section title="Parameters" lede="The values the stack was created with, as the template's parameters received them." testId="stack-parameters">
      <DataTable aria-label="Stack parameters" data={params} columns={columns} rowId={(r) => r.ParameterKey} rowHeight={48} maxHeight={560} />
      {hidden.length > 0 && <RuleHelp className="max-w-[56rem]"><b>{nameList(hidden)}</b> {hidden.length === 1 ? "is" : "are"} declared with NoEcho, which marks {hidden.length === 1 ? "it" : "them"} to be hidden in logs and consoles, so this page shows {hidden.length === 1 ? "it" : "them"} masked. On the next update the field starts empty and you tick Use existing value to keep it.</RuleHelp>}
    </Section>
  );
}
