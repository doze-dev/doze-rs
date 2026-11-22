// The update wizard's choices, kept while the person reads the preview's change set on a page of its own (View change set): coming back
// (the browser's Back, which lands on `?do=update&mode=resume`) finds the review step as it was left, not the first step with nothing chosen.
// Held in memory for one stack at a time; an Update begun from the stack's page (no `mode=resume`) starts fresh and drops it.
import type { Summary, Wizard } from "./wizard-model";

export type Kept = { stack: string; w: Wizard; step: number; furthest: number; summary: Summary | undefined; previewId: string };

let kept: Kept | null = null;

export function keepWizard(k: Kept): void {
  kept = k;
}

/** What was kept for `stack`, when `resume` (the address says it is a return); else nothing, and what was kept is dropped. (Asking again while
 * it is a return gives it again: a page may ask twice as it starts.) */
export function takeWizard(stack: string, resume: boolean): Kept | null {
  if (!resume) kept = null;
  return resume && kept?.stack === stack ? kept : null;
}
