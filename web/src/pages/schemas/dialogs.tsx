// A subject's overlays, routed by `?dialog=` over its page: the compatibility level and mode, deleting the version on screen
// (soft, then for good), deleting the subject (soft; for good only with its name typed), and Try (a JSON value checked and
// encoded with the version, nothing sent). A refusal stays in the overlay in the registry's own code and words.
import { useNavigate } from "@tanstack/react-router";
import { SaveIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { useConfigure, useDeleteSchema, useTry } from "@/api/schemas";
import { useDebounced } from "@/api/topic-writes";
import type { SchemaWrite } from "@/api/types/SchemaWrite";
import type { SubjectResponse } from "@/api/types/SubjectResponse";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Textarea } from "@/components/ui/textarea";
import { ApiFailure } from "@/api/client";
import { Callout } from "../groups/parts";
import { SchemaBlock } from "./parts";
import type { SubjectDialog } from "./search";

type Refused = Extract<SchemaWrite, { outcome: "refused" }>;
const refusedOf = (w: SchemaWrite | undefined): Refused | null => (w?.outcome === "refused" ? w : null);

export function Refusal({ r }: { r: Refused }) {
  return (
    <Callout tone="refused" data-testid="schema-refused">
      <b className="font-semibold text-bright">
        Refused {r.status} · error_code {r.error_code}.
      </b>{" "}
      {r.message}
    </Callout>
  );
}

function CompatibilityDialog({ subject, d, onDone }: { subject: string; d: SubjectResponse; onDone: () => void }) {
  const [level, setLevel] = useState(d.row.level_is_own ? d.row.level : "default");
  const [mode, setMode] = useState(d.row.mode);
  const save = useConfigure(subject);
  const refused = refusedOf(save.data);
  return (
    <>
      <DialogHeader>
        <DialogTitle>Compatibility of {subject}</DialogTitle>
        <DialogDescription>Which earlier versions a new version is checked against, and whether the subject takes new versions at all. Nothing already registered is checked again.</DialogDescription>
      </DialogHeader>
      <div className="flex max-h-[min(65vh,40rem)] flex-col gap-4 overflow-y-auto pr-1">
        <div className="flex flex-col gap-2">
          <Label id="level-l">Level</Label>
          <RadioGroup value={level} onValueChange={setLevel} aria-labelledby="level-l" className="grid gap-1" data-testid="level-choice">
            {[{ name: "default", sentence: `None of its own: the global level, ${d.global_level}, applies.` }, ...d.levels].map((l) => (
              <label key={l.name} htmlFor={`lv-${l.name}`} className="flex cursor-pointer items-start gap-3 rounded-lg border border-transparent p-2 hover:bg-accent/50 has-[[data-state=checked]]:border-primary/30 has-[[data-state=checked]]:bg-accent/60">
                <RadioGroupItem value={l.name} id={`lv-${l.name}`} className="mt-0.5" />
                <span className="min-w-0">
                  <span className="block font-mono text-[13px] text-bright">{l.name === "default" ? "Global" : l.name}</span>
                  <span className="block text-[12px] text-muted-foreground">{l.sentence}</span>
                </span>
              </label>
            ))}
          </RadioGroup>
        </div>
        <div className="flex flex-col gap-2">
          <Label id="mode-l">Mode</Label>
          <RadioGroup value={mode} onValueChange={setMode} aria-labelledby="mode-l" className="flex gap-4" data-testid="mode-choice">
            {["READWRITE", "READONLY"].map((m) => (
              <label key={m} htmlFor={`md-${m}`} className="flex cursor-pointer items-center gap-2 font-mono text-[13px]">
                <RadioGroupItem value={m} id={`md-${m}`} /> {m}
              </label>
            ))}
          </RadioGroup>
          <p className="text-[12px] text-muted-foreground">READONLY refuses new versions and deletes (42205). IMPORT is not offered here.</p>
        </div>
        {refused && <Refusal r={refused} />}
      </div>
      <DialogFooter>
        <Button variant="outline" onClick={onDone} data-testid="dialog-cancel">
          Cancel
        </Button>
        <Button
          disabled={save.isPending}
          data-testid="save-compatibility"
          onClick={() =>
            save.mutate(
              { level, mode },
              {
                onSuccess: (w) => {
                  if (w.outcome !== "configured") return;
                  toast.success(`${subject}: ${w.level}, ${w.mode}.`);
                  onDone();
                },
              },
            )
          }
        >
          <SaveIcon /> Save
        </Button>
      </DialogFooter>
    </>
  );
}

