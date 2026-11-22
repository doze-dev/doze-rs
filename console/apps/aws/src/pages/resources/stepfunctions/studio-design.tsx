// Workflow Studio's Design mode (stepfunctions__machine-create-design): the states browser (Actions, Flow, Patterns), the canvas (the graph,
// with a drop zone on every edge), and the inspector (the properties of the chosen state). A state is dragged from the browser onto an edge,
// or chosen in the browser and then put on an edge by a click; every change rewrites the definition text, which Code mode shows and the
// studio checks. design-model.ts holds what a drop, a rename and a delete do to the definition.
import { FilterIcon, GripVerticalIcon, MoveIcon, Redo2Icon, TrashIcon, Undo2Icon } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Callout } from "@doze/ui/components/app/callout";
import { JsonEditor, checkJson } from "@doze/ui/components/app/json-editor";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Switch } from "@doze/ui/components/ui/switch";
import { cn } from "@doze/ui/lib/utils";
import { Segmented } from "../../../kit";
import { CodeBlock } from "../deep/parts";
import { ACTIONS, FLOW, NO_HISTORY, PATTERNS, type EdgeRef, type History, type Offer, changed, deleteState, insertOnEdge, moveState, offerOf, redo, renameState, setState, stateOf, undo } from "./design-model";
import { type AslState, type Definition, parseDefinition } from "./graph";
import { GraphView } from "./graph-view";
import { useAbout } from "../../../api/queries";
import { useWhere } from "./hooks";
import { pretty, taskResource, taskWords } from "./model";

type Tab = "actions" | "flow" | "patterns";

const NOTE = (
  <p className="border-l-2 border-input pl-3.5 text-[13.5px] leading-[22px] text-foreground" data-testid="browser-note">
    The Actions tab lists only what doze-aws can run: Lambda, SQS, SNS, EventBridge, DynamoDB, S3, Step Functions and the other services it serves.{" "}
    <span className="text-muted-foreground">Not available in doze-aws: the .sync pattern for any service but Step Functions (Batch, ECS, Glue and the rest would run a job nowhere); Create accepts a definition that uses one, as AWS does, and the step fails when it runs: States.TaskFailed, which a Catch can handle.</span>
  </p>
);

function Chip({ o, armed, onArm, onDrag }: { o: Offer; armed: boolean; onArm: () => void; onDrag: (on: boolean) => void }) {
  return (
    <div
      role="button"
      tabIndex={0}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData("text/plain", o.id);
        e.dataTransfer.effectAllowed = "copy";
        onDrag(true);
      }}
      onDragEnd={() => onDrag(false)}
      onClick={onArm}
      onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onArm(); } }}
      aria-pressed={armed}
      title={o.hint ?? `Drag ${o.label} onto the canvas, or choose it and then choose where it goes`}
      className={cn("inline-flex cursor-grab items-center gap-1.5 rounded-full border bg-card px-3 py-1 font-mono text-[12.5px] text-bright outline-none hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50 active:cursor-grabbing", armed && "border-primary bg-pick")}
      data-testid={`offer-${o.id}`}
    >
      <GripVerticalIcon aria-hidden className="size-3 text-muted-foreground" />
      {o.label}
    </div>
  );
}

