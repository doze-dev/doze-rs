// What a publish did (sns__topic-published): who the message went to, and what the call answered (MessageId, and for a FIFO topic its SequenceNumber).
import { BracesIcon } from "lucide-react";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { Rich } from "./parts";
import { type Published, publishedSentence } from "./publish-result";
import type { Subscription } from "./sub-form-model";
import type { SubRow } from "./sub-model";

export function PublishedPanel({ published, subs, details }: { topic: string; published: Published; subs: readonly SubRow[]; details: readonly Subscription[] }) {
  return (
    <section className="mb-10 grid gap-4 rounded-[14px] border bg-card p-6 lg:p-7" aria-label="The call this made" data-testid="published-panel">
      <Rich parts={publishedSentence(published, subs, details)} testId="published-sentence" />
      <div className="max-w-[42rem] rounded-[10px] bg-well p-5">
        <h2 className="flex items-center gap-2 text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase"><BracesIcon aria-hidden className="size-3.5" /> The call this made</h2>
        <p className="mt-3 font-mono text-[13px] font-semibold text-bright">Publish <span className="font-normal text-muted-foreground">→</span> <span className="text-good">200</span></p>
        <dl className="mt-3 grid gap-3 text-[13px]">
          <div><dt className="text-muted-foreground">MessageId</dt><dd className="mt-0.5 flex items-center gap-2 font-mono text-[12.5px] text-bright" data-testid="published-message-id">{published.messageId}<CopyButton text={published.messageId} what="MessageId copied" iconOnly variant="ghost" label="Copy MessageId" /></dd></div>
          {published.sequenceNumber && <div><dt className="text-muted-foreground">SequenceNumber</dt><dd className="mt-0.5 font-mono text-[12.5px] text-bright" data-testid="published-sequence">{published.sequenceNumber}</dd></div>}
        </dl>
        {published.sequenceNumber && <p className="mt-3 text-[13px] leading-5 text-muted-foreground">SequenceNumber is a large, non-consecutive number that grows for each message group. It is returned for FIFO topics only.</p>}
      </div>
    </section>
  );
}
