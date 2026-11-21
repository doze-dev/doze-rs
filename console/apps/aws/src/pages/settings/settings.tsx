import { useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ThemeSwitch } from "@doze/ui/components/app/theme-menu";
import { Button } from "@doze/ui/components/ui/button";
import { Switch } from "@doze/ui/components/ui/switch";
import { bytes, dateTimeUtc, number } from "@doze/ui/lib/format";
import { useAbout } from "../../api/queries";
import { useSettings } from "../../api/spaces";
import type { FlagRow } from "../../api/types/FlagRow";
import { setDensity, useDensity } from "../../density";
import { authWords, iamWords } from "../../words";
import { Card, CardSkeleton, CodeBlock } from "../card";
import { ResetDialog } from "./reset-dialog";

const HINT = "Check that doze-aws is still running, then try again.";

function Fact({ label, children, note, testId }: { label: string; children: React.ReactNode; note?: string; testId: string }) {
  return (
    <>
      <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{label}</dt>
      <dd className="min-w-0 border-b py-3 text-[13.5px] break-words text-bright" data-testid={testId}>
        {children}
        {note && <span className="mt-0.5 block text-[12.5px] text-muted-foreground">{note}</span>}
      </dd>
    </>
  );
}

const FACTS = "grid grid-cols-[minmax(7.5rem,11rem)_minmax(0,1fr)] [&>dd:last-of-type]:border-b-0 [&>dt:last-of-type]:border-b-0";
const mono = "font-mono text-[13px]";

const FLAG_COLUMNS: DataColumn<FlagRow>[] = [
  { id: "name", header: "Setting", size: 210, accessorKey: "name", meta: { mono: true }, cell: (c) => `--${c.getValue<string>()}` },
  { id: "value", header: "Value", accessorFn: (r) => r.value ?? "", cell: (c) => (c.row.original.value === null ? <span className="text-muted-foreground">hidden (a secret)</span> : <span className="font-mono text-[12.5px]">{c.row.original.value || "(empty)"}</span>) },
  { id: "source", header: "From", size: 260, accessorKey: "source", cell: (c) => <span className={c.row.original.default ? "text-muted-foreground" : "font-medium text-bright"}>{c.getValue<string>()}</span> },
];

/** Settings: this doze-aws's own facts (from `/api/v1/about` and `/api/v1/settings`), every setting it was started with and
 * where each came from, its data directory and size with the reset `doze-aws data reset` does, the Live record's size, and the
 * two choices kept in this browser. Nothing here is a switch the server does not have. */