function DeleteVersionDialog({ subject, d, onDone }: { subject: string; d: SubjectResponse; onDone: () => void }) {
  const s = d.shown;
  const permanent = s.deleted;
  const del = useDeleteSchema(subject);
  const refused = refusedOf(del.data);
  return (
    <>
      <DialogHeader>
        <DialogTitle>{permanent ? `Delete version ${s.version} of ${subject} for good?` : `Soft delete version ${s.version} of ${subject}?`}</DialogTitle>
        <DialogDescription>
          {permanent
            ? `It is already soft-deleted. Deleting it for good removes the version; id ${s.id} stays reserved and is never given again.`
            : `Clients can no longer look it up by version, and it is not compared with new versions. Its id ${s.id} still resolves at /schemas/ids/${s.id}, so records written with it still decode. It can be deleted for good afterwards.`}
        </DialogDescription>
      </DialogHeader>
      {refused && <Refusal r={refused} />}
      <DialogFooter>
        <Button variant="outline" onClick={onDone} data-testid="dialog-cancel">
          {refused ? "Close" : "Cancel"}
        </Button>
        <Button
          variant="destructive"
          disabled={del.isPending}
          data-testid="delete-version-submit"
          onClick={() =>
            del.mutate(
              { version: s.version, permanent, confirm: null },
              {
                onSuccess: (w) => {
                  if (w.outcome !== "deleted") return;
                  toast.success(permanent ? `Version ${s.version} deleted for good.` : `Version ${s.version} soft-deleted; its id still resolves.`);
                  onDone();
                },
              },
            )
          }
        >
          <Trash2Icon /> {permanent ? "Delete for good" : `Soft delete v${s.version}`}
        </Button>
      </DialogFooter>
    </>
  );
}

function DeleteSubjectDialog({ subject, d, onDone }: { subject: string; d: SubjectResponse; onDone: () => void }) {
  const navigate = useNavigate();
  const permanent = d.row.deleted;
  const [typed, setTyped] = useState("");
  const del = useDeleteSchema(subject);
  const refused = refusedOf(del.data);
  const why = del.error instanceof ApiFailure ? del.error.message : null;
  return (
    <>
      <DialogHeader>
        <DialogTitle>{permanent ? `Delete ${subject} for good?` : `Soft delete ${subject}?`}</DialogTitle>
        <DialogDescription>
          {permanent
            ? "Every version is already soft-deleted. Deleting for good removes them; the ids stay reserved and are never given again. Records on topics are not touched: they show as raw bytes once no subject holds their id (40403)."
            : `All ${d.row.versions} versions are soft-deleted: clients stop finding the subject, and its own level and mode go with it. Their ids still resolve. Deleting for good is a second step, as in Confluent's registry.`}
        </DialogDescription>
      </DialogHeader>
      {permanent && (
        <div className="flex flex-col gap-2">
          <Label htmlFor="confirm-subject">Type the subject name to confirm</Label>
          <Input id="confirm-subject" value={typed} onChange={(e) => setTyped(e.target.value)} className="font-mono" autoComplete="off" spellCheck={false} data-testid="confirm-subject" />
        </div>
      )}
      {refused && <Refusal r={refused} />}
      {why && <p className="text-[13px] text-bad">{why}</p>}
      <DialogFooter>
        <Button variant="outline" onClick={onDone} data-testid="dialog-cancel">
          {refused ? "Close" : "Cancel"}
        </Button>
        <Button
          variant="destructive"
          disabled={del.isPending || (permanent && typed !== subject)}
          data-testid="delete-subject-submit"
          onClick={() =>
            del.mutate(
              { version: null, permanent, confirm: permanent ? typed : null },
              {
                onSuccess: (w) => {
                  if (w.outcome !== "deleted") return;
                  if (permanent) {
                    toast.success(`${subject} deleted for good.`);
                    void navigate({ to: "/schemas", replace: true });
                  } else {
                    toast.success(`${subject} soft-deleted: listed under soft-deleted.`);
                    onDone();
                  }
                },
              },
            )
          }
        >
          <Trash2Icon /> {permanent ? "Delete subject for good" : "Soft delete subject"}
        </Button>
      </DialogFooter>
    </>
  );
}

