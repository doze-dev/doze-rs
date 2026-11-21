// Lambda's layers: the list (lambda__layers-list, layers-none), Create layer (layer-create) and a layer's page with its versions.
import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { useQueries } from "@tanstack/react-query";
import { LayersIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { useRef, useState } from "react";
import { Alert, ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { MultiSelect } from "@doze/ui/components/app/multi-select";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { bytes as sizeOf } from "@doze/ui/lib/format";
import { useResourceDetail, useResourceList } from "../../../api/resources";
import type { ResourceList } from "../../../api/types/ResourceList";
import { callKindOp, Field, FormPage, FormSection, useKindOp } from "../../../kit";
import { callOp, useOp } from "../deep/api";
import { Card, Confirm, Loaded } from "../deep/parts";
import { HINT } from "../index-page";
import { ResourceTo, Value } from "../value";
import { type LayerForm, layerInput, layerProblems } from "./forms";
import { LAYERS, refusalOf, useLayers, type LayerRow } from "./hooks";
import { layersSentence, rowsOf } from "./list-model";
import { RUNTIMES } from "./model";
import { toBase64 } from "./zip";

const crumbs = [{ label: "Resources", to: "/resources" }, { label: "Compute and flow", to: "/resources" }, { label: "Lambda", to: "/resources/lambda" }];

const COLUMNS: DataColumn<LayerRow>[] = [
  { id: "name", header: "Layer name", min: 160, accessorFn: (r) => r.name, meta: { mono: true }, cell: (c) => <ResourceTo to={{ service: "lambda", kind: "layers", name: c.row.original.name }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.name}</ResourceTo> },
  { id: "latest", header: "Latest version", size: 140, fit: 112, accessorFn: (r) => Number(r.latest), meta: { numeric: true } },
  { id: "description", header: "Description", size: 260, min: 140, accessorFn: (r) => r.description, cell: (c) => c.row.original.description || <span className="text-muted-foreground">-</span> },
  { id: "runtimes", header: "Compatible runtimes", size: 220, fit: 160, accessorFn: (r) => r.runtimes.join(", "), meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.runtimes.join(", ") || "-"}</span> },
  { id: "archs", header: "Compatible architectures", size: 210, fit: 190, accessorFn: (r) => r.archs.join(", "), meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.archs.join(", ") || "-"}</span> },
  { id: "created", header: "Created", size: 190, fit: 90, accessorFn: (r) => r.created, cell: (c) => <Value value={c.row.original.created} format="time" /> },
];

export function LayersList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { create?: string; gone?: string };
  if (search.create === "yes") return <LayerCreate />;
  return <Layers service={service} kind={kind} gone={search.gone} />;
}

function Layers({ service, kind, gone }: { service: string; kind: string; gone?: string }) {
  const navigate = useNavigate();
  const list = useResourceList(service, kind);
  const layers = useLayers();
  const functions = useResourceList("lambda", "functions");
  const first = list.data?.pages[0];
  const rows = layers.rows;
  const counts = useQueries({ queries: rows.map((r) => ({ queryKey: ["lambda", "layer-versions", r.name], queryFn: () => callKindOp<{ LayerVersions?: unknown[] }>(LAYERS, "ListLayerVersions", { LayerName: r.name }), retry: false })) });
  const used = rowsOf(functions.data?.pages.flatMap((p: ResourceList) => p.rows) ?? []).filter((f) => f.layers.some((a) => rows.some((l) => a.includes(`:layer:${l.name}:`)))).length;
  const sentence = layersSentence(rows.map((r, i) => ({ name: r.name, versions: counts[i]?.data ? (counts[i]?.data?.LayerVersions?.length ?? 0) : null })), used);
  const go = (next: { create?: "yes"; gone?: string }) => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: next });
  const create = <Button onClick={() => go({ create: "yes" })} data-testid="create-open"><PlusIcon /> Create layer</Button>;
  return (
    <>
      <PageHeader title="Layers" crumbs={[...crumbs]} sub="Layers in this account. A layer is a .zip file archive that contains supplementary code or data." refresh={<RefreshControl updatedAt={list.dataUpdatedAt} fetching={list.isFetching} onRefresh={() => void list.refetch()} interval={false} />} actions={create} />
      {gone && <ResultBanner className="mb-6" title={`${gone} was deleted.`} returned="DeleteLayerVersion removed its last version, so the layer is gone." onDismiss={() => go({})} testId="deleted-banner" />}
      {list.isError && !first ? (
        <ErrorBlock what="The layers could not be listed" error={list.error} hint={HINT} onRetry={() => void list.refetch()} retrying={list.isFetching} testId="list-error" />
      ) : !first ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-64 rounded-xl" /></div>
      ) : rows.length === 0 ? (
        <div data-testid="list-empty"><EmptyState icon={LayersIcon} title="No layers" action={create}>A layer packages libraries or data that functions share.</EmptyState></div>
      ) : (
        <>
          {sentence && <p className="mb-5 max-w-[62rem] text-[15px] leading-6 text-muted-foreground" data-testid="list-sentence">{sentence}</p>}
          <DataTable aria-label="Layers" data={rows} columns={COLUMNS} rowId={(r) => r.name} maxHeight={680} />
          <p className="mt-3 text-[13px] text-muted-foreground">The columns are fields of ListLayers: LayerName and, from LatestMatchingVersion, Version, Description, CompatibleRuntimes, CompatibleArchitectures and CreatedDate.</p>
          <Pagination className="mt-3" shown={rows.length} total={rows.length} more={list.hasNextPage} onMore={() => void list.fetchNextPage()} loading={list.isFetchingNextPage} noun={rows.length === 1 ? "layer" : "layers"} />
        </>
      )}
    </>
  );
}

