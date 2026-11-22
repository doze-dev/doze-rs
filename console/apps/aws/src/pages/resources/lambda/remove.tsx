// Taking a part of a function away (an event source mapping, a permission statement, an alias, the function URL): one confirm
// dialog for all four, each a call of its own on the function. The reads of the function are made again when it is done.
import { type ReactNode, useState } from "react";
import { on, useOp } from "../deep/api";
import { Confirm } from "../deep/parts";

export type Removal = { title: string; input: Record<string, unknown>; text: ReactNode; /** Said after the call. */ then?: () => void };

/** `ask(removal)` opens the dialog for `operation` on the function `fn`; `dialog` is drawn once on the page. */
export function useRemove(fn: string, operation: string, testId: string) {
  const op = useOp(on.fn(fn), operation);
  const [what, setWhat] = useState<Removal | null>(null);
  const close = (o: boolean) => {
    if (!o) {
      setWhat(null);
      op.reset();
    }
  };
  const dialog = (
    <Confirm open={what !== null} onOpenChange={close} title={what?.title ?? ""} verb="Delete" operation={operation} pending={op.isPending} error={op.error} testId={testId}
      run={() => what && op.mutate(what.input, { onSuccess: () => { what.then?.(); setWhat(null); } })}>
      {what?.text}
    </Confirm>
  );
  return { ask: setWhat, dialog };
}
