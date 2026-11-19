import { InboxIcon, Loader2Icon, PlusIcon, RefreshCwIcon, SearchXIcon, ServerOffIcon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { CodeValue } from "@doze/ui/components/app/code-value";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Done } from "@doze/ui/components/app/write-done";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { LONG } from "../data";
import { Section, Specimen } from "../specimen";

export function StatesPage() {
  const [tries, setTries] = useState(0);
  return (
    <>
      <PageHeader title="States" sub="What a place says when it is loading, has nothing, or failed. Each reserves its height, so the page does not jump when the data arrives." />

      <Section id="loading" title="Loading" source="components/ui/skeleton.tsx" note="A skeleton has the shape of what is coming. With reduced motion it does not pulse.">
        <Specimen label="Tiles" stack>
          <div className="grid w-full grid-cols-2 gap-3 md:grid-cols-4" aria-busy aria-label="Loading figures">
            {[0, 1, 2, 3].map((i) => (
              <div key={i} className="rounded-xl border bg-card px-5 py-4">
                <Skeleton className="h-3 w-16" />
                <Skeleton className="mt-3 h-6 w-24" />
                <Skeleton className="mt-2.5 h-3 w-20" />
              </div>
            ))}
          </div>
        </Specimen>
        <Specimen label="Rows" stack>
          <div className="w-full rounded-xl border bg-card" aria-busy aria-label="Loading rows">
            {[72, 56, 64, 48].map((w, i) => (
              <div key={i} className="flex h-10 items-center gap-6 border-b px-3 last:border-b-0">
                <Skeleton className="h-3.5" style={{ width: `${w * 3}px` }} />
                <Skeleton className="ml-auto h-3.5 w-16" />
                <Skeleton className="h-3.5 w-20" />
              </div>
            ))}
          </div>
        </Specimen>
        <Specimen label="A control">
          <Button disabled aria-busy>
            <Loader2Icon className="animate-spin" /> Creating…
          </Button>
          <Skeleton className="h-[30px] w-72 rounded-full" />
        </Specimen>
      </Section>

      <Section id="empty" title="Empty" source="components/app/empty-state.tsx" note="An icon, a title, a sentence, and the way on.">
        <Specimen label="Nothing yet" stack>
          <EmptyState
            icon={InboxIcon}
            title="No queues yet"
            className="w-full"
            action={
              <Button data-testid="empty-action">
                <PlusIcon /> Create a queue
              </Button>
            }
          >
            A queue appears here as soon as one is created, by this console or by your code.
          </EmptyState>
        </Specimen>
        <Specimen label="Nothing matches" stack>
          <EmptyState icon={SearchXIcon} title="Nothing matches “zzz”" className="w-full" action={<Button variant="outline">Clear the filter</Button>}>
            12 queues are hidden by the filter.
          </EmptyState>
        </Specimen>
      </Section>

      <Section id="error" title="Error" source="components/app/empty-state.tsx · write-done.tsx · sonner.tsx" note="An error is said where the thing would have been, in the server's own words, with a way to try again.">
        <Specimen label="A page's data" stack>
          <EmptyState
            icon={ServerOffIcon}
            title="The console did not answer"
            className="w-full"
            action={
              <Button variant="outline" onClick={() => setTries((n) => n + 1)} data-testid="retry">
                <RefreshCwIcon /> Try again
              </Button>
            }
          >
            Is the daemon still running? <span data-testid="tries">Tried {tries + 1} {tries === 0 ? "time" : "times"}.</span>
          </EmptyState>
        </Specimen>
        <Specimen label="A write refused">
          <div role="alert" className="w-full max-w-[34rem] rounded-xl border border-bad/30 bg-bad/5 p-4 text-[13.5px] leading-[21px]">
            <b className="font-semibold text-bad">QueueAlreadyExists</b>
            <div>A queue named orders exists with different attributes.</div>
          </div>
        </Specimen>
        <Specimen label="A write done">
          <div className="w-full max-w-[34rem]">
            <Done>
              <b className="font-semibold text-bright">orders</b> was purged: 1,204 messages removed.
            </Done>
          </div>
        </Specimen>
        <Specimen label="Elsewhere">
          <Button variant="outline" onClick={() => toast.error("The browser did not allow copying", { description: "Select the text and copy it by hand." })}>
            An error toast
          </Button>
        </Specimen>
      </Section>

      <Section id="long" title="Long text" source="every component" note="A long value is cut with an ellipsis on one line and shown whole on hover and focus; it never breaks a layout.">
        <Specimen label="In 20 rem">
          <CodeValue value={LONG} className="w-80" />
        </Specimen>
        <Specimen label="In a title" stack>
          <h3 className="max-w-full font-mono text-[22px] leading-[30px] font-medium break-words text-bright">{LONG}</h3>
        </Specimen>
      </Section>
    </>
  );
}
