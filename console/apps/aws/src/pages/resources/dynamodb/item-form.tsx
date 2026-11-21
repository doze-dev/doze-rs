// Create item and Edit item (dynamodb__item-create, item-edit): the item as rows of attribute name, type and value, the key attributes
// fixed by the table, and the PutItem it will make shown as it is filled in. PutItem replaces the whole item with this key; an edit
// leaves the keys as they are (to move an item to a new key, make a new one and delete this one).
import { PlusIcon, XIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { Alert, ErrorBlock, errorWords } from "@doze/ui/components/app/alert";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { ApiFailure } from "@doze/ui/lib/api";
import { FormPage, FormSection, RuleHelp } from "../../../kit";
import { useOp, useOpRead } from "../deep/api";
import type { OwnProps } from "../deep/registry";
import { ATTRIBUTE_TYPES, type AttributeType, type AttrRow, blankRows, itemOf, newAttr, paramKey, rowsOf, TYPE_NAMES } from "./item-rows";
import type { Item } from "./json";
import { crumbs, tableTarget, useTable, useTableNav } from "./parts";

const TYPED = new Set<AttributeType>(["L", "M", "SS", "NS", "BS"]);
const HINTS: Partial<Record<AttributeType, string>> = { L: '[{"S": "a"}, {"N": "1"}]', M: '{"city": {"S": "Lyon"}}', SS: '["a", "b"]', NS: '["1", "2.5"]', BS: '["AAEC"]' };

function Row({ r, set, remove, locked }: { r: AttrRow; set: (patch: Partial<AttrRow>) => void; remove: () => void; locked: boolean }) {
  const fixed = r.key !== null;
  return (
    <div className="grid items-start gap-3 md:grid-cols-[minmax(0,1fr)_9.5rem_minmax(0,1.1fr)_7rem]" data-testid={`attr-${r.name || "new"}`}>
      <Input value={r.name} onChange={(e) => set({ name: e.target.value })} disabled={fixed} placeholder="attribute" aria-label="Attribute name" autoComplete="off" spellCheck={false} className={fixed ? "bg-well font-mono" : "bg-card font-mono"} data-testid="attr-name" />
      <Select value={r.type} onValueChange={(v) => set({ type: v as AttributeType, value: v === "BOOL" ? "true" : "" })} disabled={fixed}>
        <SelectTrigger aria-label="Attribute type" className={fixed ? "w-full bg-well" : "w-full bg-card"} data-testid="attr-type">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {ATTRIBUTE_TYPES.map((t) => (
            <SelectItem key={t} value={t}>
              {TYPE_NAMES[t]}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {r.type === "BOOL" ? (
        <Select value={r.value || "true"} onValueChange={(v) => set({ value: v })}>
          <SelectTrigger aria-label="Attribute value" className="w-full bg-card" data-testid="attr-value">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="true">true</SelectItem>
            <SelectItem value="false">false</SelectItem>
          </SelectContent>
        </Select>
      ) : r.type === "NULL" ? (
        <Input value="null" disabled aria-label="Attribute value" className="bg-well font-mono" />
      ) : (
        <Input value={r.value} onChange={(e) => set({ value: e.target.value })} disabled={fixed && locked} placeholder={HINTS[r.type] ?? "value"} aria-label="Attribute value" autoComplete="off" spellCheck={false} className={fixed && locked ? "bg-well font-mono" : "bg-card font-mono"} data-testid="attr-value" />
      )}
      <div className="flex h-9 items-center">
        {fixed ? (
          <Badge variant="secondary" className="font-normal text-muted-foreground">{r.key === "HASH" ? "Partition key" : "Sort key"}</Badge>
        ) : (
          <Button type="button" variant="ghost" size="icon-sm" onClick={remove} aria-label={`Remove ${r.name || "attribute"}`} data-testid="attr-remove">
            <XIcon />
          </Button>
        )}
      </div>
    </div>
  );
}

export function ItemForm({ name, search, edit }: OwnProps & { edit: boolean }) {
  const table = name!;
  const t = tableTarget(table);
  const read = useTable(table);
  const go = useTableNav(table);
  const keys = useMemo(() => (read.table ? [{ name: read.table.hash.name, role: "HASH" as const, type: read.table.hash.type }, ...(read.table.range ? [{ name: read.table.range.name, role: "RANGE" as const, type: read.table.range.type }] : [])] : []), [read.table]);
  const key = useMemo(() => paramKey(search.key), [search.key]);
  const got = useOpRead<{ Item?: Item }>(t, "GetItem", { Key: key ?? {} }, edit && key !== null);
  const [rows, setRows] = useState<AttrRow[] | null>(null);
  const put = useOp<unknown>(t, "PutItem");
  const back = () => go({ do: "explore" });
  if (read.isError && !read.table) return <ErrorBlock what="This table could not be read" error={read.error} onRetry={() => void read.refetch()} testId="detail-error" />;
  if (!read.table || (edit && got.isPending)) return <Skeleton className="h-80 rounded-[14px]" />;
  if (edit && !got.data?.Item) return <ErrorBlock what="This item could not be read" error={got.error ?? new Error("No item has this key any more. It may have been deleted.")} onRetry={() => void got.refetch()} testId="item-missing" />;
  const current = rows ?? (edit ? rowsOf(got.data!.Item!, keys) : blankRows(keys));
  const change = (next: AttrRow[]) => (setRows(next), put.reset());
  const made = itemOf(current);
  const item = "item" in made ? made.item : null;
  const full = item !== null && keys.every((k) => item[k.name]);
  const input = item && full ? { TableName: table, Item: item } : null;
  const said = put.error ? errorWords(put.error) : null;
  const words = keys.map((k) => k.name);
  const shownKey = edit && key && keys[0] && key[keys[0].name] ? String(Object.values(key[keys[0].name]!)[0]) : "";
  return (
    <FormPage
      title={edit ? "Edit item" : "Create item"}
      sub={edit ? <><span className="font-mono text-foreground">{shownKey}</span> in <span className="font-mono text-foreground">{table}</span>. Change the values you need, then save.</> : <>Add an item to <span className="font-mono text-foreground">{table}</span>. Only the key attributes are required; add any others you need.</>}
      crumbs={[...crumbs(table), { label: "Explore table items", to: `/resources/dynamodb/tables/${encodeURIComponent(table)}?do=explore` }, { label: edit ? "Edit item" : "Create item", to: "" }]}
      request={input ? { service: "dynamodb", operation: "PutItem", input } : null}
      refusal={put.error && said ? { code: put.error instanceof ApiFailure ? put.error.code : "Error", message: said.message } : null}
      banner={said ? <Alert tone="bad" title={edit ? "The item was not saved." : "The item was not created."} testId="item-refused"><span className="font-mono text-[13px]">{said.code ? `${said.code}: ` : ""}{said.message}</span></Alert> : undefined}
      submitLabel={edit ? "Save and close" : "Create item"}
      pending={put.isPending}
      disabled={input === null}
      disabledWhy={"error" in made ? made.error : `Give every key attribute a value (${words.join(", ")}).`}
      onCancel={back}
      onSubmit={() => input && put.mutate({ Item: input.Item }, { onSuccess: () => go({ do: "explore", dialog: edit ? "item-saved" : "item-created" }) })}
    >
      <FormSection title="Attributes" description="The key attributes are fixed by the table; everything else is yours.">
        <div className="hidden gap-3 text-[13px] text-muted-foreground md:grid md:grid-cols-[minmax(0,1fr)_9.5rem_minmax(0,1.1fr)_7rem]">
          <span>Attribute name</span>
          <span>Type</span>
          <span>Value</span>
          <span />
        </div>
        <div className="grid gap-3" data-testid="attributes">
          {current.map((r) => (
            <Row key={r.id} r={r} locked={edit} set={(patch) => change(current.map((x) => (x.id === r.id ? { ...x, ...patch } : x)))} remove={() => change(current.filter((x) => x.id !== r.id))} />
          ))}
        </div>
        <div>
          <Button type="button" variant="outline" className="bg-card" onClick={() => change([...current, newAttr()])} data-testid="attr-add">
            <PlusIcon /> Add new attribute
          </Button>
        </div>
        <RuleHelp>
          {edit ? (
            <>Key attributes cannot be changed on an existing item. To move an item to a new key, create a new item and delete this one. Saving writes the whole item back.</>
          ) : (
            <>
              Attribute types: <b>String, Number, Binary, Boolean, Null, List, Map, String set, Number set, Binary set</b>. An item can be up to 400 KB. If an item with this {words.join(" and ")} exists, it is replaced completely, not merged.
            </>
          )}
          {[...TYPED].some((x) => current.some((r) => r.type === x)) && <> A set is a JSON list of texts; a list or map is DynamoDB JSON.</>}
        </RuleHelp>
      </FormSection>
    </FormPage>
  );
}
