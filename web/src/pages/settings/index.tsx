import { useQueryClient } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { ChevronDownIcon, ChevronUpIcon, CircleCheckIcon, InfoIcon, PencilIcon, XIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { r4keys, useSettings } from "@/api/r4";
import type { SettingsApi } from "@/api/types/SettingsApi";
import type { SettingsConfigRow } from "@/api/types/SettingsConfigRow";
import type { SettingsFlag } from "@/api/types/SettingsFlag";
import type { SettingsInterval } from "@/api/types/SettingsInterval";
import type { SettingsResponse } from "@/api/types/SettingsResponse";
import type { SettingsSaved } from "@/api/types/SettingsSaved";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { PageHeader } from "@/components/app/page-header";
import { Spans } from "@/components/app/spans";
import { StatStrip } from "@/components/app/stat";
import { StatusDot } from "@/components/app/status-dot";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { number } from "@/lib/format";
import type { SettingsSearch } from "@/lib/search";
import { PageError } from "../page-error";
import { bytes, CallBox, Fact, PathValue, narrowed, SectionHead, useMinWidth, useRoutedOverlay, useStable } from "../r4/bits";
import { EditConfigDialog } from "./edit-dialog";

const SECTIONS = [
  ["configs", "Broker configs"],
  ["profile", "Version profile"],
  ["intervals", "Intervals"],
  ["weight", "Weight"],
  ["data", "Data"],
  ["flags", "Flags"],
] as const;

function SectionNav() {
  return (
    <nav aria-label="Sections" className="mb-8 flex flex-wrap gap-1 border-b pb-px" data-testid="settings-sections">
      {SECTIONS.map(([id, label]) => (
        <a key={id} href={`#${id}`} className="rounded-t-md px-3 py-2 text-[13.5px] font-medium text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" onClick={(e) => {
          e.preventDefault();
          document.getElementById(id)?.scrollIntoView({ block: "start" });
        }}>
          {label}
        </a>
      ))}
    </nav>
  );
}

function sourceTone(source: string): "ok" | "info" | "idle" {
  return source.startsWith("Dynamic") ? "ok" : source === "Static broker" ? "info" : "idle";
}

function ConfigsSection({ r, onEdit, saved, onDismiss }: { r: SettingsResponse; onEdit: (key: string, from: HTMLElement) => void; saved: SettingsSaved | null; onDismiss: () => void }) {
  const edit = useStable(onEdit);
  const columns = useMemo<DataColumn<SettingsConfigRow>[]>(
    () => [
      {
        accessorKey: "name",
        header: "Config",
        size: 200,
        cell: (c) => (
          <span className="font-mono text-[13px] text-bright" title={`${c.row.original.name}: ${c.row.original.what}`}>
            {c.row.original.name}
          </span>
        ),
      },
      {
        accessorKey: "value",
        header: "Value",
        size: 150,
        cell: (c) => (
          <span title={c.row.original.gloss ? `${c.row.original.value} (${c.row.original.gloss})` : c.row.original.value}>
            <span className="font-mono tabular-nums">{c.row.original.value}</span>
            {c.row.original.gloss && <span className="ml-2 text-[12.5px] text-muted-foreground">{c.row.original.gloss}</span>}
          </span>
        ),
      },
      {
        accessorKey: "source",
        header: "Source",
        size: 170,
        cell: (c) => (
          <span title={`${c.row.original.source}: ${c.row.original.source_note}`}>
            <StatusDot tone={sourceTone(c.row.original.source)} className="font-medium">
              {c.row.original.source}
            </StatusDot>
          </span>
        ),
      },
      { accessorKey: "takes_effect", header: "Takes effect", cell: (c) => <span title={c.row.original.takes_effect}>{c.row.original.takes_effect}</span> },
      {
        id: "edit",
        header: () => <span className="sr-only">Edit</span>,
        size: 96,
        enableSorting: false,
        cell: (c) => (
          <Button variant="outline" size="sm" className="h-7 bg-card" onClick={(e) => edit(c.row.original.name, e.currentTarget)} aria-label={`Edit ${c.row.original.name}`} data-testid={`edit-${c.row.original.name}`}>
            <PencilIcon /> Edit
          </Button>
        ),
      },
    ],
    [edit],
  );
  const s = r.settings;
  const wide = useMinWidth(1280);
  return (
    <section aria-labelledby="configs" className="mb-12">
      <SectionHead id="configs" title="Broker configs" sub="Source says where the value in force comes from: a dynamic setting first, then the flag it started with, then Kafka's own default. A topic that sets its own value keeps it." />
      {saved && <SavedCard saved={saved} onDismiss={onDismiss} />}
      <DataTable data={s.configs.rows} columns={narrowed(columns, wide, ["takes_effect"], "name")} rowId={(row) => row.name} aria-label="Dynamic broker configs" maxHeight={420} empty="The broker did not describe its configs." />
      <p className="mt-3 max-w-[52rem] text-[12.5px] text-muted-foreground">
        A change is kept on disk and is still there after a restart. It applies to all brokers or to this one (node {s.configs.node_id}). These are the only five: any other config is refused, in Kafka's words.
      </p>
    </section>
  );
}

function SavedCard({ saved, onDismiss }: { saved: SettingsSaved; onDismiss: () => void }) {
  return (
    <div className="mb-4 flex gap-3 rounded-xl border border-good/30 bg-good/5 p-4" role="status" data-testid="saved-card">
      <CircleCheckIcon className="mt-0.5 size-5 shrink-0 text-good" aria-hidden />
      <div className="min-w-0 flex-1 text-[13.5px]">
        <p>
          <b className="font-semibold text-bright">
            Saved <code className="font-mono">{saved.key}</code> = <code className="font-mono">{saved.value}</code>
          </b>
          {saved.gloss && <span className="text-muted-foreground"> ({saved.gloss})</span>}
          <span className="text-muted-foreground"> for {saved.scope === "this" ? "this broker" : "all brokers"}. {saved.effect}.</span>
        </p>
        <p className="mt-1 text-muted-foreground">
          <Spans spans={saved.who_sees} />
        </p>
        <CallBox lines={saved.call} className="mt-3 max-w-xl" />
      </div>
      <Button variant="ghost" size="icon-sm" aria-label="Dismiss" onClick={onDismiss}>
        <XIcon />
      </Button>
    </div>
  );
}

const apiColumns: DataColumn<SettingsApi>[] = [
  { accessorKey: "name", header: "API", size: 260, meta: { mono: true } },
  { accessorKey: "versions", header: "Versions", size: 160, meta: { mono: true } },
  { accessorKey: "family", header: "Family" },
];

function ProfileSection({ r }: { r: SettingsResponse }) {
  const p = r.settings.profile;
  const [all, setAll] = useState(false);
  const shown = all ? p.apis : p.apis.slice(0, 8);
  return (
    <section aria-labelledby="profile" className="mb-12">
      <SectionHead id="profile" title="Version profile" sub={`The Kafka the broker stands for, and the APIs a client is offered, each with the versions a real Kafka ${p.major} broker offers. It is what ApiVersions answers. A client picks, for each API, the highest version both sides have.`} />
      <StatStrip
        className="mb-4"
        stats={[
          { label: "Profile", value: `Kafka ${p.major}`, unit: "--kafka-version", ch: 8 },
          { label: "APIs offered", value: number(p.apis.length), unit: p.sasl_on ? "SASL on" : p.with_sasl.length ? `${p.with_sasl.length} more with SASL on` : "SASL off", ch: 4 },
          { label: "Not offered", value: number(p.not_offered.length), unit: "of this Kafka's APIs", ch: 4 },
        ]}
      />
      {p.speaking.length > 0 && (
        <ul className="mb-4 flex flex-col gap-1 text-[13.5px]" data-testid="speaking">
          {p.speaking.map((sp) => (
            <li key={`${sp.client}-${sp.api}`} className="flex flex-wrap items-center gap-2">
              <code className="font-mono text-bright">{sp.client}</code>
              <span className="text-muted-foreground">speaks</span>
              <code className="font-mono">
                {sp.api} v{sp.version}
              </code>
              <StatusDot tone={sp.inside ? "ok" : "warn"} className="font-medium">
                {sp.inside ? "inside the range" : "outside the range"}
              </StatusDot>
            </li>
          ))}
        </ul>
      )}
      <DataTable data={shown} columns={apiColumns} rowId={(a) => a.name} aria-label="APIs offered" maxHeight={all ? 640 : 400} />
      {p.apis.length > 8 && (
        <Button variant="outline" size="sm" className="mt-3 bg-card" onClick={() => setAll((v) => !v)} data-testid="show-apis">
          {all ? <ChevronUpIcon /> : <ChevronDownIcon />} {all ? "Show fewer" : `Show all ${p.apis.length} APIs`}
        </Button>
      )}
      <div className="mt-3 flex max-w-[52rem] flex-col gap-1 text-[12.5px] text-muted-foreground">
        {p.with_sasl.length > 0 && (
          <p>
            <b className="font-semibold text-foreground">{p.sasl_on ? "Offered because SASL is on: " : "Offered only with SASL on: "}</b>
            {p.with_sasl.map((a) => `${a.name} (${a.versions})`).join(", ")}.
          </p>
        )}
        {p.not_offered.length > 0 && (
          <p>
            <b className="font-semibold text-foreground">Not offered: </b>
            {p.not_offered.join(", ")} (no handler yet).
          </p>
        )}
      </div>
    </section>
  );
}

const intervalColumns: DataColumn<SettingsInterval>[] = [
  { accessorKey: "name", header: "Interval", size: 170, cell: (c) => <span title={`${c.row.original.name}: ${c.row.original.what}`} className="font-medium text-foreground">{c.row.original.name}</span> },
  { accessorKey: "value", header: "Value", size: 80, meta: { mono: true } },
  { accessorKey: "flag", header: "Flag", size: 210, meta: { mono: true } },
  { accessorKey: "default", header: "Default", size: 80, meta: { mono: true } },
  { accessorKey: "note", header: "What it comes to" },
];

function IntervalsSection({ r }: { r: SettingsResponse }) {
  const i = r.settings.intervals;
  const wide = useMinWidth(1280);
  return (
    <section aria-labelledby="intervals" className="mb-12">
      <SectionHead id="intervals" title="Intervals" sub="How often the broker looks at things on its own. Read only: set when the broker starts. To change an interval, stop the broker and start it with the flag." />
      <StatStrip
        className="mb-4"
        stats={[
          { label: "Retention check", value: i.retention_every || "–", unit: "every", ch: 6 },
          { label: "Cleaner", value: i.cleaner_on ? i.cleaner_every || "–" : "off", unit: i.cleaner_on ? "looks every" : "--log-cleaner-enable false", ch: 6 },
          { label: "Transactions", value: i.transactions || "–", unit: "checked every; no flag sets it", ch: 6 },
          i.round
            ? { label: "Last cleaner round", value: i.round.at, unit: `${i.round.topic}-${i.round.partition}${i.round.due === true ? ", due again" : ""}`, mono: true, ch: 6 }
            : { label: "Last cleaner round", value: "none yet", unit: "since the broker began", ch: 6 },
        ]}
      />
      <DataTable data={i.rows} columns={narrowed(intervalColumns, wide, ["default"])} rowId={(row) => row.flag} aria-label="Intervals" empty="The broker did not say its intervals." />
    </section>
  );
}

function WeightSection({ r }: { r: SettingsResponse }) {
  const w = r.settings.weight;
  return (
    <section aria-labelledby="weight" className="mb-12">
      <SectionHead id="weight" title="Weight" sub="What this instance costs the machine on disk and in open partitions. Weight is a feature: the binary's size and its packages are measured in the tests and may only grow with a written reason." />
      <StatStrip
        stats={[
          { label: "Binary", value: w.binary !== null ? bytes(w.binary, true) : "–", unit: "the broker program, console included", ch: 7, tip: w.binary !== null ? `${number(w.binary)} bytes` : undefined },
          { label: "Open partitions", value: `${number(w.open_partitions)} of ${number(w.max_open)}`, unit: w.flag_max_open ? `--max-open-partitions ${w.flag_max_open}` : "opened on first use", ch: 9 },
          { label: "Partitions", value: number(w.partitions), unit: `in ${w.topics === 1 ? "1 topic" : `${number(w.topics)} topics`}`, ch: 5 },
        ]}
      />
    </section>
  );
}

function DataSection({ r }: { r: SettingsResponse }) {
  const d = r.settings.data;
  const size = d.size ? `${d.size[1] ? "at least " : ""}${bytes(d.size[0])}` : "unknown";
  return (
    <section aria-labelledby="data" className="mb-12">
      <SectionHead id="data" title="Data" sub="Everything this broker keeps lives in one directory. Stop it cleanly and the directory can be copied, moved or deleted as a whole." />
      <div className="grid gap-6 xl:grid-cols-2">
        <Card className="min-w-0" data-testid="data-card">
          <CardHeader>
            <CardTitle className="text-[17px]">The data directory</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <PathValue value={d.dir || "unknown"} />
            <dl>
              <Fact label="Set by" sub={d.flag_set ? "given at start" : "the default"}>
                <code className="font-mono">--data-dir</code>
              </Fact>
              <Fact label="Size" sub={`${number(d.files)} files`}>
                <span className="tabular-nums">{size}</span>
              </Fact>
              <Fact label="Partitions on disk" sub={`in ${d.topics === 1 ? "1 topic" : `${number(d.topics)} topics`}`}>
                <span className="tabular-nums">{number(d.partitions)}</span>
              </Fact>
              <Fact label="Last stop" sub={d.last_stop === "clean" ? "the stop was recorded, so every partition was trusted at start" : d.last_stop === "unclean" ? "the next start distrusted every partition and read it back" : "this is a new data directory"}>
                {d.last_stop === "clean" ? <StatusDot tone="ok">Clean</StatusDot> : d.last_stop === "unclean" ? <StatusDot tone="warn">Not clean</StatusDot> : "None yet"}
              </Fact>
            </dl>
          </CardContent>
        </Card>
        <Card className="min-w-0">
          <CardHeader>
            <CardTitle className="text-[17px]">What a restart does</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 text-[13.5px] text-muted-foreground">
            <p>
              SIGTERM or Ctrl-C is a clean stop: every open partition is flushed and closed and the stop is recorded, while a kill is not, and the next start distrusts every partition and the first flush covers what it reads back (nothing acknowledged is lost).
            </p>
            <p>
              Your data is where it was, including topics, records, configs, committed offsets, producer ids and open transactions, while connections, group members, the wire's rows and Lab rules start again from nothing. Stored ACLs and the users made over the API stay; users from flags are re-applied.
            </p>
          </CardContent>
        </Card>
      </div>
    </section>
  );
}

const flagColumns: DataColumn<SettingsFlag>[] = [
  { accessorKey: "name", header: "Flag", size: 230, meta: { mono: true } },
  { accessorKey: "value", header: "Value", size: 170, meta: { mono: true } },
  { accessorKey: "default", header: "Default", size: 120, meta: { mono: true } },
  {
    accessorKey: "what",
    header: "What it does",
    cell: (c) => (
      <span title={c.row.original.what}>
        {c.row.original.set && (
          <Badge variant="secondary" className="mr-2 align-middle">
            set
          </Badge>
        )}
        {c.row.original.what}
      </span>
    ),
  },
];

function FlagsSection({ r }: { r: SettingsResponse }) {
  const flags = r.settings.flags;
  const differ = flags.filter((f) => f.set && f.value !== f.default);
  const [which, setWhich] = useState<"set" | "all">(differ.length > 0 ? "set" : "all");
  const rows = which === "set" ? differ : flags;
  return (
    <section aria-labelledby="flags" className="mb-12">
      <SectionHead
        id="flags"
        title="Flags"
        sub="What the broker was started with, each with its default. Read only: set at start."
        actions={
          <ToggleGroup type="single" variant="outline" value={which} onValueChange={(v) => v && setWhich(v as "set" | "all")} aria-label="Which flags" className="bg-card" data-testid="flags-which">
            <ToggleGroupItem value="set" className="px-3">
              Differ from default · {differ.length}
            </ToggleGroupItem>
            <ToggleGroupItem value="all" className="px-3">
              All · {flags.length}
            </ToggleGroupItem>
          </ToggleGroup>
        }
      />
      <DataTable data={rows} columns={flagColumns} rowId={(f) => f.name} aria-label="Flags" maxHeight={520} empty={which === "set" ? "Every flag is at its default." : "The flags are not known to this console."} />
      <p className="mt-3 flex max-w-[52rem] items-start gap-2 text-[12.5px] text-muted-foreground">
        <InfoIcon className="mt-0.5 size-3.5 shrink-0" aria-hidden />
        <span>
          A flag that carries a secret says how many were given and never what. A broker that would be open while looking closed refuses to start (exit code 2): with SASL and nobody who could log in, with a certificate it cannot read, or with the console beyond loopback while SASL or TLS is on.
        </span>
      </p>
    </section>
  );
}

export function SettingsPage() {
  const q = useSettings();
  const qc = useQueryClient();
  const search = useSearch({ from: "/settings" });
  const navigate = useNavigate({ from: "/settings" });
  const overlay = useRoutedOverlay((patch, replace) => void navigate({ search: (s: SettingsSearch) => ({ ...s, ...patch }), replace, resetScroll: false }));
  const [saved, setSaved] = useState<SettingsSaved | null>(null);
  const r = q.data;
  const head = r?.settings.head;
  return (
    <>
      <PageHeader title="Settings" sub="This broker's own configuration, explained. Five settings can change while it runs; everything else was fixed when it started and is shown read only." />
      {q.error && !r ? (
        <PageError error={q.error} onRetry={() => void q.refetch()} />
      ) : !r || !head ? (
        <Skeleton className="h-96 w-full rounded-xl" />
      ) : (
        <>
          <StatStrip
            className="mb-8"
            stats={[
              { label: "Set at run time", value: `${head.set} of 5`, unit: head.set === 0 ? "all at their start values" : "dynamic broker configs", ch: 6 },
              { label: "Running since", value: head.since ?? "–", unit: "UTC", mono: true, ch: 12, tip: head.up ? `Up ${head.up} when this page was fetched` : undefined },
              { label: "Version profile", value: head.profile, unit: "--kafka-version", ch: 8 },
            ]}
          />
          <SectionNav />
          <ConfigsSection r={r} saved={saved} onDismiss={() => setSaved(null)} onEdit={(key, from) => overlay.open({ edit: key, scope: undefined }, from)} />
          <ProfileSection r={r} />
          <IntervalsSection r={r} />
          <WeightSection r={r} />
          <DataSection r={r} />
          <FlagsSection r={r} />
        </>
      )}
      <EditConfigDialog
        configKey={search.edit}
        scope={search.scope === "this" ? "this" : "all"}
        onMove={(key, scope) => overlay.move({ edit: key, scope: scope === "this" ? "this" : undefined })}
        onClose={() => overlay.close({ edit: undefined, scope: undefined })}
        restoreFocus={overlay.restore}
        onSaved={(sv) => {
          setSaved(sv);
          qc.setQueryData<SettingsResponse>(r4keys.settings, (old) => (old ? { ...old, settings: { ...old.settings, head: { ...old.settings.head, set: sv.set }, configs: { ...old.settings.configs, rows: sv.rows } } } : old));
          void qc.invalidateQueries({ queryKey: r4keys.settings });
          overlay.close({ edit: undefined, scope: undefined });
        }}
      />
    </>
  );
}
