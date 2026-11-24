// Parameter Store's other two tabs, the ones the AWS console has beside the parameters: the public parameters AWS publishes under
// /aws/service (GetParametersByPath), and the service settings (GetServiceSetting, UpdateServiceSetting, ResetServiceSetting).
import { useNavigate } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { PencilIcon, RotateCcwIcon, SearchIcon } from "lucide-react";
import { useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { callKindOp, Field, SectionEdit } from "../../../kit";
import { Card, Note } from "../cfg-parts";
import { HINT } from "../index-page";
import { PARAMETERS } from "./form";

export type Tab = "parameters" | "public" | "settings";

/** The three tabs over the Parameter Store pages. */
export function ParameterTabs({ current }: { current: Tab }) {
  const navigate = useNavigate();
  const go = (tab: Tab) => void navigate({ to: "/resources/$service/$second", params: { service: "ssm", second: "parameters" }, search: tab === "parameters" ? {} : { mode: tab } });
  return (
    <Tabs value={current} onValueChange={(v) => go(v as Tab)} className="mb-6 gap-0">
      <TabsList variant="line" className="h-11 w-full justify-start gap-2 rounded-none border-b p-0" aria-label="Parameter Store">
        <TabsTrigger value="parameters" className="h-11 flex-none px-3 text-[14px] after:bottom-[-1px]" data-testid="tab-parameters">My parameters</TabsTrigger>
        <TabsTrigger value="public" className="h-11 flex-none px-3 text-[14px] after:bottom-[-1px]" data-testid="tab-public">Public parameters</TabsTrigger>
        <TabsTrigger value="settings" className="h-11 flex-none px-3 text-[14px] after:bottom-[-1px]" data-testid="tab-settings">Settings</TabsTrigger>
      </TabsList>
    </Tabs>
  );
}

const crumbs = [{ label: "Resources", to: "/resources" }, { label: "Parameter Store", to: "/resources/ssm" }];

type Found = { Name: string; Value?: string; Version?: number; ARN?: string };
type Page = { Parameters?: Found[]; NextToken?: string };

const ROOTS = ["/aws/service/ami-amazon-linux-latest", "/aws/service/global-infrastructure/regions", "/aws/service/ecs/optimized-ami", "/aws/service/bottlerocket"];

/** The public parameters under a path, ten at a time as GetParametersByPath gives them. */
export function PublicParameters() {
  const [typed, setTyped] = useState(ROOTS[0]!);
  const [path, setPath] = useState(ROOTS[0]!);
  const [pages, setPages] = useState<Page[]>([]);
  const [token, setToken] = useState<string | undefined>(undefined);
  const q = useQuery({
    queryKey: ["resources", "kindop", "ssm", "parameters", "GetParametersByPath", path, token ?? ""],
    queryFn: async ({ signal }) => {
      const r = await callKindOp<Page>(PARAMETERS, "GetParametersByPath", { Path: path, Recursive: true, MaxResults: 10, ...(token ? { NextToken: token } : {}) }, signal);
      setPages((p) => (token ? [...p, r] : [r]));
      return r;
    },
    retry: false,
    staleTime: 60_000,
  });
  const rows = pages.flatMap((p) => p.Parameters ?? []);
  const last = pages[pages.length - 1];
  const columns: DataColumn<Found>[] = [
    { id: "name", header: "Name", size: 520, min: 280, accessorKey: "Name", meta: { mono: true } },
    { id: "value", header: "Value", min: 200, accessorFn: (r) => r.Value ?? "", meta: { mono: true } },
    { id: "version", header: "Version", size: 100, fit: 70, accessorFn: (r) => r.Version ?? 0, meta: { numeric: true } },
  ];
  const look = (p: string) => { setPages([]); setToken(undefined); setPath(p.trim() || "/aws/service/"); };
  return (
    <>
      <PageHeader title="Parameter Store" crumbs={crumbs} sub="Public parameters are names AWS publishes for every account, for example the latest Amazon Linux image. They are read-only." />
      <ParameterTabs current="public" />
      <form className="mb-4 flex flex-wrap items-center gap-2" role="search" aria-label="Public parameters under a path" onSubmit={(e) => { e.preventDefault(); look(typed); }}>
        <div className="relative w-full max-w-[34rem] min-w-64 flex-1">
          <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input value={typed} onChange={(e) => setTyped(e.target.value)} list="public-roots" placeholder="/aws/service/ami-amazon-linux-latest" aria-label="Path" className="bg-card pl-9 font-mono text-[13px]" data-testid="public-path" />
          <datalist id="public-roots">{ROOTS.map((r) => <option key={r} value={r} />)}</datalist>
        </div>
        <Button type="submit" variant="outline" className="bg-card" data-testid="public-look">Show parameters</Button>
      </form>
      {q.isError && rows.length === 0 ? (
        <ErrorBlock what="The public parameters could not be read" error={q.error} hint={HINT} onRetry={() => void q.refetch()} retrying={q.isFetching} testId="public-error" />
      ) : q.isPending && rows.length === 0 ? (
        <Skeleton className="h-64 rounded-xl" data-testid="public-loading" />
      ) : rows.length === 0 ? (
        <p className="rounded-[var(--radius-panel)] border border-dashed px-5 py-6 text-[13.5px] text-muted-foreground" data-testid="public-none">GetParametersByPath found no public parameter under <span className="font-mono text-foreground">{path}</span>. Names start with <span className="font-mono text-foreground">/aws/service/</span>.</p>
      ) : (
        <div className="grid gap-3">
          <DataTable aria-label="Public parameters" data={rows} columns={columns} rowId={(r) => r.Name} maxHeight={560} />
          <div className="flex items-center justify-between gap-3 text-[13px] text-muted-foreground">
            <span data-testid="public-count">{rows.length} public parameter{rows.length === 1 ? "" : "s"} under {path}{last?.NextToken ? ", and more" : ""}.</span>
            {last?.NextToken && <Button type="button" variant="outline" className="bg-card" disabled={q.isFetching} onClick={() => setToken(last.NextToken)} data-testid="public-more">{q.isFetching ? "Loading…" : "Load more"}</Button>}
          </div>
        </div>
      )}
      <Note className="mt-5" testId="public-note">These come from GetParametersByPath. doze-aws serves a small table of the names in common use (Amazon Linux, ECS and EKS images, Bottlerocket, Ubuntu, Windows Server, and the Regions and services of <span className="font-mono">global-infrastructure</span>). The image ids are placeholders shaped like AWS&apos;s: they are not real images and launch nothing, so a template or a CDK app that looks one up deploys.</Note>
    </>
  );
}

type Setting = { SettingId: string; SettingValue?: string; Status?: string; LastModifiedDate?: number; LastModifiedUser?: string };

/** The two settings of Parameter Store, and what each takes. */
const SETTINGS: { id: string; title: string; values: string[]; about: string }[] = [
  { id: "/ssm/parameter-store/default-parameter-tier", title: "Default parameter tier", values: ["Standard", "Advanced", "Intelligent-Tiering"], about: "The tier of a parameter written without one. doze-aws applies it." },
  { id: "/ssm/parameter-store/high-throughput-enabled", title: "Parameter Store throughput", values: ["true", "false"], about: "Whether higher throughput is on. doze-aws keeps the setting and answers it, and does not throttle either way." },
];

const useSetting = (id: string) => useQuery({ queryKey: ["resources", "kindop", "ssm", "parameters", "GetServiceSetting", id], queryFn: ({ signal }) => callKindOp<{ ServiceSetting?: Setting }>(PARAMETERS, "GetServiceSetting", { SettingId: id }, signal), retry: false });

function Row({ s }: { s: (typeof SETTINGS)[number] }) {
  const qc = useQueryClient();
  const q = useSetting(s.id);
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const got = q.data?.ServiceSetting;
  const change = async (call: "UpdateServiceSetting" | "ResetServiceSetting", input: object) => {
    setPending(true);
    setError(null);
    try {
      await callKindOp(PARAMETERS, call, input);
      await qc.invalidateQueries({ queryKey: ["resources", "kindop", "ssm", "parameters", "GetServiceSetting", s.id] });
      setOpen(false);
    } catch (e) {
      setError(e);
    } finally {
      setPending(false);
    }
  };
  const custom = got?.Status === "Customized";
  return (
    <Card title={s.title} lede={s.about} testId={`setting-${s.id.split("/").pop()}`} actions={
      <>
        <Button type="button" variant="outline" className="bg-card" disabled={!got} onClick={() => { setValue(got?.SettingValue ?? s.values[0]!); setError(null); setOpen(true); }} data-testid="setting-edit"><PencilIcon /> Edit</Button>
        <Button type="button" variant="outline" className="bg-card" disabled={!custom || pending} onClick={() => void change("ResetServiceSetting", { SettingId: s.id })} data-testid="setting-reset"><RotateCcwIcon /> Reset to default</Button>
      </>
    }>
      {q.isError ? <p className="text-[13.5px] text-bad">The setting could not be read.</p> : !got ? <Skeleton className="h-16 rounded-xl" /> : (
        <dl className="grid grid-cols-[minmax(8rem,13rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0">
          {([["Setting ID", <span key="i" className="font-mono text-[13px]">{got.SettingId}</span>], ["Value", <b key="v" className="font-semibold text-bright" data-testid="setting-value">{got.SettingValue ?? "–"}</b>], ["Status", got.Status ?? "–"], ["Last modified", got.LastModifiedDate ? `${dateTimeUtc(got.LastModifiedDate * 1000)}${got.LastModifiedUser ? ` by ${got.LastModifiedUser}` : ""}` : "Never changed"]] as [string, React.ReactNode][]).map(([k, v]) => (
            <div key={k} className="contents"><dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{k}</dt><dd className="border-b py-3 text-[13.5px] text-bright">{v}</dd></div>
          ))}
        </dl>
      )}
      <SectionEdit open={open} onOpenChange={setOpen} title={`Edit ${s.title.toLowerCase()}`} request={{ service: "ssm", operation: "UpdateServiceSetting", input: { SettingId: s.id, SettingValue: value } }} onSave={() => void change("UpdateServiceSetting", { SettingId: s.id, SettingValue: value })} pending={pending} error={error} disabled={value === got?.SettingValue} disabledWhy="Nothing has changed." testId="edit-setting">
        <Field label="Value">
          {(id) => (
            <Select value={value} onValueChange={setValue}>
              <SelectTrigger id={id} className="w-64 bg-card" data-testid="setting-select"><SelectValue /></SelectTrigger>
              <SelectContent>{s.values.map((v) => <SelectItem key={v} value={v}>{v}</SelectItem>)}</SelectContent>
            </Select>
          )}
        </Field>
      </SectionEdit>
    </Card>
  );
}

/** The service settings of Parameter Store. */
export function ServiceSettings() {
  return (
    <>
      <PageHeader title="Parameter Store" crumbs={crumbs} sub="Settings of the account that Parameter Store reads. A setting holds its default until it is changed." />
      <ParameterTabs current="settings" />
      <div className="grid gap-5" data-testid="settings-tab">
        {SETTINGS.map((s) => <Row key={s.id} s={s} />)}
      </div>
      <Note className="mt-5" testId="settings-note">Each setting is one call: GetServiceSetting reads it, UpdateServiceSetting changes it and ResetServiceSetting puts the default back. Parameter Store has two; the others of AWS&apos;s service settings belong to other Systems Manager features.</Note>
    </>
  );
}