export function DesignMode({ text, setText }: { text: string; setText: (t: string) => void; definition: Definition | null; valid: boolean; pending: boolean }) {
  const [tab, setTab] = useState<Tab>("flow");
  const [armed, setArmed] = useState<string | null>(null);
  const [dragged, setDragged] = useState(false);
  const [selected, setSelected] = useState<string | undefined>();
  const [problem, setProblem] = useState<string | null>(null);
  const where = useWhere();
  const about = useAbout().data;
  const ctx = { ...where, endpoint: about?.endpoint ?? "http://127.0.0.1:4566" };
  const def = useMemo(() => parseDefinition(text), [text]);
  const empty = !def || Object.keys(def.States ?? {}).length === 0;
  // Undo and redo of what Design mode wrote. A change made elsewhere (Code mode) is not Design's to take back: the history starts again.
  const [hist, setHist] = useState<History>(NO_HISTORY);
  const mine = useRef(text);
  useEffect(() => {
    if (text !== mine.current) {
      mine.current = text;
      setHist(NO_HISTORY);
    }
  }, [text]);
  const show = (next: string) => {
    mine.current = next;
    setText(next);
  };
  const write = (next: Definition | null, refused: string) => {
    if (!next) {
      setProblem(refused);
      return false;
    }
    setProblem(null);
    const written = pretty(next);
    setHist((h) => changed(h, text, written));
    show(written);
    return true;
  };
  const back = () => {
    const r = undo(hist, text);
    if (!r) return;
    setHist(r.history);
    setProblem(null);
    show(r.text);
  };
  const forward = () => {
    const r = redo(hist, text);
    if (!r) return;
    setHist(r.history);
    setProblem(null);
    show(r.text);
  };
  const drop = (edge: EdgeRef, id: string) => {
    if (id.startsWith("move:") && def) {
      const name = id.slice(5);
      if (!write(moveState(def, name, edge), `${name} is already there: choose another edge, one that is not next to it.`)) return;
      setSelected(name);
      setArmed(null);
      setDragged(false);
      return;
    }
    const offer = offerOf(id);
    if (!offer || !def) return;
    const r = insertOnEdge(def, edge, offer, ctx);
    if (!r) {
      write(null, `${offer.label} ends a path, so it goes only at the end of one: drop it on the edge into End.`);
      return;
    }
    write(r.def, "");
    setSelected(r.name);
    setArmed(null);
    setDragged(false);
  };
  const dropFirst = (id: string) => drop({ scope: "", from: null, to: null, kind: "start", nth: 0 }, id);
  const state = selected && def ? stateOf(def, selected) : undefined;
  const dragging = armed !== null || dragged;
  const armedLabel = armed?.startsWith("move:") ? armed.slice(5) : (armed ? offerOf(armed)?.label : undefined);
  const keys = (e: React.KeyboardEvent) => {
    // Outside a text field, Ctrl or Cmd with Z takes the last change back, with Shift puts it back; a field keeps its own undo.
    const field = (e.target as HTMLElement).closest("input, textarea, select, [contenteditable]");
    if (field || !(e.metaKey || e.ctrlKey) || e.key.toLowerCase() !== "z") return;
    e.preventDefault();
    if (e.shiftKey) forward();
    else back();
  };
  const offers = tab === "flow" ? FLOW : tab === "actions" ? ACTIONS : PATTERNS;
  return (
    <div className="grid gap-6" data-testid="studio-design" onKeyDown={keys}>
      <section className="rounded-[14px] border bg-card p-6 lg:p-7" data-testid="states-browser">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">States browser</h2>
            <p className="mt-1 text-[13.5px] text-muted-foreground">{tab === "patterns" ? "Drag a pattern onto the canvas. A pattern brings the states that go together." : tab === "actions" ? "Drag a task onto the canvas. Each calls one operation of a service." : "Drag a state onto the canvas. Flow states control the order of steps."}</p>
          </div>
          <Segmented value={tab} onChange={setTab} options={[{ value: "actions", label: "Actions" }, { value: "flow", label: "Flow" }, { value: "patterns", label: "Patterns" }]} label="States browser" testId="browser-tabs" />
        </div>
        <div className="mt-5 flex flex-wrap gap-2" data-testid="browser-offers">
          {offers.map((o) => <Chip key={o.id} o={o} armed={armed === o.id} onArm={() => setArmed(armed === o.id ? null : o.id)} onDrag={setDragged} />)}
        </div>
        {tab === "actions" && <p className="mt-3 flex items-center gap-1.5 text-[13px] text-muted-foreground"><FilterIcon aria-hidden className="size-3.5" />Task states built for the services doze-aws serves.</p>}
        <div className="mt-5">{NOTE}</div>
      </section>
      {armed && <Callout tone="info" testId="armed-note">Choose where <b className="font-semibold">{armedLabel}</b> {armed.startsWith("move:") ? "moves to" : "goes"}: one of the <span className="font-mono">+</span> marks on the canvas. <button type="button" className="font-medium text-link hover:underline" onClick={() => setArmed(null)}>Cancel</button></Callout>}
      {problem && <Callout tone="warn" testId="design-problem">{problem}</Callout>}
      <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_22rem]">
        <section className="min-w-0 rounded-[14px] border bg-card p-6 lg:p-7" data-testid="canvas">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Canvas</h2>
              <p className="mt-1 text-[13.5px] text-muted-foreground">The workflow graph updates the definition as you edit it.</p>
            </div>
            <div className="flex items-center gap-2">
              <Button type="button" variant="outline" size="sm" className="bg-card" disabled={hist.past.length === 0} onClick={back} aria-keyshortcuts="Control+Z Meta+Z" data-testid="design-undo"><Undo2Icon /> Undo</Button>
              <Button type="button" variant="outline" size="sm" className="bg-card" disabled={hist.future.length === 0} onClick={forward} aria-keyshortcuts="Control+Shift+Z Meta+Shift+Z" data-testid="design-redo"><Redo2Icon /> Redo</Button>
            </div>
          </div>
          <div className="mt-5">
            {!def ? (
              <Callout tone="bad" testId="design-unreadable">The definition is not JSON, so there is no graph to draw. Fix it in Code mode.</Callout>
            ) : empty ? (
              <div
                className="flex min-h-48 items-center justify-center rounded-[12px] border border-dashed text-[13.5px] text-muted-foreground"
                onDragOver={(e) => e.preventDefault()}
                onDrop={(e) => { e.preventDefault(); const id = e.dataTransfer.getData("text/plain"); if (id) dropFirst(id); }}
                onClick={() => armed && dropFirst(armed)}
                data-testid="canvas-empty"
              >
                Drag the first state here.
              </div>
            ) : (
              <GraphView definition={def} selected={selected} onSelect={setSelected} design={{ active: dragging, armed, onDrop: drop }} />
            )}
          </div>
        </section>
        <Inspector def={def} name={selected} state={state} onChange={(next, refused) => write(next, refused)} onRename={(n) => setSelected(n)} onDelete={() => setSelected(undefined)} onMove={(n) => setArmed(`move:${n}`)} />
      </div>
    </div>
  );
}