/** Try: a value typed as JSON, checked and encoded with the version on screen as a serializer would; nothing is sent. */
function TryBody({ subject, d }: { subject: string; d: SubjectResponse }) {
  const [value, setValue] = useState(d.shown.sample);
  const asked = useDebounced(value, 300);
  const t = useTry({ subject, version: String(d.shown.version), value: asked });
  const e = t.data;
  return (
    <div className="flex flex-col gap-4 px-6 pb-8">
      <div className="flex flex-col gap-2">
        <Label htmlFor="try-value">Value, as JSON</Label>
        <Textarea id="try-value" value={value} onChange={(x) => setValue(x.target.value)} rows={10} spellCheck={false} className="max-h-80 font-mono text-[12.5px]" data-testid="try-value" />
        <p className="text-[12.5px] text-muted-foreground">
          {d.shown.kind === "AVRO" ? "Avro's JSON: a union value may be tagged with its branch ({\"string\": \"x\"}); bytes are text or base64." : d.shown.kind === "PROTOBUF" ? "protojson: fields by their JSON names, 64-bit numbers as strings, bytes as base64. Encoded as the first message." : "Checked against the schema, then sent as the JSON text."}
        </p>
      </div>
      <div className="min-h-40" aria-live="polite" data-testid="try-result">
        {e && e.ok && (
          <div className="flex flex-col gap-3">
            <Callout tone="ok">
              <b className="font-semibold text-bright">Fits v{e.version}.</b> {e.size} bytes: magic byte 0, id {e.id} (4 bytes), then the value.
            </Callout>
            <pre className="overflow-x-auto rounded-lg border bg-well p-3 font-mono text-[12px] leading-5 break-all whitespace-pre-wrap text-bright" data-testid="try-hex">
              {e.hex}
            </pre>
            {e.json && <SchemaBlock text={e.json} label="Copy value" />}
          </div>
        )}
        {e && !e.ok && (
          <Callout tone="refused">
            <b className="font-semibold text-bright">Does not fit v{e.version || d.shown.version}.</b> <span data-testid="try-error">{e.error}</span>
          </Callout>
        )}
      </div>
    </div>
  );
}

export function SubjectDialogs({ subject, d, dialog, onClose }: { subject: string; d: SubjectResponse; dialog?: SubjectDialog; onClose: () => void }) {
  const isDialog = dialog === "compatibility" || dialog === "delete-version" || dialog === "delete-subject";
  return (
    <>
      <Dialog open={isDialog} onOpenChange={(o) => !o && onClose()}>
        <DialogContent className="sm:max-w-[600px]" data-testid={isDialog ? `dialog-${dialog}` : undefined} aria-describedby={undefined}>
          {dialog === "compatibility" && <CompatibilityDialog subject={subject} d={d} onDone={onClose} />}
          {dialog === "delete-version" && <DeleteVersionDialog subject={subject} d={d} onDone={onClose} />}
          {dialog === "delete-subject" && <DeleteSubjectDialog subject={subject} d={d} onDone={onClose} />}
        </DialogContent>
      </Dialog>
      <Sheet open={dialog === "try"} onOpenChange={(o) => !o && onClose()}>
        <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-2xl" data-testid="try-sheet">
          <SheetHeader className="px-6 pt-6 pb-4">
            <SheetTitle className="font-mono text-[17px] break-all">
              Try {subject} v{d.shown.version}
            </SheetTitle>
            <SheetDescription>A value checked and encoded as a serializer would. Nothing is sent or registered.</SheetDescription>
          </SheetHeader>
          {dialog === "try" && <TryBody subject={subject} d={d} />}
        </SheetContent>
      </Sheet>
    </>
  );
}
