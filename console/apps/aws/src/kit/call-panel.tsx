import { BracesIcon } from "lucide-react";
import { errorWords } from "@doze/ui/components/app/alert";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { cn } from "@doze/ui/lib/utils";
import type { Preview, PreviewRequest } from "./contract";
import { outlineText } from "./outline";
import { usePreview } from "./preview";
import { unflatten } from "./wire-json";

/** How the call is drawn. The boards draw it two ways: the operation's name over its input as indented JSON (every service but one),
 * and the SNS boards' outline, `Name = value` one member to a line. */
export type CallFormat = "json" | "outline";

/** What a panel is asked to show of a call, beyond the request itself. */
export type CallShape = { format?: CallFormat; service?: string; input?: unknown; /** How a list of plain values is drawn; see {@link listsOf}. */ lists?: ListStyle };

/** A short list of plain values is drawn on one line (`"types": ["REGIONAL"]`) by every board but the Lambda boards', which put every list's
 * values on a line each. A list of objects is expanded in all of them but the CloudFormation and Step Functions boards', which draw each small
 * object on a line of its own (`{ "ParameterKey": "Stage", "ParameterValue": "prod" }`): `compact`. */
export type ListStyle = "inline" | "expanded" | "compact";

/** The services whose boards draw a call's lists expanded, even a list of one plain value (the Lambda boards: `"Architectures": [\n "x86_64"\n ]`). */
const EXPANDED_LISTS = new Set(["lambda"]);
/** The services whose boards draw a list of small objects one object to a line (`stack-create-review`, `machine-tags-manage`). */
const COMPACT_LISTS = new Set(["cloudformation", "stepfunctions"]);
export const listsOf = (service: string | undefined): ListStyle => (service && EXPANDED_LISTS.has(service) ? "expanded" : service && COMPACT_LISTS.has(service) ? "compact" : "inline");

/** The most characters a small object may take on its line in the `compact` style: a longer one is spread as any other. */
const SMALL_OBJECT = 100;

const scalar = (x: unknown): boolean => typeof x !== "object" || x === null;

/** `{ "Key": "app", "Value": "larder" }` when `v` is a flat object that fits a line; else null. */
function smallObject(v: unknown): string | null {
  if (typeof v !== "object" || v === null || Array.isArray(v)) return null;
  const e = Object.entries(v);
  if (e.length === 0 || !e.every(([, x]) => scalar(x))) return null;
  const line = `{ ${e.map(([k, x]) => `${JSON.stringify(k)}: ${JSON.stringify(x)}`).join(", ")} }`;
  return line.length <= SMALL_OBJECT ? line : null;
}

/** The call as the panel draws it: the operation's name, then its input as `format` says. `input`: the operation's input as an SDK
 * takes it (a REST protocol's wire body is only some of its members); without it, the wire body less a query protocol's Action and
 * Version. `service` is named before the operation (`sns Subscribe`) for a call to a service other than the page's own. */
export function callText(p: Preview, operation: string, { format = "json", service, input, lists = "inline" }: CallShape = {}): string {
  const body = input === undefined ? withoutWire(p.json) : input;
  if (format === "outline") return outlineText(operation, body, service);
  return `${service ? `${service} ` : ""}${operation}\n${drawJson(body, "", lists)}`;
}

/** JSON as the boards draw it: indented by two, a short list of plain values on one line (`"types": ["REGIONAL"]`). */
export function drawJson(v: unknown, indent = "", lists: ListStyle = "inline"): string {
  const inner = indent + "  ";
  if (Array.isArray(v)) {
    if (v.length === 0) return "[]";
    if (lists !== "expanded" && v.every(scalar)) {
      const flat = `[${v.map((x) => JSON.stringify(x)).join(", ")}]`;
      if (flat.length <= 48) return flat;
    }
    if (lists === "compact") {
      const lines = v.map(smallObject);
      if (lines.every((l) => l !== null)) return `[\n${lines.map((l) => inner + l).join(",\n")}\n${indent}]`;
    }
    return `[\n${v.map((x) => inner + drawJson(x, inner, lists)).join(",\n")}\n${indent}]`;
  }
  if (typeof v === "object" && v !== null) {
    const e = Object.entries(v);
    if (e.length === 0) return "{}";
    return `{\n${e.map(([k, x]) => `${inner}${JSON.stringify(k)}: ${drawJson(x, inner, lists)}`).join(",\n")}\n${indent}}`;
  }
  return JSON.stringify(v) ?? "null";
}

