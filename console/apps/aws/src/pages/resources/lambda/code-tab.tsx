// A function's Code tab (lambda__function-code): its package read in the page, the one file edited and deployed, the runtime
// settings, and the layers. Deploy sends the archive again (the files you changed, the others as they were) with UpdateFunctionCode.
import { useQuery } from "@tanstack/react-query";
import { FileIcon, FolderIcon, LayersIcon, PlusIcon } from "lucide-react";
import { useState } from "react";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { cn } from "@doze/ui/lib/utils";
import { client } from "../../../api/client";
import { RESOURCE_PATHS } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { on, useOp } from "../deep/api";
import { Card, Refused } from "../deep/parts";
import { RecentCalls } from "../../../kit";
import { CodeEditor } from "./editor";
import { type Go, useLayers } from "./hooks";
import { type Fn, familyOf, interpreterOf, runtimeName, tailOf } from "./model";
import { buildZip, isText, readZip, toBase64, type ZipFile } from "./zip";

const MOST_SHOWN = 400_000;

/** The package of a function: its files, read in the page from the zip the console hands over (`…/code`). */
async function readPackage(name: string): Promise<ZipFile[]> {
  const res = await fetch(`${client.basePath}${RESOURCE_PATHS.one("lambda", "functions", name)}/code`, { headers: { "X-Console": "1" }, credentials: "same-origin" });
  if (!res.ok) {
    const body = (await res.json().catch(() => null)) as { message?: string } | null;
    throw new Error(body?.message ?? `The package could not be read (${res.status}).`);
  }
  return readZip(new Uint8Array(await res.arrayBuffer()));
}

/** The file a handler names (`lambda_function.lambda_handler` is `lambda_function.py`), else the first one that is text. */
export function handlerFile(files: readonly ZipFile[], handler: string): string | null {
  const module = handler.split(".").slice(0, -1).join(".");
  const text = files.filter((f) => isText(f.data) && f.data.length <= MOST_SHOWN);
  return (text.find((f) => f.path.replace(/\.[^./]+$/, "") === module) ?? text[0])?.path ?? null;
}

function Row({ k, v, mono = false }: { k: string; v: React.ReactNode; mono?: boolean }) {
  return (
    <div className="contents">
      <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{k}</dt>
      <dd className={cn("border-b py-3 text-[13.5px] break-words text-bright", mono && "font-mono text-[13px]")}>{v}</dd>
    </div>
  );
}

