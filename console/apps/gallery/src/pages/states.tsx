import { InboxIcon, Loader2Icon, PlusIcon, RefreshCwIcon, SearchXIcon, ServerOffIcon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { Alert, ErrorBlock } from "@doze/ui/components/app/alert";
import { CodeValue } from "@doze/ui/components/app/code-value";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Done } from "@doze/ui/components/app/write-done";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { ApiFailure } from "@doze/ui/lib/api";
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

      <Section id="alert" title="Alert and ErrorBlock" source="components/app/alert.tsx" note="A block that says something happened, where it happened. The tone colours the icon only; the words carry the meaning. An ErrorBlock is the alert of a fetch that failed: what could not be had, the server's words and code, what to do, and Try again.">
        <Specimen label="Tones" stack>
          <div className="grid w-full gap-3 md:grid-cols-2">
            <Alert tone="info" title="The record keeps the newest 1,024 calls" testId="alert-info">
              Older ones are dropped as new ones arrive.
            </Alert>
            <Alert tone="good" title="The queue was created" testId="alert-good">
              It is empty and ready for messages.
            </Alert>
            <Alert tone="warn" title="The tail is paused" testId="alert-warn" action={<Button size="sm" variant="outline" className="bg-card">Resume</Button>}>
              12 calls arrived since. They are shown when you resume.
            </Alert>
            <Alert tone="bad" title="The stream was refused" testId="alert-bad">
              The console has as many event streams open as it allows.
            </Alert>
          </div>
        </Specimen>
        <Specimen label="A failed fetch" stack>
          <ErrorBlock
            className="w-full"
            what="The services could not be loaded"
            error={new ApiFailure(0, { code: "UNREACHABLE", message: "The console did not answer. Is it still running?" })}
            hint="Start it again, then try again. Nothing on this page was changed."
            onRetry={() => setTries((n) => n + 1)}
          />
        </Specimen>
        <Specimen label="A refusal" stack>
          <ErrorBlock className="w-full" testId="error-refusal" what="The calls could not be loaded" error={new ApiFailure(403, { code: "FORBIDDEN", message: "The console's API answers only its own pages, which send the header X-Console: 1." })} />
        </Specimen>
        <Specimen label="Long text">
          <Alert tone="info" title={LONG} className="w-80" testId="alert-long" />
        </Specimen>
      </Section>

      <Section id="long"title="Long text" source="every component" note="A long value is cut with an ellipsis on one line and shown whole on hover and focus; it never breaks a layout.">
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
