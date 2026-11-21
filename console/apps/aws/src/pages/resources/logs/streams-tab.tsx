// A log group's streams, the newest event first (DescribeLogStreams), each opening in the stream viewer.
import { useState } from "react";
import { Button } from "@doze/ui/components/ui/button";
import { on, useOpRead } from "../deep/api";
import { type LogStream, StreamTable } from "../lambda/lists";
import { Card, Loaded, Made } from "../deep/parts";

export function GroupStreams({ group }: { group: string }) {
  const [tokens, setTokens] = useState<string[]>([]);
  const token = tokens[tokens.length - 1];
  const q = useOpRead<{ logStreams?: LogStream[]; nextToken?: string }>(on.group(group), "DescribeLogStreams", { logGroupName: group, orderBy: "LastEventTime", descending: true, limit: 50, ...(token ? { nextToken: token } : {}) });
  return (
    <Card title="Log streams" lede="Each source of events writes a stream of its own; the one with the newest event first. Open one to read it, or search them all on the Search tab." testId="group-streams">
      <Loaded q={q} what="The log streams">
        {(p) => (
          <>
            <StreamTable group={group} streams={p.logStreams ?? []} />
            <div className="mt-3 flex items-center justify-end gap-2">
              {tokens.length > 0 && <Button variant="outline" size="sm" className="bg-card" onClick={() => setTokens(tokens.slice(0, -1))}>Previous page</Button>}
              {p.nextToken && <Button variant="outline" size="sm" className="bg-card" onClick={() => setTokens([...tokens, p.nextToken as string])} data-testid="streams-next">Next page</Button>}
            </div>
          </>
        )}
      </Loaded>
      <Made className="mt-3" ops={["DescribeLogStreams"]} />
    </Card>
  );
}
