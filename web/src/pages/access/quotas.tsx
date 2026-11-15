import { useQueryClient } from "@tanstack/react-query";
import { GaugeIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { useMemo, useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { ApiFailure } from "@/api/client";
import { post } from "@/api/r4";
import type { AccessQuota } from "@/api/types/AccessQuota";
import type { AccessQuotaClient } from "@/api/types/AccessQuotaClient";
import type { AccessQuotas } from "@/api/types/AccessQuotas";
import type { KafkaRefusal } from "@/api/types/KafkaRefusal";
import type { QuotaOutcome } from "@/api/types/QuotaOutcome";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { StatusDot } from "@/components/app/status-dot";
import { Done, DoneFooter } from "@/components/app/write-done";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Form, FormControl, FormDescription, FormField, FormItem, FormLabel, FormMessage } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { CallBox, RefusalPanel, SectionHead, useStable } from "../r4/bits";

/** The quotas a user or client id can be held to, with the word the table uses and what the value counts. */
const KEYS = [
  { key: "producer_byte_rate", label: "Produce", unit: "bytes a second", hint: "Produce request bytes a second; the records are taken, then the client waits." },
  { key: "consumer_byte_rate", label: "Fetch", unit: "bytes a second", hint: "Fetch response bytes a second; a throttled fetch comes back empty and is asked again." },
  { key: "request_percentage", label: "Request time", unit: "percent of a thread", hint: "The broker's time on the client's requests, as a percentage of one request thread." },
  { key: "controller_mutation_rate", label: "Partition changes", unit: "partitions a second", hint: "Partitions created or deleted a second; from CreateTopics v6 a topic past it is refused (89)." },
] as const;

const keyLabel = (k: string) => KEYS.find((x) => x.key === k)?.label ?? k;

/** The entity of a quota in `kafka-configs.sh` words. */
function entityWords(q: AccessQuota): string {
  return q.parts.map((p) => `${p.kind === "client-id" ? "client id" : p.kind} ${p.name ?? "<default>"}`).join(", ");
}

/** The remove target of a row, as the page's address carries it. */
function removeQuery(q: AccessQuota): string {
  const p = new URLSearchParams();
  for (const part of q.parts) p.set(part.kind === "client-id" ? "client" : part.kind, part.name ?? "");
  p.set("key", q.key);
  return p.toString();
}

function Who({ q }: { q: AccessQuota }) {
  return (
    <span className="flex flex-wrap items-center gap-1.5">
      {q.parts.map((p) => (
        <span key={p.kind} className="inline-flex items-center gap-1">
          <span className="text-muted-foreground">{p.kind === "client-id" ? "client id" : p.kind}</span>
          {p.name === null ? <Badge variant="secondary">default</Badge> : <span className="font-mono text-[13px] text-bright">{p.name}</span>}
        </span>
      ))}
    </span>
  );
}

function muted(c: AccessQuotaClient): React.ReactNode {
  if (c.muted_ms > 0) return <StatusDot tone="warn">Muted {Math.ceil(c.muted_ms / 100) / 10} s more</StatusDot>;
  if (c.last_mute_ms > 0) return <span className="text-muted-foreground">last muted {c.last_mute_ms} ms</span>;
  return <StatusDot tone="ok">Within</StatusDot>;
}

const clientColumns: DataColumn<AccessQuotaClient>[] = [
  { id: "who", header: "Metered as", accessorFn: (c) => `${c.user} ${c.client_id}`, cell: (c) => <span className="font-mono text-[13px] text-bright">{[c.row.original.user && `user ${c.row.original.user}`, c.row.original.client_id && `client id ${c.row.original.client_id}`].filter(Boolean).join(", ") || "no client id"}</span> },
  { id: "key", header: "Quota", size: 150, accessorFn: (c) => keyLabel(c.key) },
  { accessorKey: "rate", header: "Rate now", size: 130, meta: { numeric: true } },
  { id: "limit", header: "Limit", size: 220, accessorFn: (c) => c.limit ?? "none", cell: (c) => <span>{c.row.original.limit ?? "none"}{c.row.original.share !== null && <span className="ml-1.5 text-muted-foreground tabular-nums">({c.row.original.share}%)</span>}</span> },
  { id: "state", header: "Now", size: 170, accessorFn: (c) => c.muted_ms, cell: (c) => muted(c.row.original) },
];

/** The Client quotas section of the Access page: what is set, and who it holds now. */
export function Quotas({ quotas, onSet, onRemove }: { quotas: AccessQuotas; onSet: (from: HTMLElement) => void; onRemove: (q: string, from: HTMLElement) => void }) {
  const remove = useStable(onRemove);
  const columns = useMemo<DataColumn<AccessQuota>[]>(
    () => [
      { id: "who", header: "For", accessorFn: (q) => entityWords(q), cell: (c) => <Who q={c.row.original} /> },
      { id: "key", header: "Quota", size: 150, accessorFn: (q) => keyLabel(q.key) },
      { id: "limit", header: "Limit", size: 230, accessorFn: (q) => q.limit, cell: (c) => <span title={`${c.row.original.key}=${c.row.original.value}`}>{c.row.original.limit}</span> },
      { accessorKey: "applies", header: "Holds", cell: (c) => <span className={c.row.original.enforced ? "" : "text-muted-foreground"}>{c.row.original.applies}</span> },
      {
        id: "remove",
        header: () => <span className="sr-only">Remove</span>,
        size: 104,
        enableSorting: false,
        cell: (c) => (
          <Button variant="outline" size="sm" className="h-7 bg-card" onClick={(ev) => remove(removeQuery(c.row.original), ev.currentTarget)} aria-label={`Remove the ${keyLabel(c.row.original.key)} quota of ${entityWords(c.row.original)}`} data-testid="quota-remove">
            <Trash2Icon /> Remove
          </Button>
        ),
      },
    ],
    [remove],
  );
  const mutedNow = quotas.clients.filter((c) => c.muted_ms > 0).length;
  return (
    <section aria-labelledby="quotas" className="mb-12" data-testid="quotas">
      <SectionHead
        id="quotas"
        title="Client quotas"
        sub="What DescribeClientQuotas returns. A client over a quota is answered at once with a throttle time, and the broker reads nothing more from its connection until the time is over. The most specific quota that matches applies: user and client id, then user, then client id."
        actions={
          <Button onClick={(ev) => onSet(ev.currentTarget)} data-testid="quota-set">
            <PlusIcon /> Set quota
          </Button>
        }
      />
      {quotas.refused ? (
        <RefusalPanel refusal={quotas.refused} />
      ) : (
        <DataTable data={quotas.rows} columns={columns} rowId={(q) => removeQuery(q)} aria-label="Client quotas" maxHeight={420} empty="No quota: every client may produce, fetch and make topics as fast as the broker goes. Set one to see how a client behaves when it is held back." />
      )}
      {quotas.clients.length > 0 && (
        <div className="mt-6" data-testid="quota-clients">
          <h3 className="mb-2 flex items-center gap-2 text-[15px] font-medium text-bright">
            <GaugeIcon className="size-4 text-muted-foreground" aria-hidden /> Who the quotas hold
            <span className="text-[12.5px] font-normal text-muted-foreground">{mutedNow ? `${mutedNow} muted now` : "nobody muted now"}</span>
          </h3>
          <p className="mb-3 max-w-[52rem] text-[13px] text-muted-foreground">Measured as Kafka measures it: the sum over the last eleven one-second windows, over at least ten seconds. A user quota is one meter for all the user's client ids; a default client id quota is a meter per client id.</p>
          <DataTable data={quotas.clients} columns={clientColumns} rowId={(c) => `${c.key}:${c.user}:${c.client_id}`} aria-label="Metered clients" maxHeight={320} />
        </div>
      )}
    </section>
  );
}

type QuotaValues = { who: "user" | "client" | "both"; user: string; userDefault: boolean; client: string; clientDefault: boolean; key: string; value: string };
const BLANK: QuotaValues = { who: "client", user: "", userDefault: false, client: "", clientDefault: false, key: "producer_byte_rate", value: "" };

function shell(s: string): string {
  return /^[\w.@:/-]+$/.test(s) ? s : `'${s.replaceAll("'", "'\\''")}'`;
}

/** The same change as `kafka-configs.sh` makes it: what the dialog teaches. */
function configsCall(v: QuotaValues, remove: boolean): Array<Array<["keyword" | "str" | "plain", string]>> {
  const parts: Array<["keyword" | "str" | "plain", string]> = [["keyword", "kafka-configs.sh"], ["plain", " --bootstrap-server localhost:9092 --alter"]];
  const entity = (type: string, deflt: boolean, name: string) => (deflt ? [["plain", ` --entity-type ${type} --entity-default`] as ["plain", string]] : [["plain", ` --entity-type ${type} --entity-name `] as ["plain", string], ["str", shell(name || "<name>")] as ["str", string]]);
  const lines: Array<Array<["keyword" | "str" | "plain", string]>> = [parts];
  if (v.who !== "client") lines.push([["plain", " "], ...entity("users", v.userDefault, v.user)]);
  if (v.who !== "user") lines.push([["plain", " "], ...entity("clients", v.clientDefault, v.client)]);
  lines.push(remove ? [["plain", "  --delete-config "], ["str", v.key]] : [["plain", "  --add-config "], ["str", `${v.key}=${v.value || "<value>"}`]]);
  return lines;
}

function body(v: QuotaValues, remove: boolean) {
  return {
    user: v.who === "client" ? null : v.userDefault ? "" : v.user.trim(),
    client: v.who === "user" ? null : v.clientDefault ? "" : v.client.trim(),
    key: v.key,
    value: remove ? null : Number(v.value),
  };
}

function SetBody({ onDone }: { onDone: () => void }) {
  const qc = useQueryClient();
  const form = useForm<QuotaValues>({ defaultValues: BLANK });
  const v = useWatch({ control: form.control }) as QuotaValues;
  const [refused, setRefused] = useState<KafkaRefusal | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [made, setMade] = useState(false);
  const key = KEYS.find((k) => k.key === v.key) ?? KEYS[0];
  const submit = async (values: QuotaValues) => {
    setBusy(true);
    setRefused(null);
    setFailure(null);
    try {
      const out = await post<QuotaOutcome>("/api/v1/access/quotas", body(values, false));
      if (out.outcome === "done") {
        setMade(true);
        await qc.invalidateQueries({ queryKey: ["access"] });
      } else setRefused(out.refused);
    } catch (e) {
      setFailure(e instanceof ApiFailure ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  if (made) {
    return (
      <div className="flex flex-col gap-4">
        <Done testId="quota-made">
          <b className="font-semibold text-bright">Set.</b> Stored, and in force from the next request: a client over it is told to wait, and its connection is muted for that long.
        </Done>
        <DoneFooter onDone={onDone} />
      </div>
    );
  }
  const nameField = (name: "user" | "client", deflt: "userDefault" | "clientDefault", label: string, placeholder: string) => (
    <FormField
      control={form.control}
      name={name}
      rules={{ validate: (x, all) => all[deflt] || x.trim() !== "" || `Type the ${label}, or hold every ${label} without its own quota.` }}
      render={({ field }) => (
        <FormItem>
          <FormLabel>{label === "user" ? "User" : "Client id"}</FormLabel>
          <FormControl>
            <Input {...field} disabled={v[deflt]} placeholder={v[deflt] ? "<default>" : placeholder} autoComplete="off" className="bg-card font-mono" data-testid={`quota-${name}`} />
          </FormControl>
          <label className="flex items-center gap-2 text-[12.5px] text-muted-foreground">
            <input type="checkbox" checked={v[deflt]} onChange={(e) => form.setValue(deflt, e.target.checked)} data-testid={`quota-${name}-default`} /> The default: every {label} without a quota of its own
          </label>
          <FormMessage />
        </FormItem>
      )}
    />
  );
  return (
    <Form {...form}>
      <form noValidate onSubmit={form.handleSubmit((x) => void submit(x))} className="grid min-w-0 items-start gap-4 sm:grid-cols-2" data-testid="quota-form">
        <FormField
          control={form.control}
          name="who"
          render={({ field }) => (
            <FormItem className="sm:col-span-2">
              <FormLabel>For</FormLabel>
              <FormControl>
                <ToggleGroup type="single" variant="outline" value={field.value} onValueChange={(x) => x && field.onChange(x)} className="self-start bg-card" data-testid="quota-who">
                  <ToggleGroupItem value="client" className="px-3">Client id</ToggleGroupItem>
                  <ToggleGroupItem value="user" className="px-3">User</ToggleGroupItem>
                  <ToggleGroupItem value="both" className="px-3">User and client id</ToggleGroupItem>
                </ToggleGroup>
              </FormControl>
              <FormDescription>{v.who === "user" ? "A user is who a client logs in as (User:ANONYMOUS without a login); every client of the user shares one quota." : v.who === "client" ? "The client.id a client sends. Any client can send any id, so this holds clients that cooperate." : "One client id of one user: the most specific quota there is."}</FormDescription>
            </FormItem>
          )}
        />
        {v.who !== "client" && nameField("user", "userDefault", "user", "orders")}
        {v.who !== "user" && nameField("client", "clientDefault", "client id", "orders-producer")}
        <FormField
          control={form.control}
          name="key"
          render={({ field }) => (
            <FormItem>
              <FormLabel>Quota</FormLabel>
              <Select value={field.value} onValueChange={field.onChange}>
                <FormControl>
                  <SelectTrigger className="w-full bg-card" data-testid="quota-key">
                    <SelectValue />
                  </SelectTrigger>
                </FormControl>
                <SelectContent position="popper">
                  {KEYS.map((k) => (
                    <SelectItem key={k.key} value={k.key}>
                      {k.label} <span className="font-mono text-[12px] text-muted-foreground">{k.key}</span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <FormDescription>{key.hint}</FormDescription>
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="value"
          rules={{ validate: (x) => (x.trim() !== "" && Number.isFinite(Number(x))) || "Type a number." }}
          render={({ field }) => (
            <FormItem>
              <FormLabel>Limit</FormLabel>
              <FormControl>
                <Input {...field} inputMode="decimal" placeholder={v.key.endsWith("byte_rate") ? "1048576" : "10"} autoComplete="off" className="bg-card font-mono" data-testid="quota-value" />
              </FormControl>
              <FormDescription>In {key.unit}. Kafka refuses zero, and a fraction of a byte.</FormDescription>
              <FormMessage />
            </FormItem>
          )}
        />
        <div className="flex flex-col gap-3 sm:col-span-2" aria-live="polite">
          {refused && <RefusalPanel refusal={refused} />}
          {failure && (
            <p role="alert" className="text-[13px] text-bad">
              {failure}
            </p>
          )}
          <CallBox title="The same change from a terminal" lines={configsCall(v, false)} />
        </div>
        <DialogFooter className="sm:col-span-2">
          <DialogClose asChild>
            <Button type="button" variant="outline">
              Cancel
            </Button>
          </DialogClose>
          <Button type="submit" disabled={busy} data-testid="quota-submit">
            <PlusIcon /> {busy ? "Setting…" : "Set quota"}
          </Button>
        </DialogFooter>
      </form>
    </Form>
  );
}

/** Set quota, over the Access page (`?quota=new`): AlterClientQuotas as the console. */
export function SetQuotaDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="top-6 max-h-[calc(100vh-3rem)] translate-y-0 overflow-y-auto sm:max-w-[42rem]" data-testid="quota-dialog">
        <DialogHeader>
          <DialogTitle className="text-[20px] font-medium">Set quota</DialogTitle>
          <DialogDescription>A limit on what a client may do a second. Sent as AlterClientQuotas, as the console; kept across restarts.</DialogDescription>
        </DialogHeader>
        {open && <SetBody onDone={onClose} />}
      </DialogContent>
    </Dialog>
  );
}

/** Remove quota (`?unquota=<user, client and key>`). */
export function RemoveQuotaDialog({ query, onClose }: { query: string | undefined; onClose: () => void }) {
  return <RemoveQuota key={query ?? ""} query={query} onClose={onClose} />;
}

function RemoveQuota({ query, onClose }: { query: string | undefined; onClose: () => void }) {
  const qc = useQueryClient();
  const [refused, setRefused] = useState<KafkaRefusal | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [gone, setGone] = useState(false);
  const p = new URLSearchParams(query ?? "");
  const v: QuotaValues = { who: p.has("user") && p.has("client") ? "both" : p.has("user") ? "user" : "client", user: p.get("user") ?? "", userDefault: p.get("user") === "", client: p.get("client") ?? "", clientDefault: p.get("client") === "", key: p.get("key") ?? "", value: "" };
  const remove = async () => {
    setBusy(true);
    setFailure(null);
    try {
      const out = await post<QuotaOutcome>("/api/v1/access/quotas", body(v, true));
      if (out.outcome === "done") {
        setGone(true);
        await qc.invalidateQueries({ queryKey: ["access"] });
      } else setRefused(out.refused);
    } catch (e) {
      setFailure(e instanceof ApiFailure ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const who = [v.who !== "client" && `user ${v.userDefault ? "<default>" : v.user}`, v.who !== "user" && `client id ${v.clientDefault ? "<default>" : v.client}`].filter(Boolean).join(", ");
  return (
    <Dialog open={!!query} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[40rem]" data-testid="quota-remove-dialog">
        <DialogHeader>
          <DialogTitle className="text-[20px] font-medium">Remove this quota?</DialogTitle>
          <DialogDescription>AlterClientQuotas with the key removed, as the console. A less specific quota that matches, if any, applies from the next request.</DialogDescription>
        </DialogHeader>
        {gone ? (
          <Done testId="quota-removed">
            <b className="font-semibold text-bright">Removed.</b> The {keyLabel(v.key)} quota of {who} is gone.
          </Done>
        ) : (
          <div className="flex min-w-0 flex-col gap-4">
            <div className="rounded-lg border bg-card p-4 text-[13.5px]">
              <b className="font-medium text-bright">{keyLabel(v.key)}</b> <span className="text-muted-foreground">for</span> <span className="font-mono">{who}</span>
            </div>
            {refused && <RefusalPanel refusal={refused} />}
            {failure && (
              <p role="alert" className="text-[13px] text-bad">
                {failure}
              </p>
            )}
            <CallBox title="The same change from a terminal" lines={configsCall(v, true)} />
          </div>
        )}
        {gone ? (
          <DoneFooter onDone={onClose} />
        ) : (
          <DialogFooter>
            <DialogClose asChild>
              <Button type="button" variant="outline">
                Cancel
              </Button>
            </DialogClose>
            <Button variant="destructive" disabled={busy} onClick={() => void remove()} data-testid="quota-remove-confirm">
              <Trash2Icon /> {busy ? "Removing…" : "Remove quota"}
            </Button>
          </DialogFooter>
        )}
      </DialogContent>
    </Dialog>
  );
}
