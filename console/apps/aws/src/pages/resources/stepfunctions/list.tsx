// Step Functions' list of state machines (stepfunctions__machines-list, machines-none, machine-create-name, and the list a delete
// lands on): name, type, creation date and ARN, a sentence of what is running, Activities and Create state machine. Create
// state machine asks for the name in a dialog, then opens Workflow Studio.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { FilterIcon, ListIcon, PlusIcon, WorkflowIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useAbout } from "../../../api/queries";
import { useResourceList } from "../../../api/resources";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { Field, RadioCards } from "../../../kit";
import { HINT } from "../index-page";
import { ResourceTo, Value } from "../value";
import { useGoTo, useRunning } from "./hooks";
import { existingWords, listSentence, nameProblem } from "./model";

const isExpress = (r: ResourceRow) => (r.cells[0] ?? "").toUpperCase() === "EXPRESS";
const typeWord = (r: ResourceRow) => (isExpress(r) ? "Express" : "Standard");

const COLUMNS: DataColumn<ResourceRow>[] = [
  { id: "name", header: "Name", size: 280, min: 170, accessorFn: (r) => r.name.toLowerCase(), meta: { mono: true }, cell: (c) => <ResourceTo to={{ service: "stepfunctions", kind: "machines", name: c.row.original.name }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.name}</ResourceTo> },
  { id: "type", header: "Type", size: 120, fit: 56, accessorFn: typeWord },
  { id: "created", header: "Creation date", size: 210, fit: 112, accessorFn: (r) => r.cells[1] ?? "", cell: (c) => <Value value={c.row.original.cells[1] ?? null} format="time" /> },
  { id: "arn", header: "ARN", min: 280, accessorFn: (r) => r.id, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.id}</span> },
];

export function MachinesList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { gone?: string };
  return <Machines service={service} kind={kind} gone={search.gone} />;
}

function Machines({ service, kind, gone }: { service: string; kind: string; gone?: string }) {
  const go = useGoTo();
  const about = useAbout().data;
  const list = useResourceList(service, kind);
  const [type, setType] = useState("any");
  const [naming, setNaming] = useState(false);
  const { refetch } = list;
  const first = list.data?.pages[0];
  const all = useMemo(() => list.data?.pages.flatMap((p) => p.rows) ?? [], [list.data]);
  const running = useRunning(all.map((r) => ({ name: r.name, express: isExpress(r) })), !gone);
  const refresh = useCallback(() => {
    void refetch();
    void running.refetch();
  }, [refetch, running]);
  const rows = useMemo(() => (type === "any" ? all : all.filter((r) => typeWord(r).toLowerCase() === type)), [all, type]);
  const region = about?.region ?? "this Region";
  const none = all.length === 0;
  const sentence = running.isError || (running.isPending && all.some((r) => !isExpress(r))) ? null : listSentence(all.map((r) => ({ name: r.name, express: isExpress(r) })), running.data ?? {});
  const create = <Button onClick={() => setNaming(true)} data-testid="create-open"><PlusIcon /> Create state machine</Button>;
  return (
    <>
      {gone && <ResultBanner className="mb-6" title={`State machine ${gone} deleted.`} returned="DeleteStateMachine answered, and the machine is gone from the list." left="Its versions and aliases went with it; executions that were running carry on with the definition they started with." onDismiss={() => go.list()} testId="deleted-banner" />}
      <PageHeader
        title="State machines"
        crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.group, to: "/resources" }, { label: first.service_title, to: `/resources/${service}` }] : [])]}
        sub={none ? `Standard and Express workflows in ${region}.` : `Standard and Express workflows in ${region}. Choose a name to open it.`}
        refresh={<RefreshControl updatedAt={list.dataUpdatedAt} fetching={list.isFetching} onRefresh={refresh} interval={false} />}
        actions={
          <>
            <Button variant="outline" className="bg-card" onClick={() => go.activities()} data-testid="activities-open"><ListIcon /> Activities</Button>
            {create}
          </>
        }
      />
      {list.isError && !first ? (
        <ErrorBlock what="The state machines could not be listed" error={list.error} hint={HINT} onRetry={refresh} retrying={list.isFetching} testId="list-error" />
      ) : !first ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : none ? (
        <div data-testid="list-empty">
          <EmptyState icon={WorkflowIcon} title="No state machines" action={<Button variant="outline" className="bg-card" onClick={() => setNaming(true)} data-testid="create-empty"><PlusIcon /> Create state machine</Button>}>
            You have no state machines in this region. Create one to orchestrate steps across services.
          </EmptyState>
        </div>
      ) : (
        <>
          {sentence && <p className="mb-5 max-w-[62rem] text-[15px] leading-6 text-muted-foreground" data-testid="list-sentence"><Emphasis text={sentence} names={all.map((r) => r.name)} bold={[/\b\d+ (?=executions? running)/, /\bExpress\b/]} /></p>}
          <div className="mb-4 flex flex-wrap items-center gap-3">
            <Select value={type} onValueChange={setType}>
              <SelectTrigger className="w-44 bg-card" aria-label="Type" data-testid="type-filter"><FilterIcon className="size-4 text-muted-foreground" /><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="any">Any type</SelectItem>
                <SelectItem value="standard">Standard</SelectItem>
                <SelectItem value="express">Express</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <DataTable aria-label="State machines" data={rows} columns={COLUMNS} rowId={(r) => r.name} maxHeight={680} empty={`No ${type === "express" ? "Express" : "Standard"} state machine.`} />
          <Pagination className="mt-3" shown={rows.length} total={rows.length} more={list.hasNextPage} onMore={() => void list.fetchNextPage()} loading={list.isFetchingNextPage} noun={rows.length === 1 ? "state machine" : "state machines"} />
        </>
      )}
      <NameDialog open={naming} onOpenChange={setNaming} taken={all.map((r) => r.name)} />
    </>
  );
}

