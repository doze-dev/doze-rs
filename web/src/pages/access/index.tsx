import { useNavigate, useSearch } from "@tanstack/react-router";
import { CircleCheckIcon, PlusIcon, SearchIcon, Trash2Icon, TriangleAlertIcon } from "lucide-react";
import { useMemo } from "react";
import { useAccess } from "@/api/r4";
import type { AccessResponse } from "@/api/types/AccessResponse";
import type { AccessUser } from "@/api/types/AccessUser";
import type { AclChoice } from "@/api/types/AclChoice";
import type { AclEntry } from "@/api/types/AclEntry";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { PageHeader } from "@/components/app/page-header";
import { Spans } from "@/components/app/spans";
import { StatusDot } from "@/components/app/status-dot";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { number } from "@/lib/format";
import type { AccessSearch } from "@/lib/search";
import { PageError } from "../page-error";
import { Fact, SectionHead, useRoutedOverlay, useStable } from "../r4/bits";
import { CreateAclDialog, DeleteAclDialog } from "./acl-dialogs";

/** The form words of an ACL, as a query string: its stable id, and the address of its Delete dialog. */
export function aclQuery(e: AclEntry, kinds: AclChoice[], ops: AclChoice[]): string {
  const p = new URLSearchParams();
  p.set("type", kinds.find((k) => k.number === e.kind)?.value ?? String(e.kind));
  p.set("name", e.name);
  p.set("pattern", e.pattern);
  p.set("principal", e.principal);
  p.set("host", e.host);
  p.set("op", ops.find((o) => o.number === e.operation)?.value ?? String(e.operation));
  p.set("perm", e.permission);
  return p.toString();
}

function Mono({ children }: { children: React.ReactNode }) {
  return <span className="font-mono text-[13px] text-bright">{children}</span>;
}

function SwitchCard({ title, on, onWord, offWord, testid, children, lede }: { title: string; on: boolean; onWord: string; offWord: string; testid: string; lede: string; children: React.ReactNode }) {
  return (
    <Card className="min-w-0" data-testid={testid}>
      <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-2">
        <CardTitle className="text-[17px]">{title}</CardTitle>
        <StatusDot tone={on ? "ok" : "idle"}>{on ? onWord : offWord}</StatusDot>
        <CardDescription className="basis-full">{lede}</CardDescription>
      </CardHeader>
      <CardContent>
        <dl>{children}</dl>
      </CardContent>
    </Card>
  );
}

function Switches({ r }: { r: AccessResponse }) {
  const a = r.access;
  return (
    <div className="mb-12 grid gap-6 xl:grid-cols-3">
      <SwitchCard testid="auth-card" title="Authentication" on={a.auth.on} onWord="On" offWord="Off" lede={a.auth.on ? "A client must log in before it may send anything but ApiVersions." : "Any client may connect and send requests; nobody logs in."}>
        <Fact label="Mechanisms" sub={a.auth.on ? (a.auth.plain ? "PLAIN is on beside SCRAM: a flag user may use it" : "PLAIN is off: a stored SCRAM user cannot use it") : "--sasl-mechanisms sets them"}>
          <Mono>{a.auth.mechanisms.length ? a.auth.mechanisms.join(", ") : "none"}</Mono>
        </Fact>
        <Fact label="Users">
          <Mono>
            {number(a.auth.flag_users)} from flags, {number(a.auth.stored_users)} stored
          </Mono>
        </Fact>
        {a.auth.failed_login && (
          <Fact label="A failed login" sub="--sasl-failed-login-delay, then the connection closes">
            <Mono>{a.auth.failed_login}</Mono>
          </Fact>
        )}
      </SwitchCard>
      <SwitchCard testid="authz-card" title="Authorizer" on={!a.authz.default_allow} onWord="Denies by default" offWord="Allows by default" lede={a.authz.default_allow ? "A resource no ACL covers is open to everyone; an ACL that matches still decides." : "A resource no ACL covers is refused to everyone."}>
        <Fact label="Default">
          <Mono>--acl-default {a.authz.default_allow ? "allow" : "deny"}</Mono>
        </Fact>
        <Fact label="ACLs" sub={`${number(a.authz.allow)} allow, ${number(a.authz.deny)} deny`}>
          <Mono>{number(a.authz.acls)}</Mono>
        </Fact>
        <Fact label="Super users" sub={a.authz.super_users.length ? "never refused, DENY included" : "--acl-super-user names them"}>
          <Mono>{a.authz.super_users.length ? a.authz.super_users.join(", ") : "none"}</Mono>
        </Fact>
      </SwitchCard>
      <SwitchCard testid="tls-card" title="Encryption" on={a.tls.on} onWord="TLS on" offWord="Plaintext" lede={a.tls.on ? "Clients must connect with TLS 1.2 or newer; a plaintext client is closed with no answer." : "The Kafka listener speaks plaintext."}>
        {a.tls.cert && (
          <Fact label="Certificate" sub="the chain, leaf first">
            <Mono>{a.tls.cert}</Mono>
          </Fact>
        )}
        {a.tls.on && (
          <Fact label="Private key" sub="its contents are printed nowhere">
            Read at start
          </Fact>
        )}
        <Fact label="Listener" sub={a.tls.loopback ? "loopback: this machine only" : "reachable from other machines"}>
          <Mono>{a.tls.listener}</Mono>
        </Fact>
      </SwitchCard>
    </div>
  );
}

