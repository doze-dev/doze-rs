// KMS's keys (kms__keys-list, keys-none, aws-managed-keys, keys-deletion-scheduled): the customer managed keys and, under their own
// heading, the keys AWS services made for the account. Choose keys, then Key actions; Create key opens the wizard
// (`?create=yes`) and Schedule key deletion its own page (`?do=schedule`).
import { useNavigate, useSearch } from "@tanstack/react-router";
import { ChevronDownIcon, KeyRoundIcon, PlusIcon, SearchIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { Alert, ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { useAbout } from "../../../api/queries";
import { useAllRows, useKeyUses } from "../cfg-api";
import { Say } from "../cfg-parts";
import { HINT } from "../index-page";
import { ResourceTo, Value } from "../value";
import { KeyCreate } from "./create";
import { ScheduleDeletion } from "./schedule";
import { type Done, useKeyActions } from "./key-actions";
import { actionWords, aliasWords, awsSentence, countSentence, encryptsSentence, keyName, keyRows, type KeyAction, type KeyRow, pendingSentence, refsOf, usedBy, whyNot } from "./model";
import { crumbs, stateOf } from "./parts";

const ACTIONS: { id: KeyAction; label: string }[] = [
  { id: "enable", label: "Enable" },
  { id: "disable", label: "Disable" },
  { id: "schedule", label: "Schedule key deletion" },
  { id: "cancel", label: "Cancel key deletion" },
];

export function KmsList() {
  const search = useSearch({ strict: false }) as { mode?: string; q?: string; create?: string; do?: string; sel?: string };
  if (search.create === "yes") return <KeyCreate />;
  if (search.do === "schedule") return <ScheduleDeletion ids={(search.sel ?? "").split(",").filter(Boolean)} />;
  return <Keys aws={search.mode === "aws"} scheduled={search.do === "scheduled" ? (search.sel ?? "").split(",").filter(Boolean) : []} />;
}

const WORDS = {
  customer: { title: "Customer managed keys", noun: "customer managed key", sub: (region: string) => `The KMS keys you create and manage in ${region}. Choose an alias or key ID to open a key. Select a key, then use Key actions.` },
  aws: { title: "AWS managed keys", noun: "AWS managed key", sub: () => "Keys that AWS services created in your account. You can view them and their key policies. You cannot change their properties, rotate them, change their key policies or schedule them for deletion." },
};

function Keys({ aws, scheduled }: { aws: boolean; scheduled: string[] }) {
  const navigate = useNavigate();
  const region = useAbout().data?.region ?? "this Region";
  const list = useAllRows("kms", "keys", { all: true });
  const aliases = useAllRows("kms", "aliases");
  const { uses } = useKeyUses();
  const { run, pending } = useKeyActions();
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const [typed, setTyped] = useState("");
  const [result, setResult] = useState<Done | null>(null);
  const words = aws ? WORDS.aws : WORDS.customer;
  const keys = useMemo(() => keyRows(list.rows, list.columns, aliases.rows.map((a) => ({ name: a.name, key: a.cells[aliases.columns.findIndex((c) => c.title === "Key")] ?? "" }))), [list.rows, list.columns, aliases.rows, aliases.columns]);
  // By alias, as the console lists them; a key with none comes last.
  const mine = keys.filter((k) => k.managedByAws === aws).sort((a, b) => (a.aliases[0] === undefined ? 1 : 0) - (b.aliases[0] === undefined ? 1 : 0) || (a.aliases[0] ?? a.id).localeCompare(b.aliases[0] ?? b.id));
  const shown = mine.filter((k) => !typed.trim() || [...k.aliases, k.id].some((t) => t.toLowerCase().includes(typed.trim().toLowerCase())));
  const picked = mine.filter((k) => chosen.has(k.id));
  const go = (search: { mode?: string; create?: "yes"; do?: string; sel?: string }) => void navigate({ to: "/resources/$service/$second", params: { service: "kms", second: "keys" }, search });
  const act = async (action: KeyAction) => {
    if (action === "schedule") return go({ do: "schedule", sel: picked.map((k) => k.id).join(",") });
    const done = await run(action, picked.map((k) => k.id));
    setResult(done);
    if (!done.refused) setChosen(new Set());
  };
  const refresh = () => { void list.q.refetch(); void aliases.q.refetch(); };
  const first = list.first;
  const use = (k: KeyRow) => encryptsSentence(`alias/${keyName(k)}`, usedBy(uses, refsOf(k)));
  const sentence = aws
    ? awsSentence(mine)
    : [countSentence(mine, words.noun), pendingSentence(mine, scheduled), mine.filter((k) => k.aliases.length > 0).map(use).find(Boolean) ?? ""].filter(Boolean).join(" ");
  const columns: DataColumn<KeyRow>[] = [
    { id: "alias", header: "Aliases", min: 168, accessorFn: aliasWords, meta: { mono: true }, cell: (c) => (c.row.original.aliases.length > 0 ? <ResourceTo to={{ service: "kms", kind: "keys", name: c.row.original.id }} className="font-mono text-[13px] font-medium" testId="row-link">{aliasWords(c.row.original)}</ResourceTo> : <span className="font-mono text-[13px]">–</span>) },
    { id: "id", header: "Key ID", accessorKey: "id", min: 240, meta: { mono: true }, cell: (c) => (c.row.original.aliases.length > 0 ? <Value value={c.row.original.id} format="mono" /> : <ResourceTo to={{ service: "kms", kind: "keys", name: c.row.original.id }} className="font-mono text-[13px]" testId="row-link">{c.row.original.id}</ResourceTo>) },
    { id: "state", header: "Status", accessorFn: (r) => r.stateWords, size: 152, fit: 70, cell: (c) => stateOf(c.row.original) },
    ...(aws ? [] : ([
      { id: "kind", header: "Key type", accessorKey: "kind", size: 120, fit: 80 },
      { id: "created", header: "Creation date", accessorFn: (r: KeyRow) => r.created ?? "", size: 210, fit: 150, cell: (c) => <Value value={c.row.original.created} format="time" /> },
    ] as DataColumn<KeyRow>[])),
  ];
  const actions = (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button type="button" variant="outline" className="bg-card" disabled={pending} data-testid="key-actions">
            Key actions <ChevronDownIcon />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-52">
          {ACTIONS.map((a) => {
            const why = whyNot(a.id, picked);
            return (
              <DropdownMenuItem key={a.id} disabled={why !== null} title={why ?? undefined} onSelect={() => void act(a.id)} data-testid={`action-${a.id}`}>
                {a.label}
              </DropdownMenuItem>
            );
          })}
        </DropdownMenuContent>
      </DropdownMenu>
      <Button onClick={() => go({ create: "yes" })} data-testid="create-open"><PlusIcon /> Create key</Button>
    </>
  );
  const search = (
    <div className="relative w-full max-w-[22rem]">
      <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
      <Input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Filter keys" aria-label="Filter keys" className="bg-card pl-9 text-[13px]" data-testid="list-search" />
    </div>
  );
  const refused = result?.refused;
  const banner = refused ? (
    <Alert tone="bad" title={`${ACTIONS.find((a) => a.id === result.action)?.label} was refused: ${refused.code}`} testId="action-refused">{refused.message}</Alert>
  ) : result ? (
    <ResultBanner className="mb-0" title={doneTitle(result, keys)} returned={doneReturned(result)} onDismiss={() => setResult(null)} testId="action-done" />
  ) : null;
  const justScheduled = scheduled.map((id) => keyName(keys.find((k) => k.id === id) ?? { id, aliases: [] }));
  return (
    <>
      <PageHeader title={words.title} crumbs={crumbs(first?.group, first?.service_title)} sub={words.sub(region)} refresh={<RefreshControl updatedAt={list.q.dataUpdatedAt} fetching={list.q.isFetching} onRefresh={refresh} interval={false} />} />
      <Tabs value={aws ? "aws" : "customer"} onValueChange={(v) => { setChosen(new Set()); setResult(null); go(v === "aws" ? { mode: "aws" } : {}); }} className="mb-6 gap-0">
        <TabsList variant="line" className="h-11 w-full justify-start gap-2 rounded-none border-b p-0" aria-label="Kinds of key">
          <TabsTrigger value="customer" className="h-11 flex-none px-3 text-[14px] after:bottom-[-1px]" data-testid="tab-customer">Customer managed keys</TabsTrigger>
          <TabsTrigger value="aws" className="h-11 flex-none px-3 text-[14px] after:bottom-[-1px]" data-testid="tab-aws">AWS managed keys</TabsTrigger>
        </TabsList>
      </Tabs>
      {banner && <div className="mb-6">{banner}</div>}
      {list.q.isError && !first ? (
        <ErrorBlock what="The keys could not be listed" error={list.q.error} hint={HINT} onRetry={refresh} retrying={list.q.isFetching} testId="list-error" />
      ) : !first || aliases.loading ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : mine.length === 0 ? (
        <div data-testid="list-empty">
          <EmptyState icon={KeyRoundIcon} title={aws ? "No AWS managed keys" : "No customer managed keys"} action={aws ? undefined : <Button onClick={() => go({ create: "yes" })} data-testid="create-empty"><PlusIcon /> Create key</Button>}>
            {aws ? "AWS services make their keys in your account the first time they need one." : "Create a key to encrypt data with your own KMS key. AWS managed keys are listed under their own heading."}
          </EmptyState>
        </div>
      ) : (
        <>
          <Say text={sentence} names={[...mine.flatMap((k) => k.aliases.map((a) => `alias/${a}`)), ...uses.map((u) => u.name), ...(aws ? ["aws/<service-name>"] : [])]} bold={[/\d+(?= (?:customer managed|AWS managed) keys?)/, /\b\d+ (?:enabled|disabled|pending deletion)/, "all Enabled"]} className="mb-6" testId="list-sentence" />
          {aws ? (
            <div className="grid gap-3">{search}<DataTable aria-label="Keys" data={shown} columns={columns} rowId={(k) => k.id} maxHeight={680} /></div>
          ) : (
            <SelectableTable aria-label="Keys" noun="key" data={shown} columns={columns} rowId={(k) => k.id} selected={chosen} onSelectedChange={(s) => { setChosen(s); setResult(null); }} toolbar={search} actions={() => actions} actionsAlways actionsEnd maxHeight={680} />
          )}
          {justScheduled.length > 0 && !result && <p className="mt-5 max-w-[48rem] border-l-2 pl-3.5 text-[13.5px] leading-[21px] text-muted-foreground" data-testid="scheduled-note">Cancel key deletion puts {justScheduled.join(", ")} back in <b className="font-semibold text-foreground">Disabled</b>, not Enabled; choose Enable to use {justScheduled.length === 1 ? "it" : "them"}.</p>}
          <Note words={aws ? null : actionWords(picked)} aws={aws} />
        </>
      )}
    </>
  );
}

function Note({ words, aws }: { words: string | null; aws: boolean }) {
  return (
    <p className="mt-5 max-w-[48rem] border-l-2 pl-3.5 text-[13.5px] leading-[21px] text-muted-foreground" data-testid="list-note">
      {aws ? "Keys that an AWS service owns in its own account (AWS owned keys) are not in your account, and their metadata cannot be viewed, so they are not listed here. doze-aws creates an AWS managed key the first time SSM or Secrets Manager asks for alias/aws/ssm or alias/aws/secretsmanager, and reports its KeyManager as AWS." : words}
    </p>
  );
}

const VERB: Record<KeyAction, string> = { enable: "enabled", disable: "disabled", schedule: "scheduled for deletion", cancel: "deletion cancelled" };
const CALL: Record<KeyAction, string> = { enable: "EnableKey", disable: "DisableKey", schedule: "ScheduleKeyDeletion", cancel: "CancelKeyDeletion" };

function doneTitle(d: Done, keys: readonly KeyRow[]): string {
  const names = d.ids.map((id) => keyName(keys.find((k) => k.id === id) ?? { id, aliases: [] }));
  return `${names.join(", ")} ${VERB[d.action] === "deletion cancelled" ? "is no longer pending deletion" : `${d.ids.length === 1 ? "is" : "are"} ${VERB[d.action]}`}.`;
}
const doneReturned = (d: Done): string => (d.action === "cancel" ? "CancelKeyDeletion put the key back in Disabled, not Enabled; choose Enable to use it." : `${CALL[d.action]} was made for ${d.ids.length === 1 ? "the key" : `each of the ${d.ids.length} keys`}.`);
