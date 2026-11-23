// The cause chain of a call, in its drawer: the request from the wire that led to it and the calls between services it led to,
// each a step that opens its own drawer. Only what the record says: a call between services names the service that made it,
// the rule, mapping or schedule when the service said, and the request id of the request being answered when it was made.
import { ArrowDownIcon } from "lucide-react";
import { cn } from "@doze/ui/lib/utils";
import { clockMsUtc, number } from "@doze/ui/lib/format";
import type { CallRecord } from "../../api/types/CallRecord";
import { serviceTitle } from "../../words";
import { Result } from "./outcome";

/** The call `call` was made while answering, among `held`. */
export function parentOf(call: CallRecord, held: readonly CallRecord[]): CallRecord | undefined {
  const id = call.via?.request_id;
  return id ? held.find((c) => c.via === null && c.request_id === id) : undefined;
}

/** The calls between services made while answering `call`, oldest first. */
export function childrenOf(call: CallRecord, held: readonly CallRecord[]): CallRecord[] {
  if (call.via !== null || call.request_id === "") return [];
  return held.filter((c) => c.via?.request_id === call.request_id).sort((a, b) => a.seq - b.seq);
}

/** "SNS, rule order-placed": who made a call between services. */
export function madeBy(call: CallRecord, titles: ReadonlyMap<string, string>): string {
  const via = call.via;
  if (!via) return "";
  const by = via.by ? (serviceTitle(via.by, titles) ?? via.by) : "a service, as the caller's role";
  return via.name ? `${by}, ${via.name}` : by;
}

function Step({ call, here, titles, onOpen, note }: { call: CallRecord; here?: boolean; titles: ReadonlyMap<string, string>; onOpen: (seq: number) => void; note?: string }) {
  const body = (
    <>
      <span className="flex min-w-0 items-baseline gap-2">
        <span className="truncate font-mono text-[13px] font-medium text-bright">{call.action ?? "Unknown"}</span>
        <span className="shrink-0 text-[12px] text-muted-foreground">{serviceTitle(call.service, titles)}</span>
        <span className="ml-auto shrink-0 font-mono text-[11.5px] text-muted-foreground">{clockMsUtc(call.at_ms)}</span>
      </span>
      {call.resource && <span className="block truncate font-mono text-[12px] text-foreground">{call.resource}</span>}
      <span className="mt-0.5 flex items-center gap-2 text-[12px] text-muted-foreground">
        <Result call={call} className="text-[12px]" />
        {note && <span className="truncate">{note}</span>}
      </span>
    </>
  );
  return here ? (
    <div className="rounded-lg border-2 border-primary bg-card px-3 py-2" data-testid="cause-here" aria-current="step">
      {body}
    </div>
  ) : (
    <button type="button" onClick={() => onOpen(call.seq)} className="block w-full rounded-lg border bg-card px-3 py-2 text-left outline-none hover:border-ring focus-visible:ring-2 focus-visible:ring-ring" aria-label={`Open call ${call.seq}: ${call.action ?? "unknown operation"}`} data-testid="cause-step">
      {body}
    </button>
  );
}

const Arrow = ({ words }: { words: string }) => (
  <div className="flex items-center gap-2 py-1 pl-4 text-[12px] text-muted-foreground">
    <ArrowDownIcon aria-hidden className="size-3.5 shrink-0" />
    <span>{words}</span>
  </div>
);

/** The chain, or nothing for a request from the wire that made no call between services. */
export function CauseChain({ call, held, titles, onOpen }: { call: CallRecord; held: readonly CallRecord[]; titles: ReadonlyMap<string, string>; onOpen: (seq: number) => void }) {
  const parent = parentOf(call, held);
  const children = childrenOf(call, held);
  if (call.via === null && children.length === 0) return null;
  return (
    <section className="mt-5" aria-label="Cause" data-testid="cause-chain">
      <h3 className="mb-2 text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">Cause</h3>
      {call.via !== null && (
        <>
          {parent ? (
            <Step call={parent} titles={titles} onOpen={onOpen} note={`call ${number(parent.seq)}, from ${parent.caller ?? "no key"}`} />
          ) : (
            <p className={cn("rounded-lg border border-dashed px-3 py-2 text-[12.5px] text-muted-foreground")} data-testid="cause-root">
              {call.via.request_id ? (
                <>
                  Made while doze-aws answered request <span className="font-mono break-all text-foreground">{call.via.request_id}</span>, which the record no longer holds.
                </>
              ) : (
                <>Not made while answering a request: {call.via.name ? `${call.via.name} made it on its own.` : "the service made it on its own (a timer, a poll, a retry)."}</>
              )}
            </p>
          )}
          <Arrow words={`${madeBy(call, titles)} called ${serviceTitle(call.service, titles) ?? "a service"}`} />
        </>
      )}
      <Step call={call} here titles={titles} onOpen={onOpen} />
      {children.length > 0 && (
        <>
          <Arrow words={`while answering it, doze-aws made ${children.length === 1 ? "this call" : `these ${number(children.length)} calls`} between services`} />
          <div className="flex flex-col gap-1.5">
            {children.map((c) => (
              <Step key={c.seq} call={c} titles={titles} onOpen={onOpen} note={madeBy(c, titles)} />
            ))}
          </div>
        </>
      )}
    </section>
  );
}
