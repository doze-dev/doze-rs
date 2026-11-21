// Delete table (dynamodb__table-delete): a dialog over the page that asks it, from the list with the table ticked or from the
// table's own page. It says how many items go with the table, that its indexes go too, and what deletion protection does.
import { Callout } from "@doze/ui/components/app/callout";
import { number } from "@doze/ui/lib/format";
import { useDelete } from "../../../api/resources";
import { Ask, Said, useTable } from "./parts";

export function TableDelete({ name, open, onOpenChange, onDone }: { name: string; open: boolean; onOpenChange: (open: boolean) => void; /** Called with the items the table held, once it is gone. */ onDone: (items: number | null) => void }) {
  const { table } = useTable(name, open);
  const del = useDelete("dynamodb", "tables");
  const items = table?.items ?? null;
  const indexes = (table?.global.length ?? 0) + (table?.local.length ?? 0);
  return (
    <Ask
      open={open}
      onOpenChange={(o) => {
        if (!o) del.reset();
        onOpenChange(o);
      }}
      title="Delete table?"
      verb="Delete table"
      call="DeleteTable"
      pending={del.isPending}
      error={del.error}
      testId="delete-dialog"
      onConfirm={() => del.mutate(name, { onSuccess: () => onDone(items) })}
    >
      <p data-testid="delete-words">
        You are about to delete <span className="font-mono">{name}</span>
        {items !== null && (
          <>
            {" "}and its <b className="font-semibold text-bright">{number(items)} {items === 1 ? "item" : "items"}</b>
          </>
        )}
        . Deleting a table is an unrecoverable operation. Any indexes on it go with it ({indexes === 0 ? `${name} has none` : `${name} has ${indexes}`}).
      </p>
      <Callout tone="warn" lead="No way back.">
        doze-aws keeps no backups. If you may want the data again, copy the data directory first.
      </Callout>
      {table && (
        <Said testId="delete-protection">
          Deletion protection is <b>{table.protect ? "on" : "off"}</b> for this table. While it is on, nobody can delete the table, so the button below {table.protect ? "will be" : "would be"} refused.
        </Said>
      )}
    </Ask>
  );
}
