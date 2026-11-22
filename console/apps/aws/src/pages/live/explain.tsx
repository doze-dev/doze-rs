import { AlertTriangleIcon, ShieldAlertIcon, WandSparklesIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { Button } from "@doze/ui/components/ui/button";
import { useAccess, useHeldPolicies, usePrincipals } from "../../api/spaces";
import type { CallRecord } from "../../api/types/CallRecord";
import type { HeldPolicy } from "../../api/types/HeldPolicy";
import { who } from "../../people";
import { addTo, covering, statementsOf } from "../access/merge";
import { PassDialog } from "../access/pass-dialog";

const row = "grid grid-cols-[6rem_minmax(0,1fr)] gap-3 border-b py-2.5 text-[13px] last:border-b-0";

/** The policy Add to policy edits: the caller's own inline policy that already allows something in the action's service, else its
 * first inline policy; none when it has no inline policy (a new one is made then). */
export function editTarget(policies: readonly HeldPolicy[], action: string): HeldPolicy | null {
  const inline = policies.filter((p) => p.inline && p.document);
  const service = `${action.split(":")[0]?.toLowerCase()}:`;
  return inline.find((p) => statementsOf(p.document).some((s) => [s.Action ?? []].flat().some((a) => a.toLowerCase().startsWith(service)))) ?? inline[0] ?? null;
}

/** What a policy allows, one line each: `s3:GetObject on arn:aws:s3:::reports/*`. */
function allowsLines(doc: string | null): string[] {
  return statementsOf(doc)
    .filter((s) => s.Effect === "Allow")
    .map((s) => `${[s.Action ?? []].flat().join(", ")} on ${[s.Resource ?? []].flat().join(", ")}`);
}

/** Why this would be denied: how many statements match, the caller, the action and resource IAM checked, what IAM said, the policies
 * the caller holds (and which of them match), and what Add to policy would add to the one that is there. It reads Access's own
 * account of the refusal (IAM's access log), so it is the same reason and the same draft as Make this pass. */
export function Explain({ call }: { call: CallRecord }) {
  const access = useAccess();
  const principals = usePrincipals(call.caller ?? "");
  const [open, setOpen] = useState(false);
  const g = access.data?.groups.find((x) => x.caller === call.caller && x.action === call.action);
  const p = principals.data?.principals.find((x) => x.key === call.caller);
  const held = useHeldPolicies(p?.kind === "user" ? "user" : null, p?.name ?? null);
  const w = p ? who(p) : { label: call.caller ?? "no key", title: "" };
  const rows = useMemo(() => g?.explained ?? [], [g]);
  const first = rows[0];
  const enforce = access.data?.iam === "enforce";
  const policies = held.data?.policies ?? [];
  const target = first ? editTarget(policies, first.action) : null;
  const matching = first ? policies.reduce((n, pol) => n + covering(pol.document, first.action, first.resource), 0) : 0;
  const adds = useMemo(() => (target?.document && rows.length > 0 ? addTo(target.document, rows).added : [...new Set(rows.map((r) => `${r.action} on ${r.resource}`))]), [target, rows]);
  return (
    <section className="mt-5 rounded-xl border bg-card p-4" aria-label="Why this would be denied" data-testid="explain">
      <h3 className="flex items-center gap-2 text-[15px] font-semibold text-bright">
        <ShieldAlertIcon aria-hidden className="size-4 text-warn" /> {enforce ? "Why this was denied" : "Why this would be denied"}
      </h3>
      {first && held.data && (
        <p className="mt-3 flex items-baseline gap-3" data-testid="explain-count">
          <span className="font-light text-[44px] leading-none text-bright tabular-nums">{matching}</span>{" "}
          <span className="text-[14px] text-muted-foreground">{matching === 1 ? "statement matches" : "statements match"} this resource</span>
        </p>
      )}
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
          {p?.kind === "user" && p.name && (
            <div className="mt-4" data-testid="explain-policies">
              <div className="mb-1.5 text-[11.5px] font-medium tracking-wide text-muted-foreground uppercase">Policies evaluated for {w.label}</div>
              {held.isLoading ? (
                <p className="text-[13px] text-muted-foreground">Reading what {w.label} holds…</p>
              ) : policies.length === 0 ? (
                <p className="text-[13px] text-muted-foreground" data-testid="explain-no-policies">{w.label} holds no policy of its own, so nothing allows it.</p>
              ) : (
                <ul className="grid gap-2">
                  {policies.map((pol) => {
                    const lines = allowsLines(pol.document);
                    const n = first ? covering(pol.document, first.action, first.resource) : 0;
                    return (
                      <li key={`${pol.inline}-${pol.name}`} className="rounded-lg border bg-background p-3" data-testid="explain-policy" data-name={pol.name}>
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="font-mono text-[12.5px] font-semibold">{pol.name}</span>
                          <span className="rounded-full border px-1.5 text-[10.5px] text-muted-foreground">{pol.inline ? "identity policy" : "managed policy"}</span>
                        </div>
                        {lines.length > 0 && <div className="mt-1.5 grid gap-0.5 text-[12px] text-muted-foreground">{lines.slice(0, 4).map((l) => <span key={l} className="font-mono break-all">allows {l}</span>)}</div>}
                        {n === 0 && (
                          <p className="mt-1.5 flex items-center gap-1.5 text-[12.5px]" data-testid="explain-no-match">
                            <AlertTriangleIcon aria-hidden className="size-3.5 text-warn" /> no statement matches this resource
                          </p>
                        )}
                      </li>
                    );
                  })}
                </ul>
              )}
              {(held.data?.groups.length ?? 0) > 0 && <p className="mt-2 text-[12px] text-muted-foreground">The policies of {held.data?.groups.join(", ")}, {held.data?.groups.length === 1 ? "a group" : "groups"} {w.label} is in, are not read.</p>}
            </div>
          )}
          <div className="mt-4 rounded-lg bg-muted/60 p-3" data-testid="explain-adds">
            <div className="mb-1.5 text-[11.5px] font-medium tracking-wide text-muted-foreground uppercase">What Add to policy would add</div>
            {adds.length === 0 ? <p className="text-[12.5px]">Nothing: what IAM refused is covered, or refused by an explicit deny.</p> : adds.map((l) => <div key={l} className="font-mono text-[12.5px] break-all">+ {l}</div>)}
            {target ? (
              <div className="mt-1.5 text-[12px] text-muted-foreground" data-testid="explain-target">
                to <span className="font-mono text-foreground">{target.name}</span> <span className="rounded-full bg-primary/15 px-1.5 text-[10.5px] text-primary">new</span> · the policy {w.label} already has
              </div>
            ) : (
              <div className="mt-1.5 text-[12px] text-muted-foreground">{w.label} has no inline policy to add to, so a new one is made.</div>
            )}
          </div>
          <Button className="mt-4" size="sm" onClick={() => setOpen(true)} disabled={held.isLoading && p?.kind === "user"} data-testid="explain-add">
            <WandSparklesIcon /> Add to policy for {w.label}
          </Button>
          {open && <PassDialog open={open} onOpenChange={setOpen} rows={rows} what={`${w.label} · ${call.action ?? ""}`} edit={target?.document ? { name: target.name, document: target.document } : undefined} />}
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