function Inspector({ def, name, state, onChange, onRename, onDelete, onMove }: { def: Definition | null; name: string | undefined; state: AslState | undefined; onChange: (d: Definition | null, refused: string) => void; onRename: (name: string) => void; onDelete: () => void; onMove: (name: string) => void }) {
  const [showDefinition, setShowDefinition] = useState(false);
  return (
    <section className="min-w-0 rounded-[14px] border bg-card p-6 lg:p-7" data-testid="inspector">
      <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Inspector</h2>
      {!def || !name || !state ? (
        <p className="mt-1 text-[13.5px] text-muted-foreground" data-testid="inspector-none">Properties of the selected state. Choose a state on the canvas.</p>
      ) : (
        <div className="mt-1 grid gap-5" key={name}>
          <p className="text-[13.5px] text-muted-foreground">Properties of the selected state, <span className="font-mono text-foreground" data-testid="inspector-name">{name}</span>.</p>
          <Fields def={def} name={name} state={state} onChange={onChange} onRename={onRename} />
          <div className="flex items-center gap-3">
            <Switch id="inspector-definition" checked={showDefinition} onCheckedChange={setShowDefinition} data-testid="inspector-definition" />
            <Label htmlFor="inspector-definition">Definition</Label>
          </div>
          {showDefinition && <CodeBlock text={JSON.stringify(state, null, 2)} max="max-h-72" testId="inspector-json" />}
          {state.Type !== "Choice" && <Button type="button" variant="outline" className="w-fit bg-card" onClick={() => onMove(name)} data-testid="inspector-move"><MoveIcon /> Move state</Button>}
          <Button type="button" variant="outline" className="w-fit border-bad/40 bg-card text-bad hover:text-bad" onClick={() => { if (deleteState(def, name)) { onChange(deleteState(def, name), ""); onDelete(); } }} data-testid="inspector-delete"><TrashIcon /> Delete state</Button>
        </div>
      )}
    </section>
  );
}

