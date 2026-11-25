import { Link } from "@tanstack/react-router";
import { PlayIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Combobox } from "@doze/ui/components/app/combobox";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Label } from "@doze/ui/components/ui/label";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useForce, useOnceForm } from "../../api/lab";
import type { LabOnceKind } from "../../api/types/LabOnceKind";
import type { LabOnceTeach } from "../../api/types/LabOnceTeach";
import { OnceLesson } from "./lesson";
import { splitAddress } from "./once-link";

/** A link to an address of the console the server named (`/resources/sqs/queues/orders`, `/live?service=sqs`). */
export function AddressLink({ address, children, className, testId }: { address: string; children: React.ReactNode; className?: string; testId?: string }) {
  const { path, search } = splitAddress(address);
  return (
    <Link to={path as "/"} search={search as never} className={className ?? "rounded-sm text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none"} data-testid={testId}>
      {children}
    </Link>
  );
}

function Body({ kind, teach, onClose }: { kind: LabOnceKind; teach: LabOnceTeach | undefined; onClose: () => void }) {
  const [picked, setPicked] = useState<string | null>(null);
  const [look, setLook] = useState(false);
  const [wake, setWake] = useState<boolean | null>(null);
  const form = useOnceForm(kind.id, picked, look);
  const force = useForce(kind.id);
  const f = form.data;
  // What is shown is what is chosen: the first choice when nothing was picked is kept as it is (the list may be ordered differently after it runs).
  const first = f?.choices[0]?.value ?? null;
  if (picked === null && first !== null && !f?.resting) setPicked(first);
  const choice = picked ?? first;
  const timed = wake ?? f?.option?.default ?? false;
  const failed = form.error ? errorWords(form.error) : force.error ? errorWords(force.error) : null;
  const done = force.data;
  const result = useRef<HTMLDivElement>(null);
  // The result can be below the fold of a dialog that holds a lesson: it is brought into view when it comes.
  useEffect(() => {
    if (done) result.current?.scrollIntoView?.({ block: "nearest" });
  }, [done]);
  const options = (f?.choices ?? []).map((c) => ({ value: c.value, label: c.label, hint: [c.state, c.detail].filter(Boolean).join(" · ") || undefined }));
  const run = (e: React.FormEvent) => {
    e.preventDefault();
    if (choice && !force.isPending) force.mutate({ choice, ...(f?.option ? { time_wake: timed } : {}) });
  };
  return (
    <form onSubmit={run} className="grid gap-5" data-testid="once-form" data-kind={kind.id}>
      <DialogHeader>
        <DialogTitle className="text-[21px] font-semibold">{kind.title}</DialogTitle>
        <DialogDescription className="text-[14px] leading-[22px]">{kind.lede}</DialogDescription>
      </DialogHeader>
      {teach && (
        <details className="rounded-lg border bg-card px-4 py-3" data-testid="once-lesson">
          <summary className="cursor-pointer text-[13.5px] font-medium text-bright">What this stands for and what to expect</summary>
          <div className="mt-3"><OnceLesson teach={teach} /></div>
        </details>
      )}
      {!f ? (
        failed ? <p className="text-[13px] text-destructive" role="alert" data-testid="once-error">{failed.message}</p> : <Skeleton className="h-24 rounded-xl" aria-label="Loading" />
      ) : (
        <div className="grid gap-4">
          {f.resting && (
            <Alert tone="info" title={`${f.note ?? "It is resting, so it was not looked at."}`} testId="once-resting" action={<Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => setLook(true)} data-testid="once-look">Look anyway</Button>}>
              Looking is a call, and a call starts it. It rests again when it has been idle.
            </Alert>
          )}
          {f.empty ? (
            <p className="rounded-lg border border-dashed px-4 py-5 text-[13.5px] text-muted-foreground" data-testid="once-empty">{f.empty}</p>
          ) : (
            f.choices.length > 0 && (
              <div className="grid gap-1.5">
                <Label htmlFor="once-choice">Which {f.noun}</Label>
                <Combobox id="once-choice" aria-label={`Which ${f.noun}`} options={options} value={choice} onChange={(v) => v && setPicked(v)} search={`Search ${f.noun}s`} className="w-full" />
                {f.note && !f.resting && <p className="text-[12.5px] text-muted-foreground">{f.note}</p>}
              </div>
            )
          )}
          {f.option && (
            <div className="flex items-center gap-2.5">
              <Checkbox id="once-option" checked={timed} onCheckedChange={(c) => setWake(c === true)} data-testid="once-option" />
              <Label htmlFor="once-option" className="font-normal">{f.option.label}</Label>
            </div>
          )}
          {f.preview && <p className="border-l-2 border-input pl-3.5 text-[13px] leading-5 text-foreground" data-testid="once-preview">{f.preview}</p>}
        </div>
      )}
      {failed && f && <p className="text-[13px] text-destructive" role="alert" data-testid="once-error">{failed.message}</p>}
      {done && (
        <div ref={result}>
          <Alert tone={done.ok ? "good" : "warn"} title={done.ok ? "Done" : "Nothing was done"} testId="once-result" action={done.link ? <AddressLink address={done.link} testId="once-result-link">See the result</AddressLink> : undefined}>
            {done.sentence}
          </Alert>
        </div>
      )}
      <div className="flex justify-end gap-2">
        <Button type="button" variant="ghost" onClick={onClose} data-testid="once-close">{done ? "Close" : "Cancel"}</Button>
        <Button type="submit" disabled={!choice || force.isPending || !!f?.empty} data-testid="once-run"><PlayIcon /> {force.isPending ? "Working…" : done ? "Run again" : "Run now"}</Button>
      </div>
    </form>
  );
}

/** A forced event: what there is to choose in the region chosen (read when it opens), what forcing it does, and the result in the server's
 * words. A refusal in words is shown as such. */
export function OnceDialog({ kind, teach, onClose }: { kind: LabOnceKind | null; teach: LabOnceTeach | undefined; onClose: () => void }) {
  return (
    <Dialog open={kind !== null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[640px]" data-testid="once-dialog">
        {kind && <Body kind={kind} teach={teach} onClose={onClose} />}
      </DialogContent>
    </Dialog>
  );
}
