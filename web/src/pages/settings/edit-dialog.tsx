import { CheckCheckIcon, CircleCheckIcon, SaveIcon } from "lucide-react";
import { useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { ApiFailure } from "@/api/client";
import { post, useSettingsEdit } from "@/api/r4";
import type { SettingsEditView } from "@/api/types/SettingsEditView";
import type { SettingsLine } from "@/api/types/SettingsLine";
import type { SettingsOutcome } from "@/api/types/SettingsOutcome";
import type { SettingsRefused } from "@/api/types/SettingsRefused";
import type { SettingsSaved } from "@/api/types/SettingsSaved";
import { Spans } from "@/components/app/spans";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Form, FormControl, FormField, FormItem, FormLabel, FormMessage } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { duration, number } from "@/lib/format";
import { DYNAMIC_KEYS } from "@/lib/search";
import { bytes, CallBox, RefusalPanel } from "../r4/bits";

/** The unit hint beside the value, worked out as the person types (the server's `unit` says the same for the value it was sent). */
export function unitHint(key: string, typed: string): string | null {
  const t = typed.trim();
  if (!/^-?\d+$/.test(t)) return null;
  const n = Number(t);
  const ms = key.endsWith(".ms");
  if (n === -1 && (key === "log.retention.ms" || key === "log.retention.bytes")) return `${ms ? "ms" : "B"} = no limit`;
  if (n < 0) return null;
  return ms ? `ms = ${duration(n)}` : `B = ${n < 1024 ? `${number(n)} B` : bytes(n)}`;
}

function Line({ label, line }: { label: string; line: SettingsLine }) {
  if (line.text.length === 0 && !line.dim) return null;
  return (
    <p className="border-l-2 pl-3 text-[13px] leading-5">
      <b className="font-semibold text-foreground">{label}: </b>
      <Spans spans={line.text} />
      {/* The server gives the line and its dim sentence apart; they read as two sentences. */}
      {line.dim && !/[.:;!?]\s*$/.test(line.text.map((s) => s.text).join("")) ? "." : ""}{" "}
      {line.dim && <span className="text-muted-foreground">{line.dim}</span>}
    </p>
  );
}

function Refused({ r }: { r: SettingsRefused }) {
  return (
    <div className="flex flex-col gap-3" data-testid="settings-refused">
      <RefusalPanel refusal={{ ...r.refusal, rule: r.rule.length ? r.rule : r.refusal.rule, would_pass: r.would_pass.length ? r.would_pass : r.refusal.would_pass }} />
      {r.flag && (
        <p className="text-[12.5px] text-muted-foreground">
          The flag that sets it at start: <code className="font-mono text-foreground">{r.flag}</code>.
        </p>
      )}
      <CallBox lines={r.call} />
    </div>
  );
}

type Values = { value: string };

