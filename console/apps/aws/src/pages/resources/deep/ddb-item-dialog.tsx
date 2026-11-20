// An item in a drawer (its JSON, either way), and the dialog that puts one (create, or edit: PutItem replaces the whole item)
// with the JSON checked against the table's keys before it is sent.
import { PencilIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { JsonEditor } from "@doze/ui/components/app/json-editor";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@doze/ui/components/ui/sheet";
import { ToggleGroup, ToggleGroupItem } from "@doze/ui/components/ui/toggle-group";
import { type Target, useOp } from "./api";
import { type Item, type KeyPart, cell, itemPlainBlocker, itemText, keyOf, parseItem } from "./ddb-json";
import { Confirm, JsonBlock, Refused, useOpen } from "./parts";

type Mode = "dynamodb" | "plain";

function ModeToggle({ mode, set, blocked }: { mode: Mode; set: (m: Mode) => void; blocked: string | null }) {
  return (
    <div className="flex flex-wrap items-center gap-3">
      <ToggleGroup type="single" variant="outline" size="sm" value={mode} onValueChange={(v) => v && set(v as Mode)} aria-label="JSON form">
        <ToggleGroupItem value="dynamodb" data-testid="mode-dynamodb">
          DynamoDB JSON
        </ToggleGroupItem>
        <ToggleGroupItem value="plain" disabled={blocked !== null} data-testid="mode-plain">
          Plain JSON
        </ToggleGroupItem>
      </ToggleGroup>
      {blocked && <span className="text-[12.5px] text-muted-foreground">Plain JSON cannot hold this item: {blocked}.</span>}
    </div>
  );
}

/** Create (`item` null) or edit an item: PutItem with the JSON written in the editor. */
export function ItemDialog({ open, onOpenChange, t, keys, item, onSaved }: { open: boolean; onOpenChange: (o: boolean) => void; t: Target; keys: KeyPart[]; item: Item | null; onSaved?: () => void }) {
  const start = item ?? Object.fromEntries(keys.map((k) => [k.name, k.type === "N" ? { N: "0" } : k.type === "B" ? { B: "" } : { S: "" }]));
  const blocked = item ? itemPlainBlocker(item) : null;
  const first: Mode = blocked ? "dynamodb" : "plain";
  const [mode, setMode] = useState<Mode>(first);
  const [text, setText] = useState(() => itemText(start as Item, first));
  const put = useOp(t, "PutItem");
  const parsed = parseItem(text, mode, keys);
  const moved = item && "item" in parsed && JSON.stringify(keyOf(parsed.item, keys)) !== JSON.stringify(keyOf(item, keys));
  const change = (o: boolean) => {
    if (!o) {
      put.reset();
      setMode(first);
      setText(itemText(start as Item, first));
    }
    onOpenChange(o);
  };
  const switchMode = (m: Mode) => {
    if ("item" in parsed) setText(itemText(parsed.item, m));
    setMode(m);
  };
  const submit = () => {
    if (!("item" in parsed) || put.isPending) return;
    put.mutate({ Item: parsed.item }, { onSuccess: () => (toast.success(item ? "Item saved" : "Item created"), change(false), onSaved?.()) });
  };
  return (
    <Dialog open={open} onOpenChange={change}>
      <DialogContent className="sm:max-w-[720px]" data-testid="item-dialog">
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">{item ? "Edit item" : "Create item"}</DialogTitle>
          <DialogDescription>
            Written with <span className="font-mono text-foreground">PutItem</span>, which replaces the whole item with this key. Keys: {keys.filter((k) => k.role).map((k) => `${k.name} (${k.type}, ${k.role === "HASH" ? "partition" : "sort"})`).join(", ")}.
          </DialogDescription>
        </DialogHeader>
        <ModeToggle mode={mode} set={switchMode} blocked={blocked} />
        <JsonEditor
          label="Item"
          value={text}
          onChange={setText}
          empty="invalid"
          rows={14}
          testId="item-json"
          help={
            "error" in parsed ? (
              <span className="text-bad" data-testid="item-wrong">
                {parsed.error}
              </span>
            ) : moved ? (
              <span className="text-warn">The key changed: this writes a new item, and the one you opened stays.</span>
            ) : (
              <>Valid: {Object.keys(parsed.item).length} attributes.</>
            )
          }
        />
        <Refused operation="PutItem" error={put.error} testId="item-error" />
        <DialogFooter>
          <Button type="button" variant="outline" className="bg-card" onClick={() => change(false)}>
            Cancel
          </Button>
          <Button type="button" disabled={"error" in parsed || put.isPending} onClick={submit} data-testid="item-save">
            {put.isPending ? "Saving…" : item ? "Save item" : "Create item"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** An item in a drawer: its JSON either way, Edit and Delete. */
export function ItemSheet({ item, onClose, t, keys }: { item: Item | null; onClose: () => void; t: Target; keys: KeyPart[] }) {
  const [mode, setMode] = useState<Mode>("plain");
  const [editing, setEditing] = useState(false);
  const del = useOp(t, "DeleteItem");
  const [deleting, setDeleting] = useOpen(() => del.reset());
  const blocked = item ? itemPlainBlocker(item) : null;
  const shown: Mode = blocked ? "dynamodb" : mode;
  const title = item ? keys.map((k) => cell(item[k.name])).join(" · ") : "";
  return (
    <Sheet open={item !== null} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-[560px]" data-testid="item-sheet">
        {item && (
          <>
            <SheetHeader className="border-b px-7 pt-7 pb-5">
              <SheetTitle className="font-mono text-[18px] font-semibold break-all">{title}</SheetTitle>
              <SheetDescription>{Object.keys(item).length} attributes</SheetDescription>
            </SheetHeader>
            <div className="grid gap-5 px-7 py-6">
              <div className="flex flex-wrap items-center gap-2">
                <ModeToggle mode={shown} set={setMode} blocked={blocked} />
                <span className="flex-1" />
                <CopyButton text={itemText(item, shown)} what="Item copied" className="bg-card" />
              </div>
              <JsonBlock value={itemText(item, shown)} className="max-h-[56vh]" testId="item-json-view" />
              <div className="flex justify-end gap-2 border-t pt-5">
                <Button variant="outline" className="bg-card" onClick={() => setEditing(true)} data-testid="item-edit">
                  <PencilIcon /> Edit
                </Button>
                <Button variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => setDeleting(true)} data-testid="item-delete">
                  <Trash2Icon /> Delete
                </Button>
              </div>
            </div>
            <ItemDialog key={JSON.stringify(item)} open={editing} onOpenChange={setEditing} onSaved={onClose} t={t} keys={keys} item={item} />
            <Confirm
              open={deleting}
              onOpenChange={setDeleting}
              title="Delete item"
              verb="Delete"
              operation="DeleteItem"
              pending={del.isPending}
              error={del.error}
              run={() => del.mutate({ Key: keyOf(item, keys) }, { onSuccess: () => (setDeleting(false), toast.success("Item deleted"), onClose()) })}
              testId="item-delete-dialog"
            >
              Delete the item <b className="font-mono font-semibold break-all text-bright">{title}</b>? It cannot be undone.
            </Confirm>
          </>
        )}
      </SheetContent>
    </Sheet>
  );
}