const userColumns: DataColumn<AccessUser>[] = [
  { accessorKey: "name", header: "User", size: 180, meta: { mono: true } },
  {
    accessorKey: "from",
    header: "From",
    size: 150,
    cell: (c) => (
      <span className="flex items-center gap-2">
        <Badge variant="secondary">{c.row.original.from === "flags" ? "Flags" : "Stored"}</Badge>
        {c.row.original.super_user && <span className="text-[12.5px] text-muted-foreground">super user</span>}
      </span>
    ),
  },
  { id: "mechanisms", header: "Mechanisms", accessorFn: (u) => u.mechanisms.join(", ") || "none", meta: { mono: true } },
  { id: "iterations", header: "Iterations", size: 150, accessorFn: (u) => (u.iterations.length ? u.iterations.map(([m, n]) => (u.iterations.length > 1 ? `${number(n)} (${m.replace("SCRAM-", "")})` : number(n))).join(", ") : "–"), meta: { numeric: true } },
];

function Users({ r }: { r: AccessResponse }) {
  return (
    <section aria-labelledby="users" className="mb-12">
      <SectionHead id="users" title="Users" sub="What DescribeUserScramCredentials returns: a name, a mechanism and an iteration count. No password, salt or key is ever shown. A user from the flags or a file is re-applied at every start and cannot be changed over the API." />
      <DataTable data={r.access.users} columns={userColumns} rowId={(u) => `${u.from}:${u.name}`} aria-label="Users" maxHeight={360} empty={r.access.auth.on ? "No user yet: make one with kafka-configs.sh --alter --add-config 'SCRAM-SHA-256=[password=...]'." : "Nobody logs in on this broker, so there are no users."} />
    </section>
  );
}

