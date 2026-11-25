import { Link } from "@tanstack/react-router";
import { ExternalLinkIcon, SquareTerminalIcon, TerminalIcon } from "lucide-react";
import { Alert } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@doze/ui/components/ui/sheet";
import { clockMsUtc, dateTimeUtc, micros, number } from "@doze/ui/lib/format";
import type { CallRecord } from "../../api/types/CallRecord";
import { outcomeSentence, WOULD_BE_DENIED } from "../../words";
import { copyCommand } from "./actions";
import { CauseChain, madeBy } from "./cause";
import { Explain } from "./explain";
import { NotBuilt } from "./not-built";
import { Refused } from "./refused";
import { Result } from "./outcome";
import { labSentence } from "../../words";
import { resourceHref, resourceName, resourceSearchOf } from "./resource";

/** One field of the call: its name, its value (or why there is none), and a line on what it is. */
function Field({ label, value, none, note, mono = true, copy, testId }: { label: string; value: string | null; /** What an absent value means. */ none?: string; note?: string; mono?: boolean; copy?: boolean; testId: string }) {
  return (
    <>
      <dt className="border-b py-2.5 pr-4 text-[12.5px] text-muted-foreground">{label}</dt>
      <dd className="min-w-0 border-b py-2.5 text-[13px]" data-testid={`call-${testId}`}>
        <div className="flex min-w-0 items-start gap-2">
          {value === null ? <span className="text-muted-foreground">{none ?? "None"}</span> : <span className={mono ? "min-w-0 font-mono text-[12.5px] break-all text-bright" : "min-w-0 break-words text-bright"}>{value}</span>}
          {copy && value !== null && <CopyButton text={value} iconOnly variant="ghost" label={`Copy the ${label.toLowerCase()}`} what={`${label} copied`} className="-my-1.5 ml-auto shrink-0" />}
        </div>
        {note && <div className="mt-0.5 text-[12px] text-muted-foreground">{note}</div>}
      </dd>
    </>
  );
}

/** The drawer of one call: everything the contract has for it (crates/aws-console/src/model.rs, `CallRecord`), and nothing
 * else. An overlay on Live, with its own address (`?call=<number>`); `call` undefined with `seq` set is a call the record no
 * longer holds. */