const ARCHS = ["x86_64", "arm64"];

export function LayerCreate() {
  const navigate = useNavigate();
  const layers = useLayers();
  const [l, setL] = useState<LayerForm>({ name: "", description: "", license: "", archs: [], runtimes: [] });
  const [source, setSource] = useState<"zip" | "s3">("zip");
  const [file, setFile] = useState<{ name: string; size: number; base64: string } | null>(null);
  const [s3, setS3] = useState({ bucket: "", key: "" });
  const [tried, setTried] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const publish = useKindOp<{ LayerName?: string; Version: number }>(LAYERS, "PublishLayerVersion");
  const set = (patch: Partial<LayerForm>) => setL((x) => ({ ...x, ...patch }));
  const existing = layers.rows.find((r) => r.name === l.name);
  const hasCode = source === "zip" ? Boolean(file) : Boolean(s3.bucket && s3.key);
  const problems = layerProblems(l, hasCode);
  const content = (zip: string) => (source === "zip" ? { ZipFile: zip } : { S3Bucket: s3.bucket, S3Key: s3.key });
  const shown = { ...layerInput(l, ""), Content: content(file ? `<>` : "<the .zip file>") };
  const refused = refusalOf(publish.error);
  const pick = async (f: File | undefined) => {
    if (!f) return;
    setFile({ name: f.name, size: f.size, base64: toBase64(new Uint8Array(await f.arrayBuffer())) });
  };
  const submit = () => {
    setTried(true);
    if (Object.keys(problems).length > 0) return;
    publish.mutate({ ...layerInput(l, ""), Content: content(file?.base64 ?? "") }, { onSuccess: (r) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "lambda", kind: "layers", name: l.name }, search: { done: `published-${r.Version}` } }) });
  };
  return (
    <FormPage title="Create layer" sub="Upload an archive and name it. The first version of a new layer is version 1." crumbs={[...crumbs, { label: "Layers", to: "/resources/lambda/layers" }]}
      request={l.name ? { service: "lambda", operation: "PublishLayerVersion", input: shown } : null} showInput refusal={refused} callNote={null}
      banner={refused ? <Alert tone="bad" title="Layer not created." testId="create-refused">{refused.code}: {refused.message}</Alert> : undefined}
      submitLabel="Create" pending={publish.isPending} disabled={tried && Object.keys(problems).length > 0} disabledWhy={Object.values(problems)[0]}
      onCancel={() => void navigate({ to: "/resources/$service/$second", params: { service: "lambda", second: "layers" }, search: {} })} onSubmit={submit} testId="layer-create">
      <FormSection title="Layer configuration" testId="section-layer">
        <Field label="Name" error={tried ? problems.name : undefined} help={<>Layer names are letters, digits, hyphens and underscores.{existing ? <span className="text-muted-foreground"> Publishing under the name {existing.name} again would make version {Number(existing.latest) + 1}.</span> : null}</>} testId="field-name">
          {(id) => <Input id={id} value={l.name} onChange={(e) => set({ name: e.target.value })} autoComplete="off" spellCheck={false} className="max-w-[24rem] font-mono" data-testid="layer-name" />}
        </Field>
        <Field label={<>Description <span className="font-normal text-muted-foreground">Optional</span></>} error={tried ? problems.description : undefined} help="Up to 256 characters." testId="field-description">
          {(id) => <Input id={id} value={l.description} onChange={(e) => set({ description: e.target.value })} autoComplete="off" data-testid="layer-description" />}
        </Field>
      </FormSection>
      <FormSection title="Layer code" testId="section-code">
        <RadioGroup value={source} onValueChange={(v) => setSource(v as "zip" | "s3")} className="grid gap-2" aria-label="Where the code comes from">
          {[["zip", "Upload a .zip file"], ["s3", "Upload a file from Amazon S3"]].map(([v, t]) => (
            <Label key={v} htmlFor={`src-${v}`} className="flex cursor-pointer items-center gap-3 rounded-lg border bg-card px-4 py-3 font-normal has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40"><RadioGroupItem id={`src-${v}`} value={v!} data-testid={`layer-source-${v}`} />{t}</Label>
          ))}
        </RadioGroup>
        {source === "zip" ? (
          <Field label="Upload a .zip file" error={tried ? problems.file : undefined} testId="field-file">
            {() => (
              <div className="flex flex-wrap items-center gap-3">
                <input ref={input} type="file" accept=".zip,application/zip" className="sr-only" onChange={(e) => void pick(e.target.files?.[0])} data-testid="layer-file" aria-label="Upload a .zip file" />
                <Button type="button" variant="outline" className="bg-card" onClick={() => input.current?.click()}>Choose file</Button>
                <span className="text-[13.5px] text-muted-foreground" data-testid="layer-file-name">{file ? `${file.name}, ${sizeOf(file.size)}` : "No file chosen."}</span>
              </div>
            )}
          </Field>
        ) : (
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="Bucket" help="The S3 bucket the archive is in." testId="field-bucket">{(id) => <Input id={id} value={s3.bucket} onChange={(e) => setS3({ ...s3, bucket: e.target.value })} className="font-mono" data-testid="layer-bucket" />}</Field>
            <Field label="Key" help="The archive's object key." testId="field-key">{(id) => <Input id={id} value={s3.key} onChange={(e) => setS3({ ...s3, key: e.target.value })} className="font-mono" data-testid="layer-key" />}</Field>
          </div>
        )}
        <p className="border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground">Lambda extracts the archive into /opt when it sets up the execution environment. A layer version never changes; to change the content you publish a new version. Locally the layer&apos;s folders go onto the interpreter search paths instead of /opt.</p>
      </FormSection>
      <FormSection title="Compatibility" testId="section-compat">
        <div className="grid gap-1.5">
          <Label>Compatible architectures <span className="font-normal text-muted-foreground">Optional</span></Label>
          <div className="flex gap-6">
            {ARCHS.map((a) => (
              <Label key={a} htmlFor={`arch-${a}`} className="flex cursor-pointer items-center gap-2 font-mono font-normal"><Checkbox id={`arch-${a}`} checked={l.archs.includes(a)} onCheckedChange={(on) => set({ archs: on ? [...l.archs, a] : l.archs.filter((x) => x !== a) })} data-testid={`layer-arch-${a}`} />{a}</Label>
            ))}
          </div>
        </div>
        <Field label={<>Compatible runtimes <span className="font-normal text-muted-foreground">Optional</span></>} error={tried ? problems.runtimes : undefined} help="Up to 15 runtimes. They are used to filter ListLayers and ListLayerVersions." testId="field-runtimes">
          {() => <MultiSelect options={RUNTIMES.map((r) => ({ value: r.id, label: r.id }))} value={l.runtimes} onChange={(runtimes) => set({ runtimes })} noun="runtimes" triggerClassName="w-72" aria-label="Compatible runtimes" />}
        </Field>
        <Field label={<>License <span className="font-normal text-muted-foreground">Optional</span></>} error={tried ? problems.license : undefined} help="An SPDX identifier, a URL, or the license text; up to 512 characters." testId="field-license">
          {(id) => <Input id={id} value={l.license} onChange={(e) => set({ license: e.target.value })} autoComplete="off" className="max-w-[24rem] font-mono" data-testid="layer-license" />}
        </Field>
      </FormSection>
    </FormPage>
  );
}

