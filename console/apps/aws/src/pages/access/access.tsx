import { Link } from "@tanstack/react-router";
import { ShieldCheckIcon, WandSparklesIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { number, plural } from "@doze/ui/lib/format";
import { useAccess, usePrincipals } from "../../api/spaces";
import type { DenialGroup } from "../../api/types/DenialGroup";
import type { KeyPrincipal } from "../../api/types/KeyPrincipal";
import type { Refusal } from "../../api/types/Refusal";
import { ago, who } from "../../people";
import { CardSkeleton, Section, useNow } from "../card";
import { ResourceTo } from "../resources/value";
import { holderOf } from "./draft";
import { IdentityCards } from "./identity-cards";
import { Modes } from "./modes";
import { PassDialog } from "./pass-dialog";

const HINT = "Check that doze-aws is still running, then try again.";

/** A principal's ARN as a link to its user or role in Resources, when it names one. */
function Holder({ arn }: { arn: string }) {
  const h = holderOf(arn);
  if (!h) return <span className="font-mono text-[12.5px]">{arn}</span>;
  return (
    <ResourceTo to={{ service: "iam", kind: h.kind === "user" ? "users" : "roles", name: h.name }} className="font-mono text-[12.5px]">
      {arn}
    </ResourceTo>
  );
}

function Group({ g, now, soft }: { g: DenialGroup; now: number; soft: boolean }) {
  const [open, setOpen] = useState(false);
  const w = who(g.principal);
  const what = `${w.label} · ${g.action}`;
  return (
    <li className="grid gap-3 border-t px-6 py-5 first:border-t-0" data-testid="denial-group" data-caller={g.caller} data-action={g.action}>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        <span className="min-w-0 text-[15px] font-semibold text-bright" title={w.title} data-testid="denial-caller">
          {w.label}
        </span>
        <span className="font-mono text-[13.5px] text-foreground">
          {g.api ?? g.service} {g.action}
        </span>
        {g.would_be_denied > 0 && <StatusDot tone="warn">{plural(Number(g.would_be_denied), "call")} would be denied</StatusDot>}
        {g.denied > 0 && <StatusDot tone="bad">{plural(Number(g.denied), "call")} denied</StatusDot>}
        <span className="ml-auto text-[13px] text-muted-foreground">last {ago(g.last_ms, now)}</span>
        <Button asChild variant="outline" size="sm" className="bg-card">
          <Link to="/live" search={{ call: g.last_seq }}>
            Newest in Live
          </Link>
        </Button>
        <Button size="sm" onClick={() => setOpen(true)} data-testid="make-pass" disabled={g.explained.length === 0}>
          <WandSparklesIcon /> Make this pass
        </Button>
      </div>
      {g.resources.length > 0 && (
        <div className="truncate font-mono text-[12.5px] text-muted-foreground" title={g.resources.join("\n")}>
          on {g.resources.join(", ")}
        </div>
      )}
      {g.explained.length > 0 ? (
        <div className="grid gap-1.5 border-l-2 border-input pl-3.5 text-[13px]" data-testid="denial-why">
          {g.by_service && <p className="text-muted-foreground">IAM checks this operation under another action name; these are this caller&apos;s refusals of {g.api ?? g.service}:</p>}
          <ul className="grid gap-1">
            {g.explained.map((r) => (
              <li key={`${r.action}-${r.resource}`}>
                <span className="font-mono">{r.action}</span> on <span className="font-mono break-all">{r.resource}</span>: IAM {soft ? "would refuse it" : "refused it"} {r.reason}.
              </li>
            ))}
          </ul>
        </div>
      ) : (
        <p className="border-l-2 border-input pl-3.5 text-[13px] text-muted-foreground" data-testid="denial-why-none">
          IAM&apos;s access log has no refusal of this caller for {g.api ?? g.service} (it holds the newest 2,000 rows), so there is nothing to draft from: caller, operation and resource are above, and no statement allows them.
        </p>
      )}
      {open && <PassDialog open={open} onOpenChange={setOpen} rows={g.explained} what={what} />}
    </li>
  );
}

const REFUSAL_COLUMNS: DataColumn<Refusal>[] = [
  { id: "principal", header: "Principal", size: 320, min: 170, accessorKey: "principal", cell: (c) => <Holder arn={c.getValue<string>()} /> },
  { id: "action", header: "Action", size: 200, min: 140, accessorKey: "action", meta: { mono: true } },
  { id: "resource", header: "Resource", size: 260, min: 130, accessorKey: "resource", meta: { mono: true } },
  { id: "decision", header: "Decision", size: 130, accessorKey: "decision" },
  { id: "reason", header: "IAM's reason", accessorKey: "reason" },
  { id: "count", header: "Times", size: 80, accessorKey: "count", meta: { numeric: true } },
];

const PRINCIPAL_COLUMNS: DataColumn<KeyPrincipal>[] = [
  { id: "key", header: "Access key", size: 240, min: 190, accessorKey: "key", meta: { mono: true } },
  {
    id: "who",
    header: "Who",
    accessorFn: (p) => who(p).label,
    cell: (c) => {
      const p = c.row.original;
      if (p.kind === "user" && p.name) return <ResourceTo to={{ service: "iam", kind: "users", name: p.name }}>IAM user {p.name}</ResourceTo>;
      return <span title={who(p).title}>{p.kind === "root" ? "The account root (a key IAM did not issue)" : p.kind === "session" ? "A temporary session key" : "This console"}</span>;
    },
  },
  { id: "arn", header: "ARN", size: 340, min: 180, accessorFn: (p) => p.arn ?? "", meta: { mono: true } },
];

/** Access: IAM as the person meets it locally. The mode and how to change it; the calls IAM refused or would have, by caller and
 * operation, with IAM's reason and a drafted policy that lets them through; every refusal IAM logged; and who each key is. */
export function AccessPage() {
  const access = useAccess();
  const v = access.data;
  const now = useNow();
  const principals = usePrincipals("all");
  const { refetch } = access;
  const { refetch: refetchNames } = principals;
  const refresh = useCallback(() => {
    void refetch();
    void refetchNames();
  }, [refetch, refetchNames]);
  const soft = v?.iam === "soft";
  const keys = useMemo(() => (principals.data?.principals ?? []).filter((p) => p.kind !== "console"), [principals.data]);
  return (
    <>
      <PageHeader
        title="Access"
        sub={<><span className="text-foreground">Who can do what, and why.</span> Identities and the credentials they hold, the calls IAM refused or would have, and who each access key is.</>}
        refresh={<RefreshControl updatedAt={access.dataUpdatedAt} fetching={access.isFetching} onRefresh={refresh} />}
      />
      <IdentityCards />
      {access.isError && !v ? (
        <ErrorBlock what="Access could not be read" error={access.error} hint={HINT} onRetry={() => void refetch()} retrying={access.isFetching} testId="access-error" />
      ) : !v ? (
        <CardSkeleton rows={6} />
      ) : (
        <>
          <Modes mode={v.iam} />
          <Section
            title={soft ? "Would be denied" : "Denied"}
            testId="denials"
            lede={`From the Live record (${number(v.ring.held)} newest calls): each caller and operation IAM ${soft ? "would refuse under --iam enforce" : "refused"}, with IAM's reason from its access log.`}
          >
            {v.groups.length === 0 ? (
              <EmptyState icon={ShieldCheckIcon} title={v.iam === "off" ? "IAM is off" : "Nothing refused"}>
                {v.iam === "off" ? "No call is checked against a policy, so none is refused. Start doze-aws with --iam soft to see what your policies would refuse." : `No call in the Live record was ${soft ? "let through that IAM would refuse" : "refused by IAM"}.`}
              </EmptyState>
            ) : (
              <ul className="rounded-[14px] border bg-card" data-testid="denial-groups">
                {v.groups.map((g) => (
                  <Group key={`${g.caller}-${g.service}-${g.action}`} g={g} now={now} soft={soft} />
                ))}
              </ul>
            )}
          </Section>
          <Section title="Every refusal IAM logged" testId="refusals" lede={`IAM's access log: one row per principal, action and resource, newest first.${v.evicted > 0 ? ` It let ${number(v.evicted)} older rows go to stay at its size.` : ""}${v.log_read ? "" : " It could not be read."}`}>
            <DataTable aria-label="Refusals IAM logged" data={v.refusals} columns={REFUSAL_COLUMNS} rowId={(r) => `${r.principal}|${r.action}|${r.resource}`} maxHeight={420} empty={v.iam === "off" ? "IAM is off: it logs nothing." : "IAM has refused nothing."} />
          </Section>
          <Section title="Who is who" testId="principals" lede="Every access key in the Live record, as IAM knows it. Live shows a user's name in place of its key.">
            {principals.data ? <DataTable aria-label="Access keys" data={keys} columns={PRINCIPAL_COLUMNS} rowId={(p) => p.key} maxHeight={360} empty="No app has called yet." /> : <CardSkeleton rows={2} />}
          </Section>
        </>
      )}
    </>
  );
}