/** The JSON of the call, as it is sent (without the operation's name over it): what the quiet Copy JSON button takes. */
export function callJson(p: Preview, input?: unknown): string {
  return JSON.stringify(input === undefined ? withoutWire(p.json) : input, null, 2);
}

/** A query-protocol request carries the wire's `Action` and `Version` beside its members; the call as a person means it has the members only. */
export function withoutWire(json: unknown): unknown {
  if (typeof json !== "object" || json === null || Array.isArray(json)) return json;
  const { Action, Version, ...members } = json as Record<string, unknown>;
  return typeof Action === "string" && typeof Version === "string" ? unflatten(members) : json;
}

/** The members the call still needs, said as words: `Needs QueueName`, `Needs QueueName and Attributes`. */
export function missingWords(missing: string[]): string {
  if (missing.length === 0) return "";
  if (missing.length === 1) return `Needs ${missing[0]}.`;
  return `Needs ${missing.slice(0, -1).join(", ")} and ${missing[missing.length - 1]}.`;
}

/** The code box of a call: wrapped, never scrolled, at any width. */
export function CallCode({ text, settling = false, testId, className }: { text: string; settling?: boolean; testId: string; className?: string }) {
  return (
    <pre className={cn("rounded-xl border bg-well px-5 py-[18px] font-mono text-[13px] leading-[22px] whitespace-pre-wrap text-foreground transition-opacity [overflow-wrap:anywhere]", settling && "opacity-60", className)} data-testid={testId}>
      {text.split("\n").map((line, i) => {
        // The indentation is padding, not text: a line that wraps would otherwise put its spaces alone on a line before a token
        // too long for the rest of it (a blank line in the middle of the call). A line that wraps hangs two columns in from where
        // its text began, so the structure still reads.
        const lead = line.length - line.trimStart().length;
        return <span key={i} className="block" style={{ paddingLeft: `${lead + 2}ch`, textIndent: "-2ch" }}>{line.trimStart() || " "}</span>;
      })}
    </pre>
  );
}

/** The row of copy buttons under a call: the AWS CLI line and the boto3 snippet, as the boards draw them, and the JSON that is sent as
 * a quiet third. The outline boards word the first two `Copy as aws cli` and `Copy as boto3`; the others say `Copy as` once. */
export function CopyCalls({ cli, boto3, json, format, plural = false, waiting = false }: { cli: string; boto3: string; json: string; format: CallFormat; plural?: boolean; /** The call on screen is still catching up with the form: what would be copied is the one before. */ waiting?: boolean }) {
  return (
    <div className="flex flex-wrap items-center gap-2" data-testid="copy-as" data-waiting={waiting ? "" : undefined}>
      {format === "json" && <span className="text-[13px] text-muted-foreground">Copy as</span>}
      <CopyButton text={cli} label={format === "json" ? "AWS CLI" : "Copy as aws cli"} what={plural ? "AWS CLI lines copied" : "AWS CLI line copied"} disabled={waiting} className="bg-card" />
      <CopyButton text={boto3} label={format === "json" ? "boto3" : "Copy as boto3"} what="boto3 snippet copied" disabled={waiting} className="bg-card" />
      <div className="basis-full"><CopyButton text={json} label="Copy JSON" what="JSON copied" disabled={waiting} variant="ghost" className="-ml-2.5" /></div>
    </div>
  );
}

/** The call a form will make, live: the request as the service sees it, from `POST /preview` (asked after a pause, the last answer
 * kept while the next is on its way), drawn as the boards draw it, with the AWS CLI line and the boto3 snippet to copy. `refusal`
 * turns it into the call that was refused, with what came back under it; `note` is the sentence under the call (what it leaves out,
 * what its answer carries). */
