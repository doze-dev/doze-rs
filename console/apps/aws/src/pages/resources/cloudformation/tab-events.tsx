// A stack's Events tab (cloudformation__stack-events): everything that happened to the stack and its resources, newest first,
// a page at a time (DescribeStackEvents' NextToken).
import { useState } from "react";
import { Button } from "@doze/ui/components/ui/button";
import { RuleHelp } from "../../../kit";
import { Loaded } from "../deep/parts";
import { useEvents } from "./api";
import { EventsTable, Section } from "./parts";

export function EventsTab({ name, lede, note = true }: { name: string; lede?: React.ReactNode; note?: boolean }) {
  const [tokens, setTokens] = useState<string[]>([]);
  const token = tokens[tokens.length - 1];
  const q = useEvents(name, token);
  const events = q.data?.StackEvents ?? [];
  const only = tokens.length === 0 && !q.data?.NextToken && events.length > 0 && events.every((e) => /^(CREATE|ROLLBACK)/.test(e.ResourceStatus ?? ""));
  return (
    <Section title="Events" lede={lede ?? `Everything that happened to the stack and its resources, newest first, as CloudFormation wrote it.${only ? " This stack has only been created, so this is its whole history." : ""}`} testId="stack-events">
      <Loaded q={q} what="The stack's events">
        {() => (
          <>
            <EventsTable events={events} />
            {(tokens.length > 0 || q.data?.NextToken) && (
              <div className="flex justify-end gap-2" data-testid="events-paging">
                {tokens.length > 0 && <Button variant="outline" size="sm" className="bg-card" onClick={() => setTokens(tokens.slice(0, -1))}>Newer</Button>}
                {q.data?.NextToken && <Button variant="outline" size="sm" className="bg-card" onClick={() => setTokens([...tokens, q.data?.NextToken as string])}>Older</Button>}
              </div>
            )}
          </>
        )}
      </Loaded>
      {note && <RuleHelp className="max-w-[46rem]">Each event is written as its resource is made, changed or deleted, so a row's time is when that happened. Resources are made in dependency order: one a second resource points at comes before it, and independent ones go together.</RuleHelp>}
    </Section>
  );
}
