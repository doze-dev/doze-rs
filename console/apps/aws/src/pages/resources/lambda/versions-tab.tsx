// A function's Versions and Aliases tabs (lambda__function-versions, version-publish, function-aliases, aliases-none): the versions
// Lambda lists, the dialog that publishes the next one, and the aliases that point at versions.
import { PlusIcon, TagIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { dateTimeUtc } from "@doze/ui/lib/format";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { on, useOp, useOpRead } from "../deep/api";
import { Card, Loaded, Refused } from "../deep/parts";
import { timeMs } from "../model";
import { unchangedKey } from "./done-words";
import { nextVersion } from "./forms";
import type { Go } from "./hooks";
import type { Fn } from "./model";
import { useRemove } from "./remove";

export type Version = { Version: string; Description?: string; LastModified?: string; FunctionArn?: string; CodeSha256?: string; MemorySize?: number; Timeout?: number; Runtime?: string; Handler?: string; Role?: string; Environment?: unknown; EphemeralStorage?: unknown; Layers?: unknown };
export type Alias = { Name: string; Description?: string; FunctionVersion?: string; AliasArn?: string };

const when = (v?: string) => {
  const ms = v ? timeMs(v) : null;
  return ms === null ? (v ?? "–") : dateTimeUtc(ms);
};
const mono = (s: string | undefined) => <span className="font-mono text-[13px]">{s || "–"}</span>;

/** What a version holds that publishing copies: its code and settings (not its description, which each version has of its own). */
/** $LATEST first, then the published versions, the newest first. */
export const newestFirst = (versions: readonly Version[]): Version[] => [...versions].sort((a, b) => (a.Version === "$LATEST" ? -1 : b.Version === "$LATEST" ? 1 : Number(b.Version) - Number(a.Version)));

export const sameSettings = (a: Version, b: Version): boolean => a.CodeSha256 === b.CodeSha256 && JSON.stringify([a.MemorySize, a.Timeout, a.Runtime, a.Handler, a.Role, a.Environment, a.EphemeralStorage, a.Layers]) === JSON.stringify([b.MemorySize, b.Timeout, b.Runtime, b.Handler, b.Role, b.Environment, b.EphemeralStorage, b.Layers]);

const VERSION_COLUMNS: DataColumn<Version>[] = [
  { id: "v", header: "Version", size: 120, accessorKey: "Version", meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright" data-testid="version-cell">{c.row.original.Version}</span> },
  { id: "d", header: "Description", size: 260, accessorFn: (r) => r.Description ?? "", cell: (c) => c.row.original.Description || (c.row.original.Version === "$LATEST" ? "The unpublished version" : <span className="text-muted-foreground">–</span>) },
  { id: "m", header: "Last modified", size: 210, accessorFn: (r) => r.LastModified ?? "", cell: (c) => when(c.row.original.LastModified) },
  { id: "a", header: "Function ARN", min: 300, accessorFn: (r) => r.FunctionArn ?? "", meta: { mono: true, wrap: true }, cell: (c) => mono(c.row.original.FunctionArn) },
];

const ALIAS_COLUMNS: DataColumn<Alias>[] = [
  { id: "n", header: "Name", size: 160, accessorKey: "Name", meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.Name}</span> },
  { id: "d", header: "Description", size: 260, accessorFn: (r) => r.Description ?? "", cell: (c) => c.row.original.Description || <span className="text-muted-foreground">–</span> },
  { id: "v", header: "Version", size: 100, accessorFn: (r) => r.FunctionVersion ?? "", meta: { mono: true }, cell: (c) => mono(c.row.original.FunctionVersion) },
  { id: "a", header: "Alias ARN", min: 300, accessorFn: (r) => r.AliasArn ?? "", meta: { mono: true, wrap: true }, cell: (c) => mono(c.row.original.AliasArn) },
];

export function VersionsTab({ d, f, go }: { d: ResourceDetail; f: Fn; go: (to: Go) => void }) {
  const versions = useOpRead<{ Versions?: Version[] }>(on.fn(d.name), "ListVersionsByFunction", { MaxItems: 50 });
  const [open, setOpen] = useState(false);
  const list = versions.data?.Versions ?? [];
  return (
    <>
      <Card title="Versions" lede={<span className="block max-w-[44rem]">A version is an immutable copy of the code and settings. Numbers only go up and are never reused, even if you delete and re-create the function.</span>} actions={<Button type="button" onClick={() => setOpen(true)} data-testid="version-publish"><PlusIcon /> Publish new version</Button>} testId="lambda-versions">
        <Loaded q={versions} what="The versions">
          {(v) => (
            <>
              <DataTable aria-label="Versions" data={newestFirst(v.Versions ?? [])} columns={VERSION_COLUMNS} rowId={(r) => r.Version} maxHeight={420} empty="No versions." />
              <p className="mt-3 text-[13px] text-muted-foreground">The columns are fields of ListVersionsByFunction: Version, Description, LastModified and FunctionArn.</p>
            </>
          )}
        </Loaded>
      </Card>
      <Publish d={d} f={f} versions={list} open={open} onOpenChange={setOpen} done={(version, made) => { setOpen(false); go({ tab: "versions", done: made || !version ? "published" : unchangedKey(version) }); }} />
    </>
  );
}

/** `done(version, made)`: the version PublishVersion answered with, and whether it is a new one (an answer that is one the list already had made none). */
function Publish({ d, versions, open, onOpenChange, done }: { d: ResourceDetail; f: Fn; versions: Version[]; open: boolean; onOpenChange: (o: boolean) => void; done: (version: string | undefined, made: boolean) => void }) {
  const [text, setText] = useState("");
  const publish = useOp<{ Version?: string }>(on.fn(d.name), "PublishVersion");
  const latest = versions.find((v) => v.Version === "$LATEST");
  const last = [...versions].filter((v) => v.Version !== "$LATEST").sort((a, b) => Number(b.Version) - Number(a.Version))[0];
  const next = nextVersion(versions);
  const changed = !last || !latest || !sameSettings(last, latest);
  // Nothing changed since the last version: the call answers with that one, and no `next` number is made.
  const unchanged = Boolean(last) && !changed;
  return (
    <Dialog open={open} onOpenChange={(o) => { onOpenChange(o); if (!o) publish.reset(); }}>
      <DialogContent className="sm:max-w-[600px]" data-testid="publish-dialog">
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">Publish new version</DialogTitle>
          <DialogDescription className="text-[14px] leading-[22px] text-foreground" data-testid="publish-words">
            {unchanged ? <>Publishing takes the current code and settings of <b className="font-mono font-semibold">$LATEST</b>.</> : <>Lambda creates version <b className="font-semibold">{next}</b> from the current code and settings of <b className="font-mono font-semibold">$LATEST</b>.</>}{" "}
            {!last ? "This is the first version of the function." : changed ? <>The code or settings changed after version {last.Version} (published {when(last.LastModified)}), so a new version can be published.</> : <>Nothing changed since version {last.Version}, so Lambda answers with that version and makes no new one.</>}{" "}
            Once published, the code and most settings cannot be changed.
          </DialogDescription>
        </DialogHeader>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); publish.mutate(text ? { Description: text } : {}, { onSuccess: (r) => done(r.Version, !versions.some((v) => v.Version === r.Version)) }); }}>
          <div className="grid gap-1.5">
            <Label htmlFor="version-description">Version description <span className="font-normal text-muted-foreground">Optional</span></Label>
            <Input id="version-description" value={text} onChange={(e) => setText(e.target.value)} autoComplete="off" maxLength={256} data-testid="version-description" />
          </div>
          <Refused error={publish.error} operation="PublishVersion" testId="publish-error" />
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>Cancel</Button>
            <Button type="submit" disabled={publish.isPending} data-testid="publish-submit">{publish.isPending ? "Publishing…" : "Publish"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function AliasesTab({ d, go }: { d: ResourceDetail; go: (to: Go) => void }) {
  const aliases = useOpRead<{ Aliases?: Alias[] }>(on.fn(d.name), "ListAliases", { MaxItems: 50 });
  const rm = useRemove(d.name, "DeleteAlias", "alias-delete-dialog");
  const make = () => go({ tab: "aliases", view: "alias-create" });
  const create = <Button type="button" onClick={make} data-testid="alias-create"><PlusIcon /> Create alias</Button>;
  return (
    <Loaded q={aliases} what="The aliases">
      {(a) =>
        (a.Aliases ?? []).length === 0 ? (
          <EmptyTab icon={TagIcon} title="No aliases" action={<Button type="button" variant="outline" className="bg-card" onClick={make}>Create alias</Button>} testId="aliases-none">{d.name} has no aliases. An alias is a pointer to a version that you can move without changing callers.</EmptyTab>
        ) : (
          <Card title="Aliases" lede={<span className="block max-w-[44rem]">An alias is a pointer to a function version that you can update. Callers use the alias ARN and never change.</span>} actions={create} testId="lambda-aliases">
            <DataTable aria-label="Aliases" data={a.Aliases ?? []} columns={[...ALIAS_COLUMNS, { id: "delete", header: "", size: 60, enableSorting: false, cell: (c) => <Button type="button" variant="ghost" size="icon-sm" aria-label={`Delete alias ${c.row.original.Name}`} onClick={() => rm.ask({ title: "Delete alias", input: { Name: c.row.original.Name }, text: <>Delete the alias <b className="font-mono font-semibold text-bright">{c.row.original.Name}</b> of {d.name}? Callers that use its ARN stop reaching version {c.row.original.FunctionVersion}; the version itself stays.</>, then: () => go({ tab: "aliases", done: "alias-deleted" }) })} data-testid="alias-delete"><Trash2Icon /></Button> }]} rowId={(r) => r.Name} maxHeight={420} />
            {rm.dialog}
            <p className="mt-3 text-[13px] text-muted-foreground">The columns are fields of ListAliases: Name, Description, FunctionVersion and AliasArn.</p>
          </Card>
        )
      }
    </Loaded>
  );
}
