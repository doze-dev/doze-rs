// The statements of a function's resource-based policy (lambda__function-permissions): a table whose cells wrap, as the board's
// does, because a condition is a whole ARN and cutting it short hides the one thing that says who may call. A function has a
// handful of statements, so none of the DataTable's fixed single-line rows (made for long lists) is needed.
import { Trash2Icon } from "lucide-react";
import { Button } from "@doze/ui/components/ui/button";
import type { Statement } from "./model";
import { conditionOf, principalOf } from "./model";

const HEAD = "h-10 border-b px-3 text-left text-[12px] font-medium tracking-[0.04em] whitespace-nowrap text-muted-foreground uppercase";
const CELL = "border-b px-3 py-3.5 align-top font-mono text-[13px] break-words group-last:border-b-0";

export function PolicyTable({ statements, onDelete }: { statements: readonly Statement[]; /** Draws a delete button on each row, called with the statement's SID. */ onDelete?: (sid: string) => void }) {
  return (
    <div className="overflow-x-auto rounded-xl border bg-card" data-slot="policy-table">
      <table className="w-full border-separate border-spacing-0 text-[13.5px]" aria-label="Resource-based policy statements">
        <thead className="bg-well">
          <tr>
            {["SID", "Effect", "Principal", "Action", "Condition"].map((h) => (
              <th key={h} scope="col" className={HEAD}>{h}</th>
            ))}
            {onDelete && <th scope="col" className={HEAD}><span className="sr-only">Delete</span></th>}
          </tr>
        </thead>
        <tbody>
          {statements.map((s, i) => (
            <tr key={s.Sid ?? i} data-row={s.Sid ?? String(i)} className="group hover:bg-accent/40">
              <td className={`${CELL} font-medium text-bright`}>{s.Sid ?? "–"}</td>
              <td className={`${CELL} font-sans text-[13.5px]`}>{s.Effect ?? "–"}</td>
              <td className={CELL}>{principalOf(s)}</td>
              <td className={CELL}>{[s.Action].flat().join(", ")}</td>
              <td className={`${CELL} min-w-[16rem]`}>{conditionOf(s) || "–"}</td>
              {onDelete && <td className={CELL}>{s.Sid && <Button type="button" variant="ghost" size="icon-sm" aria-label={`Delete statement ${s.Sid}`} onClick={() => onDelete(s.Sid!)} data-testid="permission-delete"><Trash2Icon /></Button>}</td>}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