/** Create state machine: the name, then Workflow Studio (stepfunctions__machine-create-name). Nothing exists until Create there. */
function NameDialog({ open, onOpenChange, taken }: { open: boolean; onOpenChange: (o: boolean) => void; taken: string[] }) {
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const [tried, setTried] = useState(false);
  const problem = nameProblem(name, taken);
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    setTried(true);
    if (problem) return;
    onOpenChange(false);
    void navigate({ to: "/resources/$service/$second", params: { service: "stepfunctions", second: "machines" }, search: { do: "create", name } });
  };
  return (
    <Dialog open={open} onOpenChange={(o) => { onOpenChange(o); if (!o) { setName(""); setTried(false); } }}>
      <DialogContent className="sm:max-w-[600px]" data-testid="name-dialog">
        <form onSubmit={submit} noValidate className="grid gap-5">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Create state machine</DialogTitle>
            <DialogDescription className="text-[14px] leading-[22px] text-foreground">
              Start from a blank canvas and name it. You build the workflow in Workflow Studio, and nothing exists until you choose <b className="font-semibold">Create</b> there.
            </DialogDescription>
          </DialogHeader>
          <RadioCards value="blank" onChange={() => undefined} label="How to start" name="start" options={[{ value: "blank", title: "Create from blank", body: "Design the workflow on a canvas, or write the Amazon States Language yourself." }]} />
          <Field label="State machine name" error={tried && problem && !problem.endsWith("already exists in this Region.") ? problem : undefined} refusal={tried && problem?.endsWith("already exists in this Region.") ? { why: problem } : null} help={<>1 to 80 characters. No whitespace, wildcard characters ( ? * ), brackets ( &lt; &gt; &#123; &#125; [ ] ) or special characters ( &quot; # % \ ^ | ~ ` $ &amp; , ; : / ). <b className="font-semibold">You can't rename it after you create it.</b></>} hint={existingWords(taken.slice(0, 4))}>
            {(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} autoFocus autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="name-input" />}
          </Field>
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>Cancel</Button>
            <Button type="submit" data-testid="name-continue">Continue</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

