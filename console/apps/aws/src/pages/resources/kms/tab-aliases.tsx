// The Aliases tab (kms__key-aliases, key-aliases-none, key-alias-create): the aliases that point at the key, Create alias (a dialog:
// the console adds the `alias/` prefix) and Delete for the ones chosen.
import { useQueryClient } from "@tanstack/react-query";
import { PlusIcon, TagIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { callKindOp, Field, PlainConfirm } from "../../../kit";
import { useAllRows } from "../cfg-api";
import { Note } from "../cfg-parts";
import { ALIASES } from "./api";
import { aliasProblem } from "./create-model";
import { refusalOf } from "./schedule";

type Row = { name: string; arn: string };
const COLUMNS: DataColumn<Row>[] = [
  { id: "name", header: "Alias name", accessorKey: "name", min: 180, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.name}</span> },
  { id: "arn", header: "Alias ARN", accessorKey: "arn", meta: { mono: true } },
];

export function TabAliases({ id, title, aliases }: { id: string; title: string; aliases: Row[] }) {
  const qc = useQueryClient();
  const all = useAllRows("kms", "aliases");
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const [open, setOpen] = useState<"create" | "delete" | null>(null);
  const [name, setName] = useState("");
  const [tried, setTried] = useState(false);
  const [pending, setPending] = useState(false);
  const [refused, setRefused] = useState<{ code: string; message: string } | null>(null);
  const [made, setMade] = useState<string | null>(null);
  const taken = all.rows.map((r) => r.name.replace(/^alias\//, ""));
  const problem = aliasProblem(name, taken);
  const reread = () => {
    void qc.invalidateQueries({ queryKey: ["resources", "one", "kms", "keys"] });
    void qc.invalidateQueries({ queryKey: ["resources", "list", "kms"] });
  };
  const close = () => { setOpen(null); setName(""); setTried(false); setRefused(null); };
  const create = async () => {
    setTried(true);
    if (problem) return;
    setPending(true);
    setRefused(null);
    try {
      await callKindOp(ALIASES, "CreateAlias", { AliasName: `alias/${name}`, TargetKeyId: id });
      setMade(name);
      setChosen(new Set());
      reread();
      close();
    } catch (e) {
      setRefused(refusalOf(e));
    } finally {
      setPending(false);
    }
  };
  const remove = async () => {
    setPending(true);
    setRefused(null);
    try {
      for (const n of chosen) await callKindOp(ALIASES, "DeleteAlias", { AliasName: `alias/${n}` });
      setChosen(new Set());
      setMade(null);
      reread();
      close();
    } catch (e) {
      setRefused(refusalOf(e));
    } finally {
      setPending(false);
    }
  };
  const createButton = <Button type="button" variant="outline" className="bg-card" onClick={() => setOpen("create")} data-testid="alias-create-open"><PlusIcon /> Create alias</Button>;
  const picked = [...chosen];
  return (
    <div className="grid gap-5" data-testid="aliases-tab">
      {made && <ResultBanner title={`Alias ${made} created.`} returned={`CreateAlias made alias/${made} point at this key.`} onDismiss={() => setMade(null)} testId="alias-made" />}
      {aliases.length === 0 ? (
        <EmptyTab icon={TagIcon} title="No aliases" action={createButton} testId="aliases-none">An alias is a friendly name for a key. This key has none, so it is addressed by its key ID or ARN.</EmptyTab>
      ) : (
        <>
          <SelectableTable
            aria-label="Aliases"
            noun="alias"
            data={aliases}
            columns={COLUMNS}
            rowId={(r) => r.name}
            selected={chosen}
            onSelectedChange={setChosen}
            maxHeight={420}
            actionsAlways
            actionsEnd
            toolbar={<span className="text-[13.5px] text-muted-foreground" data-testid="alias-count">{aliases.length} {aliases.length === 1 ? "alias" : "aliases"}</span>}
            actions={() => <><Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={picked.length === 0} onClick={() => setOpen("delete")} data-testid="alias-delete-open"><Trash2Icon /> Delete</Button>{createButton}</>}
          />
          <Note testId="aliases-note">An alias is its own resource, not a property of the key. Deleting {picked.length > 0 ? <b>{picked.join(", ")}</b> : "an alias"} leaves the key untouched, but anything that names the alias stops working. The console shows names without the alias/ prefix. It cannot repoint an alias to another key; that is UpdateAlias through the API only.</Note>
        </>
      )}
      <Dialog open={open === "create"} onOpenChange={(o) => { if (!o) close(); }}>
        <DialogContent className="sm:max-w-[560px]" data-testid="alias-create">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Create alias</DialogTitle>
            <DialogDescription className="text-[14px] leading-[22px] text-foreground">Add an alias to <b className="font-mono font-semibold">{title}</b>. The alias is created in this account and Region and points at this key.</DialogDescription>
          </DialogHeader>
          <form className="grid gap-5" noValidate onSubmit={(e) => { e.preventDefault(); void create(); }}>
            <Field label="Alias name" error={tried ? (problem ?? undefined) : undefined} refusal={refused ? { why: `${refused.code}: ${refused.message}` } : null} help={<>Do not type <b className="font-mono">alias/</b>: the console adds it, so <span className="font-mono">alias/larder-reports-reader</span> would become <span className="font-mono">alias/alias/larder-reports-reader</span>. <b>1 to 256</b> characters of letters, digits, / _ and -, unique in this Region, and not starting with <span className="font-mono">aws/</span>. Keep secrets out of the name: it may be displayed in plaintext in logs and other output.</>}>
              {(fid) => <Input id={fid} value={name} onChange={(e) => setName(e.target.value)} placeholder="larder-reports-reader" autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="alias-name" />}
            </Field>
            <DialogFooter>
              <Button type="button" variant="outline" className="bg-card" onClick={close}>Cancel</Button>
              <Button type="submit" disabled={pending} data-testid="alias-create-submit">{pending ? "Working…" : "Create alias"}</Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      <PlainConfirm open={open === "delete"} onOpenChange={(o) => { if (!o) close(); }} title={`Delete ${picked.length === 1 ? `alias ${picked[0]}` : `${picked.length} aliases`}?`} description="Only the alias goes: the key and what it encrypted are untouched. Calls that name the key by this alias stop working." verb="Delete" call="DeleteAlias" pending={pending} error={refused ? new Error(`${refused.code}: ${refused.message}`) : undefined} onConfirm={() => void remove()} testId="alias-delete" />
      {refused && open === null && <Alert tone="bad" title={`${refused.code}`}>{refused.message}</Alert>}
    </div>
  );
}
