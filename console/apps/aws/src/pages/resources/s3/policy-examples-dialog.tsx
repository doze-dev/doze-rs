import { useState } from "react";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { CodeBlock } from "@doze/ui/components/app/code-block";
import type { PolicyExample } from "./policy-examples";

/** The button that offers starting points for the policy; choosing one puts it in the editor (it does not save anything). */
export function PolicyExamples({ examples, onUse }: { examples: PolicyExample[]; onUse: (policy: string) => void }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => setOpen(true)} data-testid="policy-examples-open">Policy examples</Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="sm:max-w-[680px]" data-testid="policy-examples">
          <DialogHeader>
            <DialogTitle>Policy examples</DialogTitle>
            <DialogDescription>Choose one to put it in the editor. It replaces what is there, and nothing is saved until you save the policy.</DialogDescription>
          </DialogHeader>
          <div className="grid max-h-[60vh] gap-4 overflow-auto pr-1">
            {examples.map((e) => (
              <section key={e.id} className="grid gap-2 rounded-xl border p-4" data-testid={`example-${e.id}`}>
                <div className="flex items-start justify-between gap-3">
                  <div className="grid gap-0.5"><h3 className="text-[14px] font-medium text-bright">{e.title}</h3><p className="text-[13px] text-muted-foreground">{e.says}</p></div>
                  <Button type="button" size="sm" onClick={() => (onUse(e.policy), setOpen(false))} data-testid={`use-${e.id}`}>Use this</Button>
                </div>
                <CodeBlock text={e.policy} wrap={false} max="max-h-48" />
              </section>
            ))}
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}