export function LayerDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const navigate = useNavigate();
  const search = useSearch({ strict: false }) as { done?: string };
  const detail = useResourceDetail(service, kind, name);
  const versions = detail.data?.subs.find((s) => s.title === "Versions");
  const [gone, setGone] = useState<number | null>(null);
  const remove = useOp(LAYER_TARGET(name), "DeleteLayerVersion");
  const done = search.done?.startsWith("published-") ? Number(search.done.slice(10)) : null;
  const rows = versions?.rows ?? [];
  const columns: DataColumn<(typeof rows)[number]>[] = [
    { id: "version", header: "Version", size: 110, accessorFn: (r) => Number(r.cells[0]), meta: { numeric: true } },
    { id: "description", header: "Description", size: 260, accessorFn: (r) => r.cells[1] ?? "", cell: (c) => c.row.original.cells[1] || <span className="text-muted-foreground">-</span> },
    { id: "created", header: "Created", size: 200, accessorFn: (r) => r.cells[2] ?? "", cell: (c) => <Value value={c.row.original.cells[2]} format="time" /> },
    { id: "arn", header: "ARN", min: 260, accessorFn: (r) => r.cells[3] ?? "", meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.cells[3]}</span> },
    { id: "delete", header: "", size: 60, enableSorting: false, cell: (c) => <Button type="button" variant="ghost" size="icon-sm" aria-label={`Delete version ${c.row.original.cells[0]}`} onClick={() => setGone(Number(c.row.original.cells[0]))} data-testid="layer-version-delete"><Trash2Icon /></Button> },
  ];
  if (detail.isError && !detail.data) return <ErrorBlock what="This layer could not be read" error={detail.error} hint={HINT} onRetry={() => void detail.refetch()} retrying={detail.isFetching} testId="detail-error" />;
  if (!detail.data) return <Skeleton className="h-72 rounded-[14px]" />;
  const last = rows.length <= 1;
  return (
    <>
      <PageHeader title={name} mono crumbs={[...crumbs, { label: "Layers", to: "/resources/lambda/layers" }]} sub={detail.data.fields.find((f) => f.label === "Description")?.value ?? undefined} actions={<Button variant="outline" className="bg-card" onClick={() => void navigate({ to: "/resources/$service/$second", params: { service, second: "layers" }, search: { create: "yes" } })}><PlusIcon /> Publish new version</Button>} />
      {done && <ResultBanner className="mb-6" title={`${name} version ${done} was published.`} returned="PublishLayerVersion made a version that never changes." onDismiss={() => void navigate({ to: "/resources/$service/$kind/$name", params: { service, kind, name }, search: {} })} testId="published-banner" />}
      <Card title="Versions" lede="Layer versions never change; a function uses one by its ARN." testId="layer-versions">
        <Loaded q={{ data: versions, error: null, isFetching: false, refetch: () => undefined }} what="The versions">{() => <DataTable aria-label="Layer versions" data={rows} columns={columns} rowId={(r) => r.cells[0] ?? r.name} maxHeight={420} />}</Loaded>
      </Card>
      <Confirm open={gone !== null} onOpenChange={(o) => { if (!o) { setGone(null); remove.reset(); } }} title="Delete layer version" verb="Delete" operation="DeleteLayerVersion" pending={remove.isPending} error={remove.error}
        run={() => remove.mutate({ VersionNumber: gone }, { onSuccess: () => { setGone(null); if (last) void navigate({ to: "/resources/$service/$second", params: { service, second: "layers" }, search: { gone: name } }); } })}>
        Delete version {gone} of <b className="font-mono font-semibold text-bright">{name}</b>? Functions that use it keep working until they are updated; {last ? "this is the last version, so the layer is gone too" : "the other versions stay"}.
      </Confirm>
      <Link to="/resources/$service/$second" params={{ service, second: "layers" }} search={{}} className="sr-only">Layers</Link>
    </>
  );
}

const LAYER_TARGET = (name: string) => ({ service: "lambda", kind: "layers", name });
void callOp;
