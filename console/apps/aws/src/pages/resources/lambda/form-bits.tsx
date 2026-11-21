// Pieces the function's forms share: the page's frame (crumbs, back, the call it makes), a number with its unit inside the field,
// and the words under a field that say what Lambda does with the setting here.
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { FormPage } from "../../../kit";
import { crumbsOf, refusalOf } from "./hooks";

export type FormProps = { d: ResourceDetail; f: import("./model").Fn; go: (to: import("./hooks").Go) => void; where: { region: string; account: string }; triggers: readonly import("./model").Trigger[]; sel?: string; /** Where the page was opened from. */ tab?: string; p?: string };

/** A number whose unit is written inside the field, at its right (`512 MB`). */
export function UnitInput({ id, value, onChange, unit, testId, width = "max-w-[16rem]" }: { id: string; value: string; onChange: (v: string) => void; unit: string; testId: string; width?: string }) {
  return (
    <div className={`relative w-full ${width}`}>
      <Input id={id} value={value} onChange={(e) => onChange(e.target.value)} inputMode="numeric" autoComplete="off" className="pr-20 font-mono" data-testid={testId} />
      <span aria-hidden className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 text-[13px] text-muted-foreground">{unit}</span>
    </div>
  );
}

/** The frame of a function's form page: where it was opened from is where Cancel returns. */
export function FnFormPage({ p, title, label, sub, children, request, refusal, callNote, banner, submitLabel, pending, disabled, disabledWhy, onSubmit, testId, aside, callService }: { p: FormProps; title: string; label: string; sub: React.ReactNode; children: React.ReactNode; request: React.ComponentProps<typeof FormPage>["request"]; refusal?: unknown; callNote?: React.ReactNode; banner?: React.ReactNode; submitLabel: string; pending: boolean; disabled: boolean; disabledWhy?: React.ReactNode; onSubmit: () => void; testId: string; aside?: React.ReactNode; callService?: boolean }) {
  const back = () => p.go({ tab: p.tab, p: p.p });
  return (
    <FormPage
      title={title}
      sub={sub}
      crumbs={crumbsOf(p.d, ...(p.tab ? [{ label: p.tab[0]?.toUpperCase() + p.tab.slice(1), to: `/resources/lambda/functions/${encodeURIComponent(p.d.name)}?tab=${p.tab}` }] : []), { label, to: "#" })}
      request={request}
      showInput
      callService={callService}
      refusal={refusalOf(refusal)}
      callNote={callNote}
      banner={banner}
      aside={aside}
      submitLabel={submitLabel}
      pending={pending}
      disabled={disabled}
      disabledWhy={disabledWhy}
      onCancel={back}
      onSubmit={onSubmit}
      testId={testId}
    >
      {children}
    </FormPage>
  );
}

/** The muted words after a rule (`Changed from 256 MB. Stored, not applied locally.`). */
export const Muted = ({ children }: { children: React.ReactNode }) => <span className="text-muted-foreground">{children}</span>;

/** A setting doze-aws does not have: its title, why, and an Expand that is off. */
export function NotHere({ title, why, optional = false, testId }: { title: string; why: React.ReactNode; optional?: boolean; testId: string }) {
  return (
    <section className="min-w-0 rounded-[14px] border bg-card p-6 opacity-80 lg:p-7" data-testid={testId}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="flex flex-wrap items-center gap-2.5 text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}{optional && <span className="rounded-md bg-secondary px-2 py-0.5 text-[12px] font-normal text-muted-foreground">Optional</span>}</h2>
          <p className="mt-1 text-[13.5px] text-muted-foreground">{why}</p>
        </div>
        <Button type="button" variant="outline" size="sm" className="bg-card" disabled>Expand</Button>
      </div>
    </section>
  );
}
