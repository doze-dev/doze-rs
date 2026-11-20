import { Alert } from "@doze/ui/components/app/alert";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@doze/ui/components/ui/sheet";
import { clockMsUtc, dateTimeUtc, micros, number } from "@doze/ui/lib/format";
import type { CallRecord } from "../../api/types/CallRecord";
import { outcomeSentence } from "../../words";
import { Result } from "./outcome";

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
export function CallSheet({ seq, call, serviceTitle, onClose }: { seq: number | undefined; call: CallRecord | undefined; /** The service's title, when the build has it. */ serviceTitle: string | null; onClose: () => void }) {
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
            </div>
            <dl className="mt-5 grid grid-cols-[8.5rem_minmax(0,1fr)] [&>dd:last-of-type]:border-b-0 [&>dt:last-of-type]:border-b-0">
              <Field testId="at" label="Answered" value={dateTimeUtc(call.at_ms).replace(" UTC", `.${clockMsUtc(call.at_ms).slice(-3)} UTC`)} />
              <Field testId="duration" label="Took" value={micros(call.duration_us)} note={`${number(call.duration_us)} µs, from the request's head being read to its answer being made.`} />
              <Field testId="service" label="Service" value={call.service} none="The request named no service of this build." note={serviceTitle && serviceTitle !== call.service ? serviceTitle : undefined} />
              <Field testId="api" label="API" value={call.api} none="None, with the service." note="The API's SDK id." />
              <Field testId="action" label="Operation" value={call.action} none="The request was refused before an operation was found." />
              <Field testId="resource" label="Resource" value={call.resource} none="Not known: the gateway names what was acted on for S3 only, until the services name their resources." copy />
              <Field testId="caller" label="Caller" value={call.caller} none="None recorded: the request carried no access key, or was refused before a service read it." note="The access key id the request was signed with." copy />
              <Field testId="region" label="Region" value={call.region} none="None recorded: the request was signed for no region, or was refused before a service read it." />
              <Field testId="status" label="HTTP status" value={String(call.status)} />
              <Field testId="error-code" label="Error code" value={call.error_code} none="None: the call succeeded." copy />
              <Field testId="outcome" label="Outcome" value={call.outcome} />
              <Field testId="request-id" label="Request id" value={call.request_id} note="What the answer carried in x-amzn-RequestId." copy />
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
