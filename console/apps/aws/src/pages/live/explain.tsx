import { ShieldAlertIcon, WandSparklesIcon } from "lucide-react";
import { useState } from "react";
import { Button } from "@doze/ui/components/ui/button";
import { useAccess, usePrincipals } from "../../api/spaces";
import type { CallRecord } from "../../api/types/CallRecord";
import { who } from "../../people";
import { PassDialog } from "../access/pass-dialog";

const row = "grid grid-cols-[6rem_minmax(0,1fr)] gap-3 border-b py-2.5 text-[13px] last:border-b-0";

/** Why this would be denied: the caller, the action and resource IAM checked, what IAM said, and what Add to policy would add. It
 * reads Access's own account of the refusal (IAM's access log), so it is the same reason and the same draft as Make this pass. */
export function Explain({ call }: { call: CallRecord }) {
  const access = useAccess();
  const principals = usePrincipals(call.caller ?? "");
  const [open, setOpen] = useState(false);
  const g = access.data?.groups.find((x) => x.caller === call.caller && x.action === call.action);
  const p = principals.data?.principals.find((x) => x.key === call.caller);
  const w = p ? who(p) : { label: call.caller ?? "no key", title: "" };
  const rows = g?.explained ?? [];
  const first = rows[0];
  const enforce = access.data?.iam === "enforce";
  return (
    <section className="mt-5 rounded-xl border bg-card p-4" aria-label="Why this would be denied" data-testid="explain">
      <h3 className="flex items-center gap-2 text-[15px] font-semibold text-bright">
        <ShieldAlertIcon aria-hidden className="size-4 text-warn" /> {enforce ? "Why this was denied" : "Why this would be denied"}
      </h3>
      <dl className="mt-2">
        <div className={row}>
          <dt className="text-muted-foreground">Caller</dt>
          <dd data-testid="explain-caller">
            <span title={w.title}>{w.label}</span> <span className="text-muted-foreground">· access key per app</span>
          </dd>
        </div>
        <div className={row}>
          <dt className="text-muted-foreground">Action</dt>
          <dd className="font-mono text-[12.5px]" data-testid="explain-action">{first?.action ?? call.action}</dd>
        </div>
        {first && (
          <div className={row}>
            <dt className="text-muted-foreground">Resource</dt>
            <dd className="font-mono text-[12.5px] break-all" data-testid="explain-resource">{first.resource}</dd>
          </div>
        )}
        <div className={row}>
          <dt className="text-muted-foreground">Mode</dt>
          <dd>{enforce ? "IAM enforce: refused and recorded" : "IAM soft: allowed and recorded; real AWS would deny"}</dd>
        </div>
      </dl>
      {rows.length > 0 ? (
        <>
          <ul className="mt-3 grid gap-1.5 border-l-2 border-input pl-3 text-[13px]" data-testid="explain-why">
            {rows.map((r) => (
              <li key={`${r.action}-${r.resource}`}>
                IAM {enforce ? "refused" : "would refuse"} <span className="font-mono">{r.action}</span> {r.reason}.
              </li>
            ))}
          </ul>
          <div className="mt-4 rounded-lg bg-muted/60 p-3" data-testid="explain-adds">
            <div className="mb-1.5 text-[11.5px] font-medium tracking-wide text-muted-foreground uppercase">What Add to policy would add</div>
            {[...new Set(rows.map((r) => `${r.action} on ${r.resource}`))].map((l) => (
              <div key={l} className="font-mono text-[12.5px] break-all">+ {l}</div>
            ))}
          </div>
          <Button className="mt-4" size="sm" onClick={() => setOpen(true)} data-testid="explain-add">
            <WandSparklesIcon /> Add to policy for {w.label}
          </Button>
          {open && <PassDialog open={open} onOpenChange={setOpen} rows={rows} what={`${w.label} · ${call.action ?? ""}`} />}
        </>
      ) : (
        <p className="mt-3 text-[13px] text-muted-foreground" data-testid="explain-none">
          {access.isLoading ? "Reading IAM's account of it…" : "IAM's access log has no refusal of this caller and operation in the newest 2,000 rows it keeps, so there is nothing to draft from."}
        </p>
      )}
      <p className="mt-3 text-[12.5px] text-muted-foreground">Soft mode never blocks. It records what real AWS would have said.</p>
    </section>
  );
}
