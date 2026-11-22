import { ArrowLeftIcon, ArrowRightIcon } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import { PageHeader, type Crumb } from "@doze/ui/components/app/page-header";
import { type Step, Stepper } from "@doze/ui/components/app/stepper";
import { Button } from "@doze/ui/components/ui/button";
import { cn } from "@doze/ui/lib/utils";
import { CallPanel } from "./call-panel";
import { CallsPanel } from "./calls-panel";
import { withCurrent } from "./crumbs";
import { focusFirstProblem } from "./form-parts";
import type { PreviewRequest } from "./contract";

/** A wizard's position and moves. On the last step the submit button takes the place of Next. */
export type Wizard = { steps: Step[]; current: number; furthest?: number; onStep: (index: number) => void; onBack: () => void; onNext: () => void; nextLabel?: string; /** Next is off (the step's problem is on screen); the footer says nothing more. */ nextDisabled?: boolean };

/** A full page for a create or an edit: the header, an optional banner (a refusal, or what was just done), the sections of the
 * form (children: {@link FormSection}s) with the footer, and the CallPanel docked at the right, so what will be sent is in view
 * while it is typed. With `wizard`, the Stepper is above the title and the footer is Back and Next until the last step. The
 * footer says why the button is off (`disabledWhy`) rather than leaving a person to guess. */
export function FormPage({ title, sub, crumbs, crumb, banner, request, requests, refusal, callNote, callTitle, stepsAfterTitle = false, titleExtra, footerExtra, backLabel = "Back", hideFooter = false, panel, callService = false, showInput = false, shown, aside, children, submitLabel, onSubmit, onCancel, cancelLabel = "Cancel", pending = false, disabled = false, disabledWhy, wizard, mono = false, noCall = false, testId = "form-page" }: { title: string; sub?: React.ReactNode; crumbs?: Crumb[]; /** The page's last crumb, when its words are not the title's (the board's `Edit` for `Edit pick-lists`). */ crumb?: string; banner?: React.ReactNode; /** The call the form makes; `null` until there is something to send. */ request: PreviewRequest | null; /** The calls, when the form makes more than one (they are shown in order, in place of `request`). */ requests?: PreviewRequest[]; /** The service's refusal of the last submit: shown on the panel as what came back. */ refusal?: { code: string; message: string } | null; callNote?: React.ReactNode; /** The panel’s heading, when it is not `The call this will make` (`The call the test makes`). */ callTitle?: string; /** Draw a wizard’s Stepper after the title and its sentence (where some boards draw it), not above the title. */ stepsAfterTitle?: boolean; /** Beside the title: the name of the resource the form is of (a chip). */ titleExtra?: React.ReactNode; /** A button between Previous and the submit (the review step's `Create change set`). */ footerExtra?: React.ReactNode; /** The words of the wizard's back button (`Previous`). */ backLabel?: string; /** The page has its own buttons (the result of a wizard): no footer. */ hideFooter?: boolean; /** Another panel in the place of the call panel (what a step builds, when it is not a call). */ panel?: React.ReactNode; /** Name the service in the call panel (the call is to another service than the page's). */ callService?: boolean; /** See CallPanel. */ showInput?: boolean; /** See CallPanel. */ shown?: (operation: string, input: unknown) => unknown; /** More panels under the CallPanel (what the existing resource already has). */ aside?: React.ReactNode; children: React.ReactNode; submitLabel: string; onSubmit: () => void; onCancel: () => void; cancelLabel?: string; pending?: boolean; disabled?: boolean; disabledWhy?: React.ReactNode; wizard?: Wizard; mono?: boolean; /** No call panel (a wizard step that makes no call of its own): the form takes the whole width. */ noCall?: boolean; testId?: string }) {
  const last = !wizard || wizard.current === wizard.steps.length - 1;
  // The field a submit found a problem on, or the service refused, is brought into view in the commit that draws the problem:
  // not on a timer after the click, which scrolled the page under a pointer that was still down when the answer came quickly,
  // and did nothing at all when the answer came slowly.
  const form = useRef<HTMLFormElement>(null);
  const [attempts, setAttempts] = useState(0);
  const refused = refusal ? `${refusal.code}: ${refusal.message}` : "";
  useLayoutEffect(() => {
    if (attempts > 0 || refused) focusFirstProblem(form.current);
  }, [attempts, refused]);
  return (
    <div data-testid={testId}>
      <PageHeader title={title} sub={sub} crumbs={crumbs && crumbs.length > 0 ? withCurrent(crumbs, title, crumb) : crumbs} mono={mono} titleExtra={titleExtra} belowCrumbs={wizard && !stepsAfterTitle ? <Stepper steps={wizard.steps} current={wizard.current} furthest={wizard.furthest} onStep={wizard.onStep} /> : undefined} />
      {wizard && stepsAfterTitle && <div className="mb-6"><Stepper steps={wizard.steps} current={wizard.current} furthest={wizard.furthest} onStep={wizard.onStep} /></div>}
      {banner && <div className="mb-6">{banner}</div>}
      <div className={cn("grid items-start gap-6", !noCall && "xl:grid-cols-[minmax(0,1fr)_22.5rem]")}>
        <form
          ref={form}
          className="grid min-w-0 gap-6"
          noValidate
          onSubmit={(e) => {
            e.preventDefault();
            if (last) {
              if (!disabled && !pending) {
                onSubmit();
                setAttempts((n) => n + 1);
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
            {last && footerExtra}
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
        {!noCall && (
          <div className="grid min-w-0 gap-6 xl:sticky xl:top-6" data-slot="form-aside">
            {panel ?? (requests ? <CallsPanel requests={requests} refusal={refusal} note={callNote} title={callTitle} showInput={showInput} shown={shown} /> : <CallPanel request={request} refusal={refusal} note={callNote} title={callTitle} showService={callService} showInput={showInput} shown={shown} />)}
            {aside}
          </div>
        )}
      </div>
    </div>
  );
}
