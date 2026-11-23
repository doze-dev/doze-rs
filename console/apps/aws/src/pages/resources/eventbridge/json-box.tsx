// A JSON document typed by a person, as the boards draw it: monospace, checked as it is typed, with Copy and Prettify under it (and what else a
// page puts in `extra`).
import { CopyIcon } from "lucide-react";
import { useId } from "react";
import { copyText } from "@doze/ui/components/app/copy";
import { checkJson } from "@doze/ui/components/app/json-editor";
import { Button } from "@doze/ui/components/ui/button";
import { Label } from "@doze/ui/components/ui/label";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { cn } from "@doze/ui/lib/utils";
import { prettyJson } from "./words";

export function JsonBox({ value, onChange, label, rows = 10, empty = "ok", extra, hideLabel = false, placeholder, testId = "json-box", className }: { value: string; onChange: (v: string) => void; label: string; rows?: number; empty?: "ok" | "invalid"; extra?: React.ReactNode; hideLabel?: boolean; placeholder?: string; testId?: string; className?: string }) {
  const id = useId();
  const check = checkJson(value, empty);
  return (
    <div className={cn("grid gap-1.5", className)}>
      <Label htmlFor={id} className={hideLabel ? "sr-only" : undefined}>{label}</Label>
      <Textarea id={id} value={value} onChange={(e) => onChange(e.target.value)} rows={rows} placeholder={placeholder} spellCheck={false} autoComplete="off" aria-invalid={!check.ok} className="resize-y bg-card font-mono text-[13px] leading-5" data-testid={testId} />
      {!check.ok && <p className="text-[13px] text-bad" role="alert" data-testid={`${testId}-problem`}>Not JSON: {check.why}</p>}
      <div className="mt-1 flex flex-wrap items-center gap-2">
        <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => void copyText(value, "Copied")} data-testid={`${testId}-copy`}><CopyIcon /> Copy</Button>
        <Button type="button" variant="outline" size="sm" className="bg-card" disabled={!check.ok || value.trim() === ""} onClick={() => onChange(prettyJson(value))} data-testid={`${testId}-prettify`}>Prettify</Button>
        {extra}
      </div>
    </div>
  );
}
