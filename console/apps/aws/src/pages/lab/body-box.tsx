import { Label } from "@doze/ui/components/ui/label";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { number } from "@doze/ui/lib/format";

/** How many bytes `text` is when it is sent (UTF-8): what the server counts against the most a body may be. */
export const bytesOf = (text: string): number => new TextEncoder().encode(text).length;

/** The input a call is sent with: the editor's input, and the body as its `Body` member when there is one (an empty box sends none, so the editor's own `Body` stands). */
export function withBody(input: Record<string, unknown>, body: string): Record<string, unknown> {
  return body === "" ? input : { ...input, Body: body };
}

/** An input taken apart for the editor and the body box: a text `Body` goes to the box, so the editor shows the rest. */
export function splitBody(input: Record<string, unknown>): { input: Record<string, unknown>; body: string } {
  const { Body, ...rest } = input;
  return typeof Body === "string" ? { input: rest, body: Body } : { input, body: "" };
}

/** The body of an object being put: text only, at most `max` bytes, sent as the object's bytes the way the bucket's page uploads them. */
export function BodyBox({ value, onChange, max }: { value: string; onChange: (v: string) => void; max: number }) {
  const used = bytesOf(value);
  const over = used > max;
  return (
    <div className="grid gap-1.5" data-testid="lab-body">
      <div className="flex items-center justify-between gap-3">
        <Label htmlFor="lab-body-text">Body</Label>
        <span className={over ? "text-[12.5px] text-destructive" : "text-[12.5px] text-muted-foreground"} data-testid="lab-body-size">{number(used)} of {number(max)} bytes</span>
      </div>
      <Textarea id="lab-body-text" value={value} onChange={(e) => onChange(e.target.value)} spellCheck={false} rows={5} placeholder="The text the object holds" className="bg-card font-mono text-[12.5px] leading-5" aria-invalid={over} data-testid="lab-body-input" />
      <p className={over ? "text-[12.5px] text-destructive" : "text-[12.5px] text-muted-foreground"} data-testid="lab-body-help">
        {over ? `The body is over ${number(max)} bytes (${max / 1024} KiB): upload larger objects on the bucket's page, or with the AWS CLI.` : `Text only, at most ${max / 1024} KiB. It is the object's bytes; the length is set for you. Left empty, the object is empty.`}
      </p>
    </div>
  );
}
