// Pieces the function's forms share: the page's frame (crumbs, back, the call it makes), a number with its unit inside the field,
// and the words under a field that say what Lambda does with the setting here.
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
