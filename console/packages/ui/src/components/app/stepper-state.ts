// The pure state of a wizard's steps: which are done, which is current, where a person may jump. No React in here.

export type StepState = "done" | "current" | "upcoming";

/** The state of each of `count` steps when step `current` (0-based) is shown. */
export function stepStates(count: number, current: number): StepState[] {
  return Array.from({ length: count }, (_, i) => (i < current ? "done" : i === current ? "current" : "upcoming"));
}

/** Whether a person may go to step `index`: back to any step they have been through, forward only as far as they have reached
 * (`furthest`): a step is never skipped to, because what it asks may be what a later step needs. */
export function canGoTo(index: number, current: number, furthest: number): boolean {
  return index !== current && index >= 0 && index <= Math.max(furthest, current);
}

/** The step after `current`, never past the last (`count - 1`), and the one before, never before the first. */
export const nextStep = (current: number, count: number) => Math.min(Math.max(count - 1, 0), current + 1);
export const prevStep = (current: number) => Math.max(0, current - 1);

/** `Step 4 of 4`. */
export const stepWords = (current: number, count: number) => `Step ${current + 1} of ${count}`;
