import { InfoIcon } from "lucide-react";
import { Button } from "@doze/ui/components/ui/button";
import { Sheet, SheetClose, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from "@doze/ui/components/ui/sheet";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import type { ServiceRow } from "../../api/types/ServiceRow";

function Snippet({ label, code, what, testId }: { label: string; code: string; what: string; testId: string }) {
  return (
    <div>
      <div className="mb-1.5 text-[11.5px] font-medium tracking-wide text-muted-foreground uppercase">{label}</div>
      <div className="flex items-center gap-3 rounded-xl border bg-well px-4 py-3">
        <code className="min-w-0 flex-1 font-mono text-[13px] break-all text-foreground" data-testid={testId}>
          {code}
        </code>
        <CopyButton text={code} iconOnly variant="ghost" label={`Copy ${what}`} what={`${what} copied`} />
      </div>
    </div>
  );
}

/** Which services run: where the list is set (a start-up setting, so a restart), what is served now, and what a service left
 * out answers. Not a switch: a service left out is not served by this instance at all. */
export function WhichServicesSheet({ open, onOpenChange, services }: { open: boolean; onOpenChange: (open: boolean) => void; services: readonly ServiceRow[] }) {
  const left = services.filter((s) => !s.served);
  const subset = (left.length > 0 ? services.filter((s) => s.served) : services.slice(0, 3)).map((s) => s.name);
  const list = subset.join(",");
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-[520px]" data-testid="services-sheet">
        <SheetHeader className="gap-2 px-7 pt-7 pb-4">
          <SheetTitle className="text-[20px] font-semibold tracking-[-0.01em] text-bright">Which services run</SheetTitle>
          <SheetDescription className="text-[13.5px]" data-testid="services-sheet-lede">
            {left.length === 0 ? `All ${services.length} services run by default.` : `${services.length - left.length} of ${services.length} services run in this instance.`} Listing fewer is a start-up setting, not a switch in this console, because a service you leave out is not served by this instance at all.
          </SheetDescription>
        </SheetHeader>
        <div className="grid gap-5 px-7 pb-6">
          {left.length > 0 && (
            <p className="text-[13.5px]" data-testid="services-left-out">
              Left out in this run: <span className="font-medium text-bright">{left.map((s) => s.title).join(", ")}</span>. A call to one is answered 403 OptInRequired, in words that say how to turn it on.
            </p>
          )}
          <Snippet label="In doze-aws.toml" code={`services = [${subset.map((n) => `"${n}"`).join(", ")}]`} what="the setting" testId="services-toml" />
          <Snippet label="On the command line" code={`doze-aws --services ${list}`} what="the command" testId="services-flag" />
          <Snippet label="Or in the environment" code={`DOZE_AWS_SERVICES=${list}`} what="the variable" testId="services-env" />
          <div className="flex gap-3 rounded-xl border bg-card px-4 py-3.5 text-[13.5px]">
            <InfoIcon className="mt-0.5 size-4 shrink-0 text-cool" aria-hidden />
            <p>
              <b className="font-semibold text-bright">Takes a restart.</b> A flag or variable that overrules the config file is said in the start-up message, with what the file had, so a stack is not started on a subset by accident. Unknown service names are an error.
            </p>
          </div>
        </div>
        <SheetFooter className="border-t px-7 py-4">
          <SheetClose asChild>
            <Button variant="outline" className="bg-card" data-testid="services-sheet-close">
              Close
            </Button>
          </SheetClose>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}
