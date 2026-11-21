// Add a layer (lambda__layer-add): a layer version of this account, or any layer's ARN, set on the function with
// UpdateFunctionConfiguration (the function's whole list of layers, each by its ARN with the version on the end).
import { useState } from "react";
import { Combobox } from "@doze/ui/components/app/combobox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Field, FormSection, useKindOpRead } from "../../../kit";
import { on, useOp } from "../deep/api";
import { LAYERS_MOST } from "./forms";
import { FnFormPage, type FormProps, Muted } from "./form-bits";
import { LAYERS, useLayers } from "./hooks";

type Version = { Version: number; Description?: string; LayerVersionArn: string };

export function LayerAdd(p: FormProps) {
  const { d, f } = p;
  const layers = useLayers();
  const [mode, setMode] = useState<"custom" | "arn">("custom");
  const [name, setName] = useState<string | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  const [arn, setArn] = useState("");
  const [tried, setTried] = useState(false);
  const chosen = name ?? (layers.rows.length === 1 ? (layers.rows[0]?.name ?? null) : null);
  const versions = useKindOpRead<{ LayerVersions?: Version[] }>(LAYERS, "ListLayerVersions", { LayerName: chosen ?? "" }, mode === "custom" && Boolean(chosen));
  const list = versions.data?.LayerVersions ?? [];
  const newest = list[0];
  const picked = list.find((v) => String(v.Version) === version) ?? newest;
  const target = mode === "custom" ? (picked?.LayerVersionArn ?? null) : /^arn:aws:lambda:[\w-]+:\d{12}:layer:[\w-]+:\d+$/.test(arn.trim()) ? arn.trim() : null;
  const save = useOp(on.fn(d.name), "UpdateFunctionConfiguration");
  const next = target ? [...f.layers.filter((l) => l !== target), target] : f.layers;
  const full = f.layers.length >= LAYERS_MOST;
  const problem = full ? `A function takes at most ${LAYERS_MOST} layers, and ${d.name} has ${f.layers.length}.` : target === null ? (mode === "custom" ? "Choose a layer and one of its versions." : "Enter the layer version's ARN: arn:aws:lambda:<region>:<account>:layer:<name>:<version>.") : null;
  const input = { FunctionName: d.name, Layers: next };
  const submit = () => {
    setTried(true);
    if (problem) return;
    save.mutate({ Layers: next }, { onSuccess: () => p.go({ tab: "code", done: "layer-added" }) });
  };
  return (
    <FnFormPage p={{ ...p, tab: "code" }} title="Add a layer" label="Add a layer" sub={<><b className="font-medium text-foreground">Attach a layer version to {d.name}.</b> A layer is a .zip file archive that contains supplementary code or data.</>}
      request={target ? { service: "lambda", operation: "UpdateFunctionConfiguration", input } : null} refusal={save.error} callNote="Each layer is given by its ARN with the version on the end."
      submitLabel="Add" pending={save.isPending} disabled={tried && Boolean(problem)} disabledWhy={problem} onSubmit={submit} testId="layer-add-form">
      <FormSection title="Function layer" testId="section-layer">
        <div className="grid gap-1.5">
          <Label>Choose a layer</Label>
          <RadioGroup value={mode} onValueChange={(v) => setMode(v as "custom" | "arn")} className="grid gap-2" aria-label="Where the layer comes from">
            <div className="rounded-lg border border-dashed px-4 py-3 text-[13.5px] text-muted-foreground opacity-80" data-testid="layer-aws-off"><span className="font-medium text-foreground">AWS layers</span><br />Not available in doze-aws: AWS-managed extension layers are hosted by AWS.</div>
            {[["custom", "Custom layers", "Layers created in this account."], ["arn", "Specify an ARN", "Use a layer from a different AWS account by its Amazon Resource Name (ARN)."]].map(([v, t, text]) => (
              <Label key={v} htmlFor={`mode-${v}`} className="flex cursor-pointer items-start gap-3 rounded-lg border bg-card px-4 py-3 font-normal has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40">
                <RadioGroupItem id={`mode-${v}`} value={v!} className="mt-0.5" data-testid={`layer-mode-${v}`} />
                <span className="grid gap-0.5"><span className="font-medium text-bright">{t}</span><span className="text-[13px] text-muted-foreground">{text}</span></span>
              </Label>
            ))}
          </RadioGroup>
        </div>
        {mode === "custom" ? (
          <>
            <Field label="Custom layers" help={<>From ListLayers: {layers.rows.length === 1 ? <><span className="font-mono">{layers.rows[0]?.name}</span> is the only layer in this account.</> : `${layers.rows.length} layers in this account.`} Choose a layer, then one of its versions.</>} testId="field-layer">
              {(id) => <Combobox id={id} options={layers.rows.map((l) => ({ value: l.name, label: l.name }))} value={chosen} onChange={(v) => { setName(v); setVersion(null); }} placeholder={layers.pending ? "Reading layers…" : layers.rows.length === 0 ? "There are no layers yet" : "Choose a layer"} search="Search layers…" empty="No layer matches." className="w-full max-w-[22rem] font-mono" aria-label="Choose a layer" />}
            </Field>
            <Field label="Layer version" help={<>From ListLayerVersions. Layer versions never change; to use new content, publish a new version and choose it here.{newest ? <Muted> Version {newest.Version} is the newest of {chosen}.</Muted> : null}<Muted> Locally a layer&apos;s folders go onto the interpreter search paths, not into /opt.</Muted></>} testId="field-version">
              {(id) => (
                <Select value={picked ? String(picked.Version) : undefined} onValueChange={setVersion} disabled={list.length === 0}>
                  <SelectTrigger id={id} className="w-full max-w-[16rem] bg-card font-mono" data-testid="layer-version"><SelectValue placeholder={chosen ? "Choose a version" : "Choose a layer first"} /></SelectTrigger>
                  <SelectContent>{list.map((v) => <SelectItem key={v.Version} value={String(v.Version)} className="font-mono">{v.Version}</SelectItem>)}</SelectContent>
                </Select>
              )}
            </Field>
          </>
        ) : (
          <Field label="Layer version ARN" error={tried && problem ? problem : undefined} help="The ARN of a layer version, with the version number on the end." testId="field-arn">
            {(id) => <Input id={id} value={arn} onChange={(e) => setArn(e.target.value)} placeholder="arn:aws:lambda:us-east-1:111122223333:layer:larder-shared:3" autoComplete="off" spellCheck={false} className="font-mono" data-testid="layer-arn" />}
          </Field>
        )}
        <p className="border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground" data-testid="layer-limits">You can add up to five layers to a function. <Muted>The unzipped size of the function and all its layers together cannot exceed 250 MB.</Muted></p>
      </FormSection>
    </FnFormPage>
  );
}
