// What the DynamoDB pages share: where a table is, its DescribeTable, the moves between its pages (`?do=`, `?tab=`), the crumbs,
// and the dialog that asks before a delete.
import { useNavigate } from "@tanstack/react-router";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import type { OwnSearch } from "../search";
import { type Target, useOpRead } from "../deep/api";
import { type Described, tableOf } from "./model";

export const tableTarget = (name: string): Target => ({ service: "dynamodb", kind: "tables", name });
export const KIND = { service: "dynamodb", kind: "tables" };

/** DescribeTable of a table, as the pages read it. A write through `useOp` makes it read again. */
export function useTable(name: string, enabled = true) {
  const q = useOpRead<Described>(tableTarget(name), "DescribeTable", {}, enabled);
  return { ...q, table: tableOf(q.data) };
}

/** The part of an address the pages of one table move between. `tab` and `dialog` are for the table's page; the rest say what a `?do=` acts on. */
export type Move = OwnSearch & { tab?: string };
const KEYS = ["do", "name", "key", "ver", "sel", "dialog", "mode", "tab"] as const;
const clean = (s: Move) => Object.fromEntries(KEYS.flatMap((k) => (s[k] ? [[k, s[k]]] : [])));

/** Moves between the pages of one table; what is not named is not kept. */
export function useTableNav(name: string) {
  const navigate = useNavigate();
  return (search: Move = {}, replace = false) => void navigate({ to: "/resources/$service/$kind/$name", params: { ...KIND, name }, search: clean(search), replace });
}

/** Moves between the pages of the list of tables. */
export function useListNav() {
  const navigate = useNavigate();
  return (search: Move = {}) => void navigate({ to: "/resources/$service/$second", params: { service: "dynamodb", second: "tables" }, search: clean(search) });
}

/** Resources, the group, DynamoDB, Tables, and the table. */
export function crumbs(name?: string, last?: string): { label: string; to: string }[] {
  const base = [
    { label: "Resources", to: "/resources" },
    { label: "Storage and data", to: "/resources" },
    { label: "DynamoDB", to: "/resources/dynamodb/tables" },
    { label: "Tables", to: "/resources/dynamodb/tables" },
  ];
  const table = name ? [...base, { label: name, to: `/resources/dynamodb/tables/${encodeURIComponent(name)}` }] : base;
  return last ? [...table, { label: last, to: "" }] : table;
}

/** A dialog over the page that asks once before it does what cannot be taken back; the service's refusal is shown in it. */
export function Ask({ open, onOpenChange, title, children, verb, onConfirm, pending, error, call, testId = "ask" }: { open: boolean; onOpenChange: (open: boolean) => void; title: string; children: React.ReactNode; verb: string; onConfirm: () => void; pending: boolean; error: unknown; /** The operation, for the refusal's title. */ call: string; testId?: string }) {
  const failed = error ? errorWords(error) : null;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[600px]" data-testid={testId}>
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">{title}</DialogTitle>
          <DialogDescription className="sr-only">{title}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-4 text-[14px] leading-[22px] text-foreground">{children}</div>
        {failed && (
          <Alert tone="bad" title={`${call} was refused${failed.code ? `: ${failed.code}` : ""}`} testId={`${testId}-error`}>
            {failed.message}
          </Alert>
        )}
        <DialogFooter>
          <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)} data-testid={`${testId}-cancel`}>
            Cancel
          </Button>
          <Button type="button" variant="destructive" disabled={pending} onClick={onConfirm} data-testid={`${testId}-submit`}>
            {pending ? "Working…" : verb}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** A passage that says what happens and what does not, set apart by a rule at its left. */
export const Said = ({ children, testId }: { children: React.ReactNode; testId?: string }) => (
  <p className="border-l-2 border-input pl-3.5 text-[13px] leading-5 text-foreground [&_b]:font-semibold [&_b]:text-bright" data-testid={testId}>
    {children}
  </p>
);
