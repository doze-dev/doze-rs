import { ArrowLeftIcon, ArrowRightIcon } from "lucide-react";
import { PageHeader, type Crumb } from "@doze/ui/components/app/page-header";
import { type Step, Stepper } from "@doze/ui/components/app/stepper";
import { Button } from "@doze/ui/components/ui/button";
import { cn } from "@doze/ui/lib/utils";
import { CallPanel } from "./call-panel";
import { CallsPanel } from "./calls-panel";
import { focusFirstProblem } from "./form-parts";
import type { PreviewRequest } from "./contract";

/** A wizard's position and moves. On the last step the submit button takes the place of Next. */
export type Wizard = { steps: Step[]; current: number; furthest?: number; onStep: (index: number) => void; onBack: () => void; onNext: () => void; nextLabel?: string; /** Next is off (the step's problem is on screen); the footer says nothing more. */ nextDisabled?: boolean };

/** A full page for a create or an edit: the header, an optional banner (a refusal, or what was just done), the sections of the
 * form (children: {@link FormSection}s) with the footer, and the CallPanel docked at the right, so what will be sent is in view
 * while it is typed. With `wizard`, the Stepper is above the title and the footer is Back and Next until the last step. The
 * footer says why the button is off (`disabledWhy`) rather than leaving a person to guess. */
export function FormPage({ title, sub, crumbs, banner, request, requests, refusal, callNote, callTitle, backLabel = "Back", hideFooter = false, panel, callService = false, showInput = false, aside, children, submitLabel, onSubmit, onCancel, cancelLabel = "Cancel", pending = false, disabled = false, disabledWhy, wizard, mono = false, testId = "form-page" }: { title: string; sub?: React.ReactNode; crumbs?: Crumb[]; banner?: React.ReactNode; /** The call the form makes; `null` until there is something to send. */ request: PreviewRequest | null; /** The calls, when the form makes more than one (they are shown in order, in place of `request`). */ requests?: PreviewRequest[]; /** The service's refusal of the last submit: shown on the panel as what came back. */ refusal?: { code: string; message: string } | null; callNote?: React.ReactNode; /** The call panel's heading, when it is not `The call this will make`. */ callTitle?: string; /** The wizard's button to the step before. */ backLabel?: string; /** The page has its own buttons (the result of a wizard): no footer. */ hideFooter?: boolean; /** Another panel in the place of the call panel (what a step builds, when it is not a call). */ panel?: React.ReactNode; /** Name the service in the call panel (the call is to another service than the page's). */ callService?: boolean; /** See CallPanel. */ showInput?: boolean; /** More panels under the CallPanel (what the existing resource already has). */ aside?: React.ReactNode; children: React.ReactNode; submitLabel: string; onSubmit: () => void; onCancel: () => void; cancelLabel?: string; pending?: boolean; disabled?: boolean; disabledWhy?: React.ReactNode; wizard?: Wizard; mono?: boolean; testId?: string }) {
  const last = !wizard || wizard.current === wizard.steps.length - 1;
  return (
    <div data-testid={testId}>
      <PageHeader title={title} sub={sub} crumbs={crumbs} mono={mono} belowCrumbs={wizard && <Stepper steps={wizard.steps} current={wizard.current} furthest={wizard.furthest} onStep={wizard.onStep} />} />
      {banner && <div className="mb-6">{banner}</div>}
      <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_22.5rem]">
        <form
          className="grid min-w-0 gap-6"
          noValidate
          onSubmit={(e) => {
            e.preventDefault();
            if (last) {
              if (!disabled && !pending) {
                const form = e.currentTarget;
                onSubmit();
                // The problems a submit finds are said on their fields once the page has drawn them.
                setTimeout(() => focusFirstProblem(form), 80);
              }
            } else wizard?.onNext();
          }}
        >
          {children}
          {!hideFooter && (
          <div className="flex flex-wrap items-center justify-end gap-x-4 gap-y-2 border-t pt-5" data-slot="form-footer">
            {disabled && disabledWhy && (
              <p className="mr-auto text-[13px] text-muted-foreground" data-testid="disabled-why">
                {disabledWhy}
              </p>
            )}
            <Button type="button" variant="outline" className="bg-card" onClick={onCancel} data-testid="form-cancel">
              {cancelLabel}
            </Button>
            {wizard && wizard.current > 0 && (
              <Button type="button" variant="outline" className="bg-card" onClick={wizard.onBack} data-testid="form-back">
                <ArrowLeftIcon /> {backLabel}
              </Button>
            )}
            <Button type="submit" disabled={last ? disabled || pending : Boolean(wizard?.nextDisabled)} data-testid="form-submit" className={cn(!last && "pr-3")}>
              {last ? (pending ? "Working…" : submitLabel) : (
                <>
                  {wizard?.nextLabel ?? "Next"} <ArrowRightIcon />
                </>
              )}
            </Button>
          </div>
          )}
        </form>
        <div className="grid min-w-0 gap-6 xl:sticky xl:top-6" data-slot="form-aside">
          {panel ?? (requests ? <CallsPanel requests={requests} refusal={refusal} note={callNote} title={callTitle} /> : <CallPanel request={request} refusal={refusal} note={callNote} title={callTitle} showService={callService} showInput={showInput} />)}
          {aside}
        </div>
      </div>
    </div>
  );
}
