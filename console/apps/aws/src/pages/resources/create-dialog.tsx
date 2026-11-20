import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { useCreate } from "../../api/resources";
import type { FormInput } from "../../api/types/FormInput";

function first(inputs: FormInput[]): Record<string, string> {
  return Object.fromEntries(inputs.map((i) => [i.key, i.value]));
}

/** A placeholder is an example, never a value: said so (`e.g. 30`), in the muted colour of every placeholder. */
export const example = (p: string): string | undefined => (p ? `e.g. ${p}` : undefined);

function Field({ input, value, set }: { input: FormInput; value: string; set: (v: string) => void }) {
  const id = `create-${input.key}`;
  const help = input.help && (
    <p id={`${id}-help`} className="text-[12.5px] leading-[18px] text-muted-foreground">
      {input.help}
    </p>
  );
  if (input.input === "toggle") {
    return (
      <div className="flex items-start gap-3">
        <Switch id={id} checked={value === "true"} onCheckedChange={(on) => set(on ? "true" : "")} aria-describedby={input.help ? `${id}-help` : undefined} className="mt-0.5" data-testid={`field-${input.key}`} />
        <div className="grid gap-1">
          <Label htmlFor={id}>{input.label}</Label>
          {help}
        </div>
      </div>
    );
  }
  const common = { id, value, "aria-describedby": input.help ? `${id}-help` : undefined, "aria-required": input.required, "data-testid": `field-${input.key}` };
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>
        {input.label}
        {!input.required && <span className="ml-1.5 font-normal text-muted-foreground">optional</span>}
      </Label>
      {input.input === "select" ? (
        <Select value={value} onValueChange={set}>
          <SelectTrigger id={id} className="w-full bg-card font-mono text-[13px]" aria-describedby={common["aria-describedby"]} data-testid={common["data-testid"]}>
            <SelectValue placeholder="Choose…" />
          </SelectTrigger>
          <SelectContent>
            {input.options.map((o) => (
              <SelectItem key={o} value={o} className="font-mono text-[13px]">
                {o}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : input.input === "textarea" ? (
        <Textarea {...common} placeholder={example(input.placeholder)} onChange={(e) => set(e.target.value)} className="min-h-24 bg-card font-mono text-[13px]" spellCheck={false} />
      ) : (
        <Input {...common} type="text" inputMode={input.input === "number" ? "numeric" : undefined} placeholder={example(input.placeholder)} onChange={(e) => set(e.target.value)} className="bg-card font-mono text-[13px]" spellCheck={false} autoComplete="off" />
      )}
      {help}
    </div>
  );
}

/** Create one of a kind: the descriptor's form in a dialog over the list. The service's own refusal is shown in the dialog,
 * which stays open with what was typed; once made, the new resource's page opens. */
export function CreateDialog({ open, onOpenChange, service, kind, one, operation, inputs }: { open: boolean; onOpenChange: (o: boolean) => void; service: string; kind: string; one: string; operation: string; inputs: FormInput[] }) {
  const [form, setForm] = useState(() => first(inputs));
  const create = useCreate(service, kind);
  const navigate = useNavigate();
  const missing = inputs.filter((i) => i.required && !form[i.key]?.trim());
  const change = (o: boolean) => {
    if (!o) {
      setForm(first(inputs));
      create.reset();
    }
    onOpenChange(o);
  };
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (missing.length > 0 || create.isPending) return;
    create.mutate(form, {
      onSuccess: (made) => {
        change(false);
        void navigate({ to: "/resources/$service/$kind/$name", params: { service, kind, name: made.name } });
      },
    });
  };
  const failed = create.error ? errorWords(create.error) : null;
  return (
    <Dialog open={open} onOpenChange={change}>
      <DialogContent
        className="sm:max-w-[600px]"
        data-testid="create-dialog"
        onOpenAutoFocus={(e) => {
          // The first field still to fill, not one the page filled in (a subscription's topic).
          const empty = [...(e.currentTarget as HTMLElement).querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("input[data-testid^=field-], textarea[data-testid^=field-]")].find((el) => el.value === "");
          if (empty) {
            e.preventDefault();
            empty.focus();
          }
        }}
      >
        <form onSubmit={submit} className="grid gap-5">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Create {one}</DialogTitle>
            <DialogDescription>
              Made with <span className="font-mono text-foreground">{operation}</span> through the services' bus, as the caller <span className="font-mono text-foreground">console</span>: Live shows the call.
            </DialogDescription>
          </DialogHeader>
          {inputs.map((i) => (
            <Field key={i.key} input={i} value={form[i.key] ?? ""} set={(v) => setForm((f) => ({ ...f, [i.key]: v }))} />
          ))}
          {failed && (
            <Alert tone="bad" title={failed.code ? `${operation} was refused: ${failed.code}` : `${operation} failed`} testId="create-error">
              {failed.message}
            </Alert>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => change(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={missing.length > 0 || create.isPending} data-testid="create-submit">
              {create.isPending ? "Creating…" : `Create ${one}`}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