/** What the chosen state's type has to set. */
function Fields({ def, name, state, onChange, onRename }: { def: Definition; name: string; state: AslState; onChange: (d: Definition | null, refused: string) => void; onRename: (name: string) => void }) {
  const [rename, setRename] = useState(name);
  const taken = rename !== name && renameState(def, name, rename) === null;
  const replace = (body: AslState) => onChange(setState(def, name, body), "The state could not be changed.");
  const t = taskResource(state.Resource);
  return (
    <div className="grid gap-5">
      <div className="grid gap-1.5">
        <Label htmlFor="inspector-rename">Name</Label>
        <Input
          id="inspector-rename"
          value={rename}
          onChange={(e) => setRename(e.target.value)}
          onBlur={() => {
            if (rename === name) return;
            const next = renameState(def, name, rename);
            if (next) {
              onChange(next, "");
              onRename(rename);
            } else setRename(name);
          }}
          onKeyDown={(e) => e.key === "Enter" && (e.currentTarget.blur())}
          aria-invalid={taken}
          className="bg-card font-mono"
          autoComplete="off"
          spellCheck={false}
          data-testid="inspector-rename"
        />
        {taken && <p className="text-[12.5px] text-bad">{rename.trim() === "" ? "A state needs a name." : `${rename} is the name of another state.`}</p>}
      </div>
      {state.Type === "Task" && <TaskFields state={state} replace={replace} />}
      {state.Type === "Wait" && (
        <div className="grid gap-1.5">
          <Label htmlFor="inspector-seconds">Seconds</Label>
          <Input id="inspector-seconds" inputMode="numeric" value={String(state.Seconds ?? "")} onChange={(e) => { const n = Number(e.target.value); if (e.target.value !== "" && Number.isInteger(n) && n >= 0) replace({ ...state, Seconds: n }); }} className="max-w-32 bg-card" data-testid="inspector-seconds" />
        </div>
      )}
      {state.Type === "Pass" && <JsonField label="Result" value={state.Result} onValid={(v) => { const next = { ...state }; if (v === undefined) delete next.Result; else next.Result = v; replace(next); }} testId="inspector-result" help="What the state gives instead of its input. Leave it empty to pass the input on." />}
      {state.Type === "Fail" && (
        <>
          <div className="grid gap-1.5"><Label htmlFor="inspector-error">Error</Label><Input id="inspector-error" value={String(state.Error ?? "")} onChange={(e) => replace({ ...state, Error: e.target.value })} className="bg-card font-mono" data-testid="inspector-error" /></div>
          <div className="grid gap-1.5"><Label htmlFor="inspector-cause">Cause</Label><Input id="inspector-cause" value={String(state.Cause ?? "")} onChange={(e) => replace({ ...state, Cause: e.target.value })} className="bg-card" data-testid="inspector-cause" /></div>
        </>
      )}
      {state.Type === "Choice" && (
        <>
          <JsonField label="Choices" value={state.Choices} required onValid={(v) => replace({ ...state, Choices: v as AslState["Choices"] })} testId="inspector-choices" help="Each rule is a comparison of a Variable and the Next state it chooses." />
          <div className="grid gap-1.5">
            <Label>Default</Label>
            <select value={state.Default ?? ""} onChange={(e) => replace({ ...state, Default: e.target.value })} className="h-9 rounded-md border bg-card px-3 font-mono text-[13px]" aria-label="Default" data-testid="inspector-default">
              {Object.keys(def.States ?? {}).filter((n) => n !== name).map((n) => <option key={n} value={n}>{n}</option>)}
            </select>
          </div>
        </>
      )}
      {(state.Type === "Parallel" || state.Type === "Map") && (
        <p className="text-[13px] text-muted-foreground">{state.Type === "Parallel" ? `${state.Branches?.length ?? 0} branches run at once. Drop states inside a branch on its own edges.` : "Its item processor runs once for each item. Drop states inside it on its own edges."}</p>
      )}
      {t && !t.activity && state.Type === "Task" && <p className="sr-only">{taskWords(state)}</p>}
    </div>
  );
}

function TaskFields({ state, replace }: { state: AslState; replace: (s: AslState) => void }) {
  const t = taskResource(state.Resource);
  return (
    <JsonField
      label="API Parameters"
      value={state.Parameters}
      onValid={(v) => {
        const next = { ...state };
        if (v === undefined) delete next.Parameters;
        else next.Parameters = v as Record<string, unknown>;
        replace(next);
      }}
      testId="inspector-parameters"
      help={
        <>
          Becomes <span className="font-mono text-foreground">Resource</span> <span className="font-mono text-foreground break-all">{state.Resource}</span> and its <span className="font-mono text-foreground">Parameters</span>.{t && !t.activity ? <span className="text-muted-foreground"> {taskWords(state)} is a call doze-aws serves.</span> : null}
        </>
      }
    />
  );
}

/** A JSON value edited as text: the definition takes it once it parses (a part that does not is kept as typed). */
function JsonField({ label, value, onValid, testId, help, required = false }: { label: string; value: unknown; onValid: (v: unknown) => void; testId: string; help?: React.ReactNode; required?: boolean }) {
  const [text, setText] = useState(value === undefined ? "" : JSON.stringify(value, null, 2));
  return (
    <JsonEditor
      label={label}
      value={text}
      onChange={(v) => {
        setText(v);
        const c = checkJson(v, required ? "invalid" : "ok");
        if (c.ok) onValid(c.value);
      }}
      empty={required ? "invalid" : "ok"}
      rows={7}
      help={help}
      testId={testId}
    />
  );
}