export function SettingsPage() {
  const about = useAbout();
  const settings = useSettings();
  const density = useDensity();
  const [resetting, setResetting] = useState(false);
  const [changedOnly, setChangedOnly] = useState(false);
  const a = about.data;
  const s = settings.data;
  const flags = useMemo(() => (s ? (changedOnly ? s.flags.filter((f) => !f.default) : s.flags) : []), [s, changedOnly]);
  const served = a?.services.filter((x) => x.served) ?? [];
  return (
    <>
      <PageHeader title="Settings" sub="This doze-aws: how it was started, where it keeps its data, and the choices kept in this browser." refresh={<RefreshControl updatedAt={settings.dataUpdatedAt} fetching={settings.isFetching} onRefresh={() => (void about.refetch(), void settings.refetch())} />} />
      {(about.isError && !a) || (settings.isError && !s) ? (
        <ErrorBlock what="Settings could not be read" error={about.error ?? settings.error} hint={HINT} onRetry={() => (void about.refetch(), void settings.refetch())} retrying={settings.isFetching} testId="settings-error" />
      ) : !a || !s ? (
        <CardSkeleton rows={8} />
      ) : (
        <div className="grid gap-8">
          <Card testId="settings-instance" title="This doze-aws">
            <div className="grid lg:grid-cols-2 lg:gap-x-12">
              <dl className={FACTS}>
                <Fact label="Version" testId="fact-version">
                  <span className={mono}>{a.version}</span>
                </Fact>
                <Fact label="Account" testId="fact-account" note="Every request is for this account.">
                  <span className={mono}>{a.account}</span>
                </Fact>
                <Fact label="Region" testId="fact-region" note={a.regions.length > 1 ? `Calls in Live were signed for: ${a.regions.join(", ")}.` : "The default (--region)."}>
                  <span className={mono}>{a.region}</span>
                </Fact>
                <Fact label="Endpoint" testId="fact-endpoint">
                  <span className={mono}>{a.endpoint}</span>
                </Fact>
                <Fact label="Signatures" testId="fact-auth" note={authWords(a.auth)}>
                  <span className={mono}>{a.auth}</span>
                </Fact>
              </dl>
              <dl className={FACTS}>
                <Fact label="IAM" testId="fact-iam" note={iamWords(a.iam).sentence}>
                  <span className={mono}>{a.iam}</span>
                </Fact>
                <Fact label="Console" testId="fact-console" note="This page's own address. It has no login and answers this machine only. Its calls use the key console, which acts as the account root: IAM allows them in every mode.">
                  <span className={mono}>{a.console}</span>
                </Fact>
                <Fact label="Process" testId="fact-process" note={`Started ${dateTimeUtc(a.started * 1000)}.`}>
                  <span className={mono}>pid {a.pid}</span>
                </Fact>
                <Fact label="Services served" testId="fact-served" note="--services; the others answer every call with how to turn them on.">
                  {served.length === a.services.length ? `All ${number(served.length)}` : served.map((x) => x.title).join(", ")}
                </Fact>
              </dl>
            </div>
          </Card>

          <Card
            testId="settings-flags"
            title="How it was started"
            lede="Every setting, its value, and where the value came from: a flag, a DOZE_AWS_* variable, doze-aws.toml, or the default. They are read at start; change one and start doze-aws again."
            actions={
              <label className="flex items-center gap-2 text-[13px] text-muted-foreground">
                <Switch checked={changedOnly} onCheckedChange={setChangedOnly} data-testid="flags-changed-only" /> Set ones only
              </label>
            }
          >
            <DataTable aria-label="Settings" data={flags} columns={FLAG_COLUMNS} rowId={(f) => f.name} maxHeight={4000} empty={changedOnly ? "Every setting is its default." : "This doze-aws did not hand its settings to the console."} />
          </Card>

          <Card
            testId="settings-data"
            title="Data"
            lede="Where state is kept on this machine, and what each service keeps there (the report doze-aws data prints)."
            actions={
              s.reset ? (
                <Button variant="outline" className="border-destructive/40 bg-card text-destructive" onClick={() => setResetting(true)} data-testid="reset-open">
                  Reset local data
                </Button>
              ) : undefined
            }
          >
            <dl className={FACTS}>
              <Fact label="Directory" testId="fact-data-dir">
                <span className={`${mono} break-all`}>{s.data_dir}</span>
              </Fact>
              <Fact label="Size" testId="fact-data-size" note={s.data_error ?? undefined}>
                {s.data_bytes === null ? "Not known" : bytes(s.data_bytes)}
              </Fact>
              <Fact label="By service" testId="fact-data-services">
                {s.data.filter((d) => d.bytes > 0).length === 0 ? <span className="text-muted-foreground">No service keeps anything yet.</span> : s.data.filter((d) => d.bytes > 0).map((d) => `${d.title} ${bytes(d.bytes)}`).join(" · ")}
              </Fact>
            </dl>
            {!s.reset && (
              <div className="mt-5">
                <p className="mb-2 text-[13.5px]">This console cannot reset the data here; from a shell:</p>
                <CodeBlock code="doze-aws data reset" what="Command copied" />
              </div>
            )}
          </Card>

          <Card testId="settings-live" title="Live record" lede="The newest calls, kept in memory for Live, Home and Access; nothing of it is written to disk.">
            <dl className={FACTS}>
              <Fact label="Holds" testId="fact-ring-held">
                {number(s.ring.held)} of {number(s.ring.capacity)} calls ({number(s.ring.recorded)} since start)
              </Fact>
              <Fact label="Memory" testId="fact-ring-bytes" note={`At most ${bytes(s.ring_most_bytes)} when every call carries the longest text it may.`}>
                {bytes(s.ring.bytes)}
              </Fact>
            </dl>
          </Card>

          <Card testId="settings-browser" title="This browser" lede="Kept in this browser only; doze-aws does not know them.">
            <dl className={FACTS}>
              <Fact label="Theme" testId="pref-theme">
                <ThemeSwitch />
              </Fact>
              <Fact label="Compact tables" testId="pref-density" note="Shorter rows in every table, from the next one opened.">
                <Switch checked={density === "compact"} onCheckedChange={(on) => setDensity(on ? "compact" : "comfortable")} data-testid="density-switch" aria-label="Compact tables" />
              </Fact>
            </dl>
          </Card>
        </div>
      )}
      {resetting && <ResetDialog open={resetting} onOpenChange={setResetting} />}
    </>
  );
}