export function CallPanel({ request, refusal, note, title, className, showService = false, showInput = false, shown, format = "json", testId = "call-panel" }: { /** What the panel shows of the input, where the wire's name for a member is not the SDK's. */ shown?: (operation: string, input: unknown) => unknown; request: PreviewRequest | null; /** Name the service before the operation (`sns Subscribe`), for a call to a service other than the page's own. */ showService?: boolean; /** The panel shows the request's own input, as the SDK takes it, not the wire body the preview answers (they differ for REST services: S3's bucket is in the address). */ showInput?: boolean; /** The service's refusal of this call: its code and words. */ refusal?: { code: string; message: string } | null; note?: React.ReactNode; title?: string; className?: string; /** The SNS boards draw the call as an outline, every other board as JSON. */ format?: CallFormat; testId?: string }) {
  const preview = usePreview(request);
  const p = preview.data;
  const failed = preview.error ? errorWords(preview.error) : null;
  const heading = title ?? (refusal ? "The call that was refused" : "The call this will make");
  const missing = p ? missingWords(p.missing) : "";
  const input = request && showInput ? (shown ? shown(request.operation, request.input) : request.input) : undefined;
  const helpText = note && <div className="text-[13px] leading-5 text-muted-foreground">{note}</div>;
  const copy = p && request && <CopyCalls cli={p.cli} boto3={p.boto3} json={callJson(p, input)} format={format} waiting={preview.settling} />;
  return (
    <aside className={cn("min-w-0 rounded-[var(--radius-card)] border bg-card p-5", className)} aria-label={heading} data-slot="call-panel" data-refused={refusal ? "" : undefined} data-testid={testId}>
      <div className="flex items-center justify-between gap-3">
        <h2 className="flex items-center gap-2 text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">
          <BracesIcon aria-hidden className="size-3.5" /> {heading}
        </h2>
        {preview.settling && p && <span className="text-[11.5px] text-muted-foreground" data-testid={`${testId}-settling`}>Updating…</span>}
      </div>
      <div className="mt-3" aria-live="polite">
        {request === null ? (
          <p className="rounded-[var(--radius-field)] border border-dashed px-4 py-6 text-[13px] text-muted-foreground" data-testid={`${testId}-idle`}>
            The call appears here as soon as there is something to send.
          </p>
        ) : failed && !p ? (
          <div className="rounded-[var(--radius-field)] border border-bad/30 bg-bad/5 px-4 py-3 text-[13px]" data-testid={`${testId}-error`}>
            <p className="font-medium text-bright">The call could not be shown.</p>
            <p className="mt-0.5 text-foreground">{failed.message}</p>
            {failed.code && <p className="mt-1 font-mono text-[12px] text-muted-foreground">{failed.code}</p>}
          </div>
        ) : !p ? (
          <Skeleton className="h-44 rounded-xl" data-testid={`${testId}-loading`} />
        ) : (
          <CallCode text={callText(p, request.operation, { format, service: showService ? request.service : undefined, input, lists: listsOf(request.service) })} settling={preview.settling} testId={`${testId}-code`} />
        )}
      </div>
      {missing && (
        <p className="mt-2 text-[12.5px] text-warn" data-testid={`${testId}-missing`}>
          {missing}
        </p>
      )}
      {refusal && (
        <div className="mt-3 rounded-[var(--radius-field)] border border-bad/30 bg-bad/5 px-4 py-3 text-[13px]" data-testid={`${testId}-came-back`}>
          <p className="text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">What came back</p>
          <p className="mt-1 font-mono text-[12.5px] font-medium text-bad">{refusal.code}</p>
          <p className="mt-0.5 text-foreground">{refusal.message}</p>
        </div>
      )}
      {format === "outline" ? (
        <>
          {copy && <div className="mt-4">{copy}</div>}
          {helpText && <div className="mt-4">{helpText}</div>}
        </>
      ) : (
        <>
          {helpText && <div className="mt-3">{helpText}</div>}
          {copy && <div className="mt-3">{copy}</div>}
        </>
      )}
    </aside>
  );
}
