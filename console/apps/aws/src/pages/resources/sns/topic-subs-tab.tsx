// A topic's Subscriptions tab (sns__topic-detail, topic-alerts, topic-subscriptions-none, topic-created): who the topic delivers to, with each one's
// status, and the way to subscribe another or to confirm one that waits.
import { CheckIcon, LinkIcon, PlusIcon, SearchIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { Block } from "../sqs/parts";
import { RadioRows } from "./parts";
import { subColumns } from "./subs-table";
import { orderSubs, topicSubRow, type SubRow } from "./sub-model";

export const topicSubs = (d: ResourceDetail, arn: string): SubRow[] => orderSubs((d.subs.find((s) => s.title === "Subscriptions")?.rows ?? []).map((r) => topicSubRow(r, arn)));

export function SubscriptionsTab({ d, arn, fifo, create, confirm }: { d: ResourceDetail; arn: string; fifo: boolean; create: () => void; confirm: (s: SubRow) => void }) {
  const rows = useMemo(() => topicSubs(d, arn), [d, arn]);
  const [typed, setTyped] = useState("");
  const [chosen, setChosen] = useState<string | null>(null);
  const pick = rows.find((r) => r.name === chosen) ?? null;
  const shown = rows.filter((r) => `${r.endpoint} ${r.id ?? ""} ${r.protocol}`.toLowerCase().includes(typed.trim().toLowerCase()));
  const waiting = rows.some((r) => r.pending);
  const sub = d.subs.find((s) => s.title === "Subscriptions");
  const make = <Button type="button" variant="outline" className="bg-card" onClick={create} data-testid="sub-create"><PlusIcon /> Create subscription</Button>;
  if (sub?.error) return <p className="text-[13.5px] text-bad">The subscriptions could not be read: {sub.error}</p>;
  if (rows.length === 0) {
    return (
      <Block title="Subscriptions" testId="subscriptions-tab">
        <EmptyTab icon={LinkIcon} title="No subscriptions" action={make} testId="subscriptions-empty">
          {fifo ? <>Nothing is subscribed to this topic. To subscribe to a FIFO topic, choose the Amazon SQS protocol.</> : <>Nothing is subscribed to {d.name}, so a message published to it is delivered nowhere.</>}
        </EmptyTab>
      </Block>
    );
  }
  return (
    <Block title="Subscriptions" testId="subscriptions-tab">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="relative w-full max-w-[22rem]">
          <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search subscriptions" aria-label="Search subscriptions" className="bg-card pl-9" data-testid="subs-search" />
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {waiting && <Button type="button" variant="outline" className="bg-card" disabled={!pick?.pending} title={pick?.pending ? undefined : "Choose a subscription that is pending confirmation."} onClick={() => pick && confirm(pick)} data-testid="sub-confirm"><CheckIcon /> Confirm subscription</Button>}
          {make}
        </div>
      </div>
      <RadioRows value={chosen} onChange={setChosen}>
        <DataTable aria-label="Subscriptions" data={shown} columns={subColumns({ radio: true, topic: false })} rowId={(r) => r.name} maxHeight={420} rowHeight={56} rowClassName={(r) => (r.name === chosen ? "[&>td]:bg-pick/50" : undefined)} onRowClick={(r) => setChosen(r.name)} empty="No subscription matches." />
      </RadioRows>
      {sub?.more && <p className="text-[13px] text-muted-foreground">The first page only; there are more.</p>}
    </Block>
  );
}