function Body({ view, scope, onMove, onSaved }: { view: SettingsEditView; scope: "all" | "this"; onMove: (key: string, scope: "all" | "this") => void; onSaved: (s: SettingsSaved) => void }) {
  const form = useForm<Values>({ defaultValues: { value: view.value } });
  // The last answer of a Check, for the view (config and scope) it was asked for; a new view replaces it.
  const [answer, setAnswer] = useState<{ of: SettingsEditView; form: SettingsEditView } | null>(null);
  const shown = answer && answer.of === view ? answer.form : view;
  const [refused, setRefused] = useState<SettingsRefused | null>(null);
  const [busy, setBusy] = useState<"check" | "save" | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  // The teaching follows the scope the server answered for; what was typed stays.
  const teach = shown.teach;
  const typed = useWatch({ control: form.control, name: "value" });
  const unit = unitHint(view.key, typed) ?? view.unit;

  const send = async (check: boolean) => {
    setBusy(check ? "check" : "save");
    setFailure(null);
    try {
      const out = await post<SettingsOutcome>("/api/v1/settings/edit", { key: view.key, was: view.key, scope, value: form.getValues("value"), check });
      if (out.outcome === "saved") {
        onSaved(out.saved);
        return;
      }
      if (out.outcome === "refused") {
        setRefused(out.refused);
        form.clearErrors("value");
        return;
      }
      setRefused(null);
      setAnswer({ of: view, form: out.form });
      if (out.form.error) form.setError("value", { message: out.form.error });
      else form.clearErrors("value");
    } catch (e) {
      setFailure(e instanceof ApiFailure ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };
  const checked = shown.checked && shown.checked.after === typed.trim() ? shown.checked : null;

  return (
    <Form {...form}>
      <form
        noValidate
        className="flex min-w-0 flex-col gap-5"
        onSubmit={form.handleSubmit(() => void send(false))}
        data-testid="settings-edit-form"
      >
        <div className="flex flex-col gap-2">
          <Label htmlFor="cfg-key">Config</Label>
          <Select value={view.key} onValueChange={(k) => onMove(k, scope)}>
            <SelectTrigger id="cfg-key" className="w-full bg-card font-mono" data-testid="cfg-key">
              <SelectValue />
            </SelectTrigger>
            <SelectContent position="popper" className="max-h-80">
              {(view.keys.length ? view.keys : [...DYNAMIC_KEYS]).map((k) => (
                <SelectItem key={k} value={k} className="font-mono">
                  {k}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <p className="text-[12.5px] text-muted-foreground">{view.what}. One of the five; any other config cannot be changed while the broker runs.</p>
        </div>
        <div className="flex flex-col gap-2">
          <Label id="cfg-scope-l">Applies to</Label>
          <ToggleGroup type="single" variant="outline" value={scope} onValueChange={(v) => v && onMove(view.key, v as "all" | "this")} aria-labelledby="cfg-scope-l" className="self-start bg-card" data-testid="cfg-scope">
            <ToggleGroupItem value="all" className="px-3">
              All brokers
            </ToggleGroupItem>
            <ToggleGroupItem value="this" className="px-3">
              This broker ({view.node_id})
            </ToggleGroupItem>
          </ToggleGroup>
          <p className="text-[12.5px] text-muted-foreground">This broker's own value wins over the one for all brokers.</p>
        </div>
        <FormField
          control={form.control}
          name="value"
          render={({ field }) => (
            <FormItem>
              <FormLabel>New value</FormLabel>
              <div className="relative">
                <FormControl>
                  <Input
                    {...field}
                    inputMode="numeric"
                    autoComplete="off"
                    className="bg-card pr-40 font-mono"
                    data-testid="cfg-value"
                    onChange={(e) => {
                      field.onChange(e);
                      setRefused(null);
                    }}
                  />
                </FormControl>
                {unit && (
                  <span className="pointer-events-none absolute inset-y-0 right-3 flex max-w-36 items-center truncate font-mono text-[12.5px] text-muted-foreground" data-testid="cfg-unit">
                    {unit}
                  </span>
                )}
              </div>
              <FormMessage />
              <div className="flex flex-col gap-2 pt-1 text-muted-foreground" data-testid="cfg-teach">
                  <Line label="Now" line={teach.now} />
                  <Line label="Allowed" line={teach.allowed} />
                  <Line label="Takes effect" line={teach.takes_effect} />
                  <Line label="Who sees it" line={teach.who_sees} />
                </div>
            </FormItem>
          )}
        />
        <div className="min-h-0" aria-live="polite">
          {checked && (
            <p className="flex items-center gap-2 rounded-lg border border-good/30 bg-good/5 px-3 py-2 text-[13px]" data-testid="cfg-checked">
              <CircleCheckIcon className="size-4 shrink-0 text-good" aria-hidden />
              <span>
                The broker would take it (validate_only): <code className="font-mono">{checked.before}</code> → <code className="font-mono font-semibold">{checked.after}</code>. Nothing changed yet.
              </span>
            </p>
          )}
          {refused && <Refused r={refused} />}
          {failure && (
            <p role="alert" className="text-[13px] text-bad">
              {failure}
            </p>
          )}
        </div>
        {!refused && <CallBox lines={shown.call} />}
        <DialogFooter className="border-t pt-4">
          <DialogClose asChild>
            <Button type="button" variant="outline">
              Cancel
            </Button>
          </DialogClose>
          <Button type="button" variant="outline" disabled={busy !== null} onClick={() => void send(true)} data-testid="cfg-check">
            <CheckCheckIcon /> {busy === "check" ? "Checking…" : "Check"}
          </Button>
          <Button type="submit" disabled={busy !== null} data-testid="cfg-save">
            <SaveIcon /> {busy === "save" ? "Saving…" : "Save"}
          </Button>
        </DialogFooter>
      </form>
    </Form>
  );
}

/** The Edit dialog of one of the five dynamic broker configs; its address is `?edit=<key>&scope=this`. */
export function EditConfigDialog({ configKey, scope, onMove, onClose, onSaved, restoreFocus }: { configKey: string | undefined; scope: "all" | "this"; onMove: (key: string, scope: "all" | "this") => void; onClose: () => void; onSaved: (s: SettingsSaved) => void; restoreFocus?: (e: Event) => void }) {
  const q = useSettingsEdit(configKey, scope);
  const view = q.data;
  return (
    <Dialog open={!!configKey} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="top-6 max-h-[calc(100vh-3rem)] translate-y-0 overflow-y-auto sm:max-w-[40rem]" data-testid="settings-edit" onCloseAutoFocus={restoreFocus}>
        <DialogHeader>
          <DialogTitle className="text-[20px] font-medium">
            Edit <span className="font-mono">{configKey}</span>
          </DialogTitle>
          <DialogDescription>{view?.what ?? "One of the five broker configs that can change while the broker runs."}</DialogDescription>
        </DialogHeader>
        {q.error ? (
          <p role="alert" className="text-[13.5px] text-bad">
            {q.error.message}
          </p>
        ) : view && view.key === configKey ? (
          <Body key={view.key} view={view} scope={scope} onMove={onMove} onSaved={onSaved} />
        ) : (
          <Skeleton className="h-96 w-full" />
        )}
      </DialogContent>
    </Dialog>
  );
}
