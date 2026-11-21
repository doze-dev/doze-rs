// A list of values a person adds to one at a time (a CORS configuration's origins, headers and methods, an authorizer's identity sources): the
// field, Add, and what has been added as chips that can be taken off.
import { PlusIcon, XIcon } from "lucide-react";
import { useState } from "react";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";

export function ListEditor({ values, onChange, placeholder, label, testId, mono = true }: { values: readonly string[]; onChange: (v: string[]) => void; placeholder?: string; label: string; testId: string; mono?: boolean }) {
  const [typed, setTyped] = useState("");
  const add = () => {
    const v = typed.trim();
    if (v !== "" && !values.includes(v)) onChange([...values, v]);
    setTyped("");
  };
  return (
    <div className="grid gap-2.5" data-testid={testId}>
      <div className="flex items-center gap-2">
        <Input value={typed} onChange={(e) => setTyped(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); add(); } }} placeholder={placeholder} aria-label={label} autoComplete="off" spellCheck={false} className={mono ? "font-mono" : undefined} data-testid={`${testId}-input`} />
        <Button type="button" variant="outline" className="bg-card" onClick={add} data-testid={`${testId}-add`}><PlusIcon /> Add</Button>
      </div>
      {values.length > 0 && (
        <ul className="flex flex-wrap gap-2" aria-label={`${label}, added`}>
          {values.map((v) => (
            <li key={v} className="inline-flex items-center gap-1.5 rounded-lg border bg-well py-1 pr-1 pl-3 text-[13px]" data-testid={`${testId}-item`}>
              <span className={mono ? "font-mono" : undefined}>{v}</span>
              <button type="button" aria-label={`Remove ${v}`} className="grid size-6 place-content-center rounded-md text-muted-foreground hover:bg-accent hover:text-bright focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" onClick={() => onChange(values.filter((x) => x !== v))}><XIcon className="size-3.5" /></button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