export function CallSheet({ seq, call, serviceTitle, titles, held, onOpen, onClose }: { seq: number | undefined; call: CallRecord | undefined; /** The service's title, when the build has it. */ serviceTitle: string | null; titles: ReadonlyMap<string, string>; /** Every call on the tape, for the cause chain. */ held: readonly CallRecord[]; onOpen: (seq: number) => void; onClose: () => void }) {
  const href = call ? resourceHref(call) : null;
  return (
    <Sheet open={seq !== undefined} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-[520px]" data-testid="call-sheet" aria-describedby="call-sheet-description">
        <SheetHeader className="gap-1 px-7 pt-7 pb-4">
          <SheetTitle className="pr-8 font-mono text-[19px] leading-7 font-medium tracking-[-0.01em] break-all text-bright" data-testid="call-title">
            {call ? (call.action ?? "Unknown operation") : `Call ${seq === undefined ? "" : number(seq)}`}
          </SheetTitle>
          <SheetDescription id="call-sheet-description" className="text-[13px]">
            {call ? (
              <>
                {serviceTitle ?? "No service of this build"} · call {number(call.seq)} · {clockMsUtc(call.at_ms)} UTC
              </>
            ) : (
              "A call of this doze-aws"
            )}
          </SheetDescription>
        </SheetHeader>
        {call ? (
          <div className="px-7 pb-8">
            <div className="rounded-xl border bg-card p-4" data-testid="call-summary">
              <Result call={call} className="text-[13.5px]" />
              <p className="mt-1.5 text-[13px] text-foreground">{outcomeSentence(call)}</p>
              {call.via && (
                <p className="mt-1.5 text-[13px] text-foreground" data-testid="call-via">
                  A call between services: {madeBy(call, titles)} made it on doze-aws's internal bus, not over the wire.
                </p>
              )}
              {call.function_error && (
                <p className="mt-2 rounded-md bg-bad/10 px-2.5 py-1.5 text-[13px] text-bad" data-testid="call-function-error">
                  The function failed ({call.function_error}). The invocation itself answered {call.status}: the failure is in the answer, in X-Amz-Function-Error and the error's own text, which the record does not keep.
                </p>
              )}
              {call.lab && (
                <p className="mt-2 rounded-md bg-muted px-2.5 py-1.5 text-[13px] text-foreground" data-testid="call-lab">
                  {labSentence(call.lab)}
                </p>
              )}
              {call.would_be_denied && (
                <p className="mt-2 rounded-md bg-tint-warn px-2.5 py-1.5 text-[13px] text-warn" data-testid="call-would-be-denied">
                  {WOULD_BE_DENIED.sentence}
                </p>
              )}
              <p className="mt-2 text-[12px] text-muted-foreground" data-testid="no-replay">
                Replay is not available: the record keeps no request bodies, so a call cannot be sent again from here.
              </p>
              <div className="mt-3 flex flex-wrap gap-2" data-testid="call-actions">
                <Button variant="outline" size="sm" className="bg-card" disabled={call.action === null} onClick={() => void copyCommand(call.seq, "cli")} data-testid="call-copy-cli">
                  <SquareTerminalIcon /> Copy as AWS CLI
                </Button>
                <Button variant="outline" size="sm" className="bg-card" disabled={call.action === null} onClick={() => void copyCommand(call.seq, "curl")} data-testid="call-copy-curl">
                  <TerminalIcon /> Copy as curl
                </Button>
                {href && (
                  <Button asChild variant="outline" size="sm" className="max-w-full bg-card" data-testid="call-open-resource">
                    <Link to={href} search={resourceSearchOf(call)}>
                      <ExternalLinkIcon /> Open <span className="truncate font-mono text-[12px]">{resourceName(call)}</span>
                    </Link>
                  </Button>
                )}
              </div>
            </div>
            {(call.would_be_denied || call.outcome === "denied") && <Explain call={call} />}
            {call.outcome === "not-built" && <NotBuilt call={call} />}
            <Refused call={call} />
            <CauseChain call={call} held={held} titles={titles} onOpen={onOpen} />
            <dl className="mt-5 grid grid-cols-[8.5rem_minmax(0,1fr)] [&>dd:last-of-type]:border-b-0 [&>dt:last-of-type]:border-b-0">
              <Field testId="at" label="Answered" value={dateTimeUtc(call.at_ms).replace(" UTC", `.${clockMsUtc(call.at_ms).slice(-3)} UTC`)} />
              <Field testId="duration" label="Took" value={micros(call.duration_us)} note={`${number(call.duration_us)} µs, from the request's head being read to its answer being made.${call.lab?.effect.kind === "delayed" ? ` It includes +${number(call.lab.effect.ms)} ms that Lab rule ${call.lab.id} made it wait.` : ""}`} />
              <Field testId="service" label="Service" value={call.service} none="The request named no service of this build." note={serviceTitle && serviceTitle !== call.service ? serviceTitle : undefined} />
              <Field testId="api" label="API" value={call.api} none="None, with the service." note="The API's SDK id." />
              <Field testId="action" label="Operation" value={call.action} none="The request was refused before an operation was found." />
              <Field testId="resource" label="Resource" value={call.resource} none="Not known: the call named nothing doze-aws looks for (a queue, table, function, topic, bucket and the like)." note={call.member ? `The call's ${call.member}.` : call.resource ? "The S3 path the request was sent to." : undefined} copy />
              <Field testId="caller" label="Caller" value={call.caller} none="None recorded: the request carried no access key, or was refused before a service read it." note={call.via ? "The access key of the caller the service made this call for." : "The access key id the request was signed with."} copy />
              <Field testId="region" label="Region" value={call.region} none="None recorded: the request was signed for no region, or was refused before a service read it." />
              <Field testId="did" label="What it did" value={call.summary} none="Not said: doze-aws reads a few words (a count, a size) from the answer of some operations only, and nothing from a call that failed." note="Read from the call's input and answer as it went by. The record keeps no bodies." />
              <Field testId="status" label="HTTP status" value={String(call.status)} />
              <Field testId="error-code" label="Error code" value={call.error_code} none="None: the call succeeded." copy />
              <Field testId="outcome" label="Outcome" value={call.outcome} />
              <Field testId="request-id" label="Request id" value={call.request_id || null} none="None: a call between services has no request of its own." note="What the answer carried in x-amzn-RequestId." copy />
              <Field testId="seq" label="Number" value={String(call.seq)} note="Its number in this run of doze-aws: 1 for the first call, no gaps." />
            </dl>
          </div>
        ) : (
          <div className="px-7 pb-8">
            <Alert tone="info" title="This call is no longer in the record" testId="call-gone">
              The record keeps only the newest calls, in memory, and starts again when doze-aws does. Close this and pick a call from the list.
            </Alert>
          </div>
        )}
      </SheetContent>
    </Sheet>
  );
}