export function CodeTab({ d, f, go }: { d: ResourceDetail; f: Fn; go: (to: Go) => void }) {
  const pkg = useQuery({ queryKey: ["lambda", "package", d.name, f.lastModified], queryFn: () => readPackage(d.name), retry: false, staleTime: Number.POSITIVE_INFINITY });
  const [edits, setEdits] = useState<Record<string, string>>({});
  const [picked, setPicked] = useState<string | null>(null);
  const deploy = useOp<unknown>(on.fn(d.name), "UpdateFunctionCode");
  const files = pkg.data ?? [];
  const current = picked ?? handlerFile(files, f.handler);
  const text = (path: string) => edits[path] ?? new TextDecoder().decode(files.find((x) => x.path === path)?.data);
  const changed = Object.keys(edits).filter((p) => edits[p] !== new TextDecoder().decode(files.find((x) => x.path === p)?.data));
  const layers = useLayers();
  const newest = [...layers.rows].sort((a, b) => b.created.localeCompare(a.created))[0];
  const envs = Object.entries(f.env);
  const send = () => {
    const out = files.map((x) => (edits[x.path] !== undefined && changed.includes(x.path) ? { ...x, data: new TextEncoder().encode(edits[x.path]) } : x));
    deploy.mutate({ ZipFile: toBase64(buildZip(out)) }, { onSuccess: () => go({ tab: "code", done: "deployed" }) });
  };
  const family = familyOf(f.runtime);
  const interpreter = interpreterOf(f.runtime);
  return (
    <div className="grid gap-6" data-testid="code-tab">
      <Card title="Code source" lede="Edit the code here, then choose Deploy to update your function's code." testId="code-source">
        {f.pkg === "Image" ? (
          <EmptyTab title="A container image" testId="code-image">{d.name} is deployed as a container image, which doze-aws stores and never runs: there is no code to show.</EmptyTab>
        ) : pkg.isPending ? (
          <Skeleton className="h-64 rounded-[12px]" data-testid="code-loading" />
        ) : pkg.error || !current ? (
          <EmptyTab title="No code to show" testId="code-none">{pkg.error ? (pkg.error as Error).message : "The package holds no text file to show."}</EmptyTab>
        ) : (
          <>
            <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_17rem]">
              <div className="min-w-0">
                <p className="mb-2 text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase" data-testid="code-file">{current}</p>
                <CodeEditor value={text(current)} onChange={(v) => setEdits({ ...edits, [current]: v })} family={family} label={`Source of ${current}`} />
              </div>
              <div className="grid content-start gap-6">
                <div>
                  <h3 className="mb-2 text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">Explorer</h3>
                  <ul className="grid gap-0.5 text-[13.5px]" data-testid="code-explorer">
                    <li className="flex items-center gap-2 py-1 text-bright"><FolderIcon aria-hidden className="size-4 text-muted-foreground" />{d.name}</li>
                    {files.map((x) => {
                      const editable = isText(x.data) && x.data.length <= MOST_SHOWN;
                      return (
                        <li key={x.path}>
                          <button type="button" disabled={!editable} onClick={() => setPicked(x.path)} aria-current={x.path === current ? "true" : undefined} className={cn("flex w-full items-center gap-2 rounded-md py-1 pl-4 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50 disabled:text-muted-foreground", x.path === current ? "text-bright" : "text-foreground hover:bg-accent")} data-testid="code-file-link">
                            <FileIcon aria-hidden className="size-4 shrink-0 text-muted-foreground" />
                            <span className="truncate">{x.path}</span>
                            {changed.includes(x.path) && <span className="ml-auto text-[11.5px] text-warn">edited</span>}
                          </button>
                        </li>
                      );
                    })}
                  </ul>
                </div>
                <div>
                  <h3 className="mb-2 text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">Environment variables</h3>
                  {envs.length === 0 ? (
                    <p className="text-[13.5px] text-muted-foreground" data-testid="code-env-none">None.</p>
                  ) : (
                    <dl className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0" data-testid="code-env">
                      {envs.map(([k, v]) => (
                        <div key={k} className="contents">
                          <dt className="truncate border-b py-2 pr-2 font-mono text-[12.5px] text-muted-foreground" title={k}>{k}</dt>
                          <dd className="truncate border-b py-2 font-mono text-[12.5px] text-bright" title={v}>{v}</dd>
                        </div>
                      ))}
                    </dl>
                  )}
                  <p className="mt-3 text-[13px] leading-5 text-muted-foreground">A read-only list. Change values on the{" "}
                    <button type="button" className="rounded-sm font-semibold text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" onClick={() => go({ tab: "configuration", p: "env" })} data-testid="code-env-link">Environment variables</button> page.</p>
                </div>
              </div>
            </div>
            <div className="mt-6 flex flex-wrap items-center justify-between gap-3 border-t pt-5">
              <p className="text-[13px] text-muted-foreground" data-testid="code-state">{changed.length > 0 ? `${changed.length} ${changed.length === 1 ? "file" : "files"} changed, not deployed.` : "No change since the last deploy."}</p>
              <Button type="button" disabled={changed.length === 0 || deploy.isPending} onClick={send} data-testid="code-deploy">{deploy.isPending ? "Deploying…" : "Deploy"}</Button>
            </div>
            <div className="mt-3"><Refused error={deploy.error} operation="UpdateFunctionCode" testId="deploy-error" /></div>
          </>
        )}
      </Card>
      <Card title="Runtime settings" testId="runtime-settings">
        <dl className="grid grid-cols-[minmax(7rem,12rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0">
          <Row k="Runtime" v={runtimeName(f.runtime) || "–"} />
          <Row k="Handler" v={f.handler || "–"} mono />
          <Row k="Architecture" v={f.arch} mono />
        </dl>
        <p className="mt-4 max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-foreground">
          {interpreter ? <>A .zip function&apos;s code runs as a local process: the runtime names the interpreter, and the <span className="font-mono">{interpreter}</span> on this machine runs it.</> : <>A .zip function&apos;s code runs as a local process: this runtime brings its own, a <span className="font-mono">bootstrap</span> file that doze-aws starts.</>}
        </p>
      </Card>
      <Card title="Layers" lede="A function can use up to five layers; their contents are merged in the order listed." actions={<Button type="button" variant="outline" className="bg-card" onClick={() => go({ tab: "code", view: "layer-add" })} data-testid="layer-add"><PlusIcon /> Add a layer</Button>} testId="layers-card">
        {f.layers.length === 0 ? (
          <EmptyTab icon={LayersIcon} title="No layers" testId="layers-none">
            {d.name} does not use a layer.{newest ? <> <span className="font-mono text-foreground">{newest.name}</span> version {newest.latest} is the newest in this account.</> : layers.pending ? null : " This account has no layer yet."}
          </EmptyTab>
        ) : (
          <ol className="grid gap-2" data-testid="layers-list">
            {f.layers.map((arn, i) => (
              <li key={arn} className="flex flex-wrap items-baseline gap-x-4 rounded-lg border bg-well px-4 py-2.5 text-[13.5px]">
                <span className="text-muted-foreground">{i + 1}</span>
                <span className="font-mono text-bright">{tailOf(arn.split(":").slice(0, -1).join(":"))}</span>
                <span className="text-muted-foreground">version {arn.split(":").pop()}</span>
                <span className="min-w-0 truncate font-mono text-[12px] text-muted-foreground">{arn}</span>
              </li>
            ))}
          </ol>
        )}
      </Card>
      <RecentCalls calls={d.calls} noun="function" />
    </div>
  );
}
