import { BookmarkIcon, FolderOpenIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Section } from "../card";
import { newId, summarise } from "./history";
import { SAVED_KEY, readKept, useKept, writeKept } from "./kept";
import { MOST_NAME, MOST_SAVED, parseSaved, type Saved, saveInput } from "./saved";

/** The row under the editor that keeps the input as it is under a name. */
export function SaveInput({ ask }: { ask: { target: string; service: string; title: string; op: string; input: Record<string, unknown> } | null }) {
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  const [why, setWhy] = useState<string | null>(null);
  const save = () => {
    if (!ask) return;
    const now = Date.now();
    const done = saveInput(readKept(SAVED_KEY, parseSaved), { ...ask, id: newId(now), name, at: now });
    if (!done.ok) return setWhy(done.why);
    writeKept(SAVED_KEY, done.list);
    toast.success(`Saved as ${name.trim()}`);
    setNaming(false);
    setName("");
    setWhy(null);
  };
  if (!naming) {
    return (
      <Button type="button" variant="outline" className="bg-card" disabled={!ask} onClick={() => setNaming(true)} data-testid="lab-save-open">
        <BookmarkIcon /> Save input
      </Button>
    );
  }
  return (
    <div className="flex flex-wrap items-center gap-2" data-testid="lab-save-bar">
      <Input
        autoFocus
        value={name}
        maxLength={MOST_NAME}
        onChange={(e) => (setName(e.target.value), setWhy(null))}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            save();
          } else if (e.key === "Escape") {
            e.preventDefault();
            setNaming(false);
          }
        }}
        placeholder="A name for this input"
        aria-label="Name for this input"
        aria-invalid={why !== null}
        className="h-9 w-56 bg-card"
        data-testid="lab-save-name"
      />
      <Button type="button" onClick={save} data-testid="lab-save-confirm">
        Save
      </Button>
      <Button type="button" variant="ghost" onClick={() => (setNaming(false), setWhy(null))}>
        Cancel
      </Button>
      {why && <p className="basis-full text-[12.5px] text-destructive" role="alert" data-testid="lab-save-why">{why}</p>}
    </div>
  );
}

/** The inputs saved under a name, for the region chosen. Open puts one in the editor. */
export function SavedList({ onOpen }: { onOpen: (s: Saved) => void }) {
  const list = useKept(SAVED_KEY, parseSaved);
  return (
    <Section title="Saved inputs" testId="lab-saved" lede={`Up to ${MOST_SAVED}, kept in this browser (not in doze-aws), for the region you are looking at.`}>
      {list.length === 0 ? (
        <p className="rounded-[var(--radius-card)] border border-dashed bg-card px-6 py-8 text-center text-[13.5px] text-muted-foreground" data-testid="lab-saved-empty">
          Nothing saved. Use Save input under the editor to keep one under a name.
        </p>
      ) : (
        <ul className="divide-y rounded-[var(--radius-card)] border bg-card">
          {list.map((s) => (
            <li key={s.id} className="flex min-w-0 items-center gap-3 px-5 py-3.5" data-testid="lab-saved-row" data-name={s.name}>
              <div className="min-w-0 flex-1">
                <div className="truncate text-[14px] font-medium text-bright" title={s.name}>{s.name}</div>
                <p className="truncate text-[12.5px] text-muted-foreground" title={summarise(s.input, 400)}>
                  {s.title} {s.op} · <span className="font-mono">{summarise(s.input, 60)}</span>
                </p>
              </div>
              <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => onOpen(s)} data-testid="lab-saved-open">
                <FolderOpenIcon /> Open
              </Button>
              <Button type="button" variant="ghost" size="icon-sm" aria-label={`Delete ${s.name}`} title="Delete it" onClick={() => writeKept(SAVED_KEY, readKept(SAVED_KEY, parseSaved).filter((x) => x.id !== s.id))} data-testid="lab-saved-delete">
                <Trash2Icon />
              </Button>
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}