function Acls({ r, search, set, onDelete, onCreate }: { r: AccessResponse; search: AccessSearch; set: (p: Partial<AccessSearch>) => void; onDelete: (q: string, from: HTMLElement) => void; onCreate: (from: HTMLElement) => void }) {
  const kinds = r.kinds;
  const ops = r.operations;
  const label = (list: AclChoice[], n: number) => list.find((k) => k.number === n)?.label ?? String(n);
  const del = useStable(onDelete);
  const columns = useMemo<DataColumn<AclEntry>[]>(
    () => [
      {
        id: "resource",
        header: "Resource",
        accessorFn: (e) => `${label(kinds, e.kind)} ${e.name}`,
        cell: (c) => (
          <span>
            <span className="text-muted-foreground">{label(kinds, c.row.original.kind)} </span>
            <span className="font-mono text-[13px] text-bright">{c.row.original.name}</span>
          </span>
        ),
      },
      { accessorKey: "pattern", header: "Pattern", size: 90, cell: (c) => (c.row.original.pattern === "literal" ? "Literal" : "Prefixed") },
      {
        id: "principal",
        header: "Principal @ host",
        size: 190,
        accessorFn: (e) => `${e.principal} @ ${e.host}`,
        meta: { mono: true },
      },
      { id: "operation", header: "Operation", size: 120, accessorFn: (e) => label(ops, e.operation), meta: { mono: true } },
      {
        accessorKey: "permission",
        header: "Permission",
        size: 96,
        cell: (c) => <StatusDot tone={c.row.original.permission === "allow" ? "ok" : "warn"}>{c.row.original.permission === "allow" ? "Allow" : "Deny"}</StatusDot>,
      },
      {
        id: "delete",
        header: () => <span className="sr-only">Delete</span>,
        size: 96,
        enableSorting: false,
        cell: (c) => (
          <Button variant="outline" size="sm" className="h-7 bg-card" onClick={(ev) => del(aclQuery(c.row.original, kinds, ops), ev.currentTarget)} aria-label={`Delete the ACL on ${c.row.original.name} for ${c.row.original.principal}`} data-testid="acl-delete">
            <Trash2Icon /> Delete
          </Button>
        ),
      },
    ],
    [kinds, ops, del],
  );
  const q = (search.q ?? "").toLowerCase();
  const kindN = search.kind ? kinds.find((k) => k.value === search.kind)?.number : undefined;
  const rows = r.access.acls.rows.filter((e) => (kindN === undefined || e.kind === kindN) && (!search.perm || e.permission === search.perm) && (!q || `${e.name} ${e.principal} ${e.host} ${label(ops, e.operation)}`.toLowerCase().includes(q)));
  return (
    <section aria-labelledby="acls" className="mb-12">
      <SectionHead
        id="acls"
        title="ACLs"
        sub="What DescribeAcls returns. A DENY wins over any ALLOW; an ALLOW of Read, Write, Delete or Alter also allows Describe."
        actions={
          <Button onClick={(ev) => onCreate(ev.currentTarget)} data-testid="acl-create">
            <PlusIcon /> Create ACL
          </Button>
        }
      />
      <div className="mb-3 flex flex-wrap items-center gap-2" data-testid="acl-filters">
        <div className="relative">
          <SearchIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden />
          <Input value={search.q ?? ""} onChange={(e) => set({ q: e.target.value || undefined })} placeholder="Name, principal, host…" aria-label="Filter ACLs" className="w-64 bg-card pl-8" data-testid="acl-q" />
        </div>
        <Select value={search.kind ?? "any"} onValueChange={(v) => set({ kind: v === "any" ? undefined : v })}>
          <SelectTrigger className="w-48 bg-card" aria-label="Resource type" data-testid="acl-kind">
            <SelectValue />
          </SelectTrigger>
          <SelectContent position="popper" className="max-h-80">
            <SelectItem value="any">Every resource type</SelectItem>
            {kinds.map((k) => (
              <SelectItem key={k.value} value={k.value}>
                {k.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <ToggleGroup type="single" variant="outline" value={search.perm ?? "any"} onValueChange={(v) => v && set({ perm: v === "any" ? undefined : (v as "allow" | "deny") })} aria-label="Permission" className="bg-card" data-testid="acl-perm">
          <ToggleGroupItem value="any" className="px-3">
            Both
          </ToggleGroupItem>
          <ToggleGroupItem value="allow" className="px-3">
            Allow
          </ToggleGroupItem>
          <ToggleGroupItem value="deny" className="px-3">
            Deny
          </ToggleGroupItem>
        </ToggleGroup>
        <span className="ml-auto text-[12.5px] text-muted-foreground tabular-nums" data-testid="acl-count">
          {rows.length === r.access.acls.rows.length ? `${number(rows.length)} ACLs` : `${number(rows.length)} of ${number(r.access.acls.rows.length)} ACLs`}
        </span>
      </div>
      {r.access.acls.refused ? (
        <div role="alert" className="rounded-xl border border-bad/30 bg-bad/5 p-4 text-[13.5px]">
          <b className="font-mono text-bad">{r.access.acls.refused.name}</b> {r.access.acls.refused.message}
        </div>
      ) : (
        <DataTable data={rows} columns={columns} rowId={(e) => aclQuery(e, kinds, ops)} aria-label="ACLs" maxHeight={520} empty={r.access.acls.rows.length ? "No ACL matches these filters." : r.access.authz.default_allow ? "No ACL yet. With --acl-default allow, every resource is open until an ACL says otherwise." : "No ACL yet: every resource is refused to everyone but the super users."} />
      )}
      {r.access.acls.more > 0 && <p className="mt-2 text-[12.5px] text-muted-foreground">The broker has {number(r.access.acls.more)} more ACLs than the table lists.</p>}
    </section>
  );
}

function ConsoleCards({ r }: { r: AccessResponse }) {
  const c = r.access.console;
  return (
    <div className="mb-12 grid gap-6 xl:grid-cols-2">
      <Card className="min-w-0" data-testid="console-card">
        <CardHeader>
          <CardTitle className="text-[17px]">The console</CardTitle>
          <CardDescription>It reads and writes the broker the way a client does, as one more principal, and it has no login of its own.</CardDescription>
        </CardHeader>
        <CardContent>
          <dl>
            <Fact label="Principal" sub="reserved: no user, credential or --acl-super-user may take the name">
              <Mono>{c.principal}</Mono>
            </Fact>
            <Fact label="Rights" sub="so the ACLs above never stop the console">
              Super user
            </Fact>
            <Fact label="Listens on" sub={c.beyond_loopback ? "beyond loopback: anyone who can reach it can read, write and inject Lab faults" : "loopback; with SASL or TLS on, anywhere else is refused at start unless --console-unprotected"}>
              <Mono>{c.listens}</Mono>
            </Fact>
            <Fact label="Allowed hosts" sub={c.allow_hosts.length ? "added with --console-allow-host" : "none added with --console-allow-host"}>
              {c.allow_hosts.length ? <Mono>{c.allow_hosts.join(", ")}</Mono> : "IP addresses and localhost"}
            </Fact>
            <Fact label="Writes">Same origin only</Fact>
          </dl>
          {c.beyond_loopback && (
            <div role="alert" className="mt-4 flex gap-2 rounded-lg border border-warn/30 bg-warn/5 p-3 text-[13px]" data-testid="exposure-warning">
              <TriangleAlertIcon className="mt-0.5 size-4 shrink-0 text-warn" aria-hidden />
              <span>
                <b className="font-semibold text-foreground">The console is exposed.</b> It is served beyond loopback with no login (--console-unprotected): anyone who can reach {c.listens} acts as {c.principal}, a super user, and can inject faults with the Lab.
              </span>
            </div>
          )}
        </CardContent>
      </Card>
      <Card className="min-w-0" data-testid="warnings-card">
        <CardHeader>
          <CardTitle className="text-[17px]">What the broker said at start</CardTitle>
          <CardDescription>Three warnings, each only when its condition holds.</CardDescription>
        </CardHeader>
        <CardContent>
          <ul className="flex flex-col">
            {r.access.warnings.map((w, i) => (
              <li key={i} className="flex gap-3 border-b py-3 text-[13.5px] last:border-b-0" data-holds={w.holds}>
                {w.holds ? <TriangleAlertIcon className="mt-0.5 size-4 shrink-0 text-warn" aria-hidden /> : <CircleCheckIcon className="mt-0.5 size-4 shrink-0 text-good" aria-hidden />}
                <div className="min-w-0 flex-1">
                  <div className="text-foreground">
                    <Spans spans={w.title} />
                  </div>
                  <div className="text-[12.5px] text-muted-foreground">
                    <Spans spans={w.because} />
                  </div>
                </div>
                <span className="font-mono text-[12px] whitespace-nowrap text-muted-foreground">{w.holds ? "said at start" : "not the case"}</span>
              </li>
            ))}
          </ul>
        </CardContent>
      </Card>
    </div>
  );
}

export function AccessPage() {
  const search = useSearch({ from: "/access" });
  const navigate = useNavigate({ from: "/access" });
  const q = useAccess(search.state ?? "");
  const setFilter = (patch: Partial<AccessSearch>) => void navigate({ search: (s: AccessSearch) => ({ ...s, ...patch }), replace: true, resetScroll: false });
  const overlay = useRoutedOverlay((patch, replace) => void navigate({ search: (s: AccessSearch) => ({ ...s, ...patch }), replace, resetScroll: false }));
  const r = q.data;
  const secured = r ? r.access.auth.on || r.access.tls.on || !r.access.authz.default_allow : false;
  return (
    <>
      <PageHeader
        title="Access"
        sub={
          r ? (
            <>
              <span className="mr-2 inline-flex align-middle">{secured ? <StatusDot tone="ok">Security on</StatusDot> : <StatusDot tone="idle">Open</StatusDot>}</span>
              {secured ? "Who may connect and what each principal may do, as the broker decides it." : "Any client on this machine may connect and do anything. Everything below is read from the broker; no password, key or salt is ever shown."}
            </>
          ) : (
            "Who may connect and what each principal may do."
          )
        }
      />
      {q.error && !r ? (
        <PageError error={q.error} onRetry={() => void q.refetch()} />
      ) : !r ? (
        <Skeleton className="h-96 w-full rounded-xl" />
      ) : (
        <>
          <Switches r={r} />
          <Users r={r} />
          <Acls r={r} search={search} set={setFilter} onCreate={(from) => overlay.open({ acl: "new" }, from)} onDelete={(dq, from) => overlay.open({ delete: dq }, from)} />
          <ConsoleCards r={r} />
        </>
      )}
      {r && <CreateAclDialog open={search.acl === "new"} kinds={r.kinds} operations={r.operations} onClose={() => overlay.close({ acl: undefined })} restoreFocus={overlay.restore} />}
      {r && <DeleteAclDialog query={search.delete} kinds={r.kinds} operations={r.operations} onClose={() => overlay.close({ delete: undefined })} restoreFocus={overlay.restore} />}
    </>
  );
}
