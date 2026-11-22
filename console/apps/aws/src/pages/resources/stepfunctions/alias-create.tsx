// Create alias (stepfunctions__alias-create): a name, a description, and the routing, one version or a split of two by percentage,
// with the CreateStateMachineAlias call drawn as it is typed.
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { Field, FormPage, FormSection, RuleHelp, useServiceCrumbs } from "../../../kit";
import { callOp, on } from "../deep/api";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { OwnProps } from "../deep/registry";
import { useAliases, useGoTo, useVersions, useWhere, machineHref } from "./hooks";
import { aliasProblem } from "./model";

export function AliasCreate({ name: machine }: OwnProps) {
  const name = machine ?? "";
  const go = useGoTo();
  const qc = useQueryClient();
  const base = useServiceCrumbs("stepfunctions");
  const where = useWhere();
  const versions = useVersions(name);
  const aliases = useAliases(name);
  const [alias, setAlias] = useState("");
  const [description, setDescription] = useState("");
  const [first, setFirst] = useState<string | null>(null);
  const [second, setSecond] = useState<string | null>(null);
  const [split, setSplit] = useState(false);
  const [weight, setWeight] = useState("100");
  const [weight2, setWeight2] = useState("0");
  const [tried, setTried] = useState(false);
  const make = useMutation({
    mutationFn: (input: object) => callOp<{ stateMachineAliasArn: string }>(on.machine(name), "CreateStateMachineAlias", input),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["resources", "deep", "stepfunctions", "machines", name] }),
  });
  if (!versions.data || !aliases.data) return <Skeleton className="h-80 rounded-[14px]" />;
  const list = [...versions.data].sort((a, b) => Number(b.number) - Number(a.number));
  const taken = aliases.data.map((a) => a.name);
  const arnOf = (n: string | null) => `arn:aws:states:${where.region}:${where.account}:stateMachine:${name}:${n ?? "?"}`;
  const v1 = first ?? list[0]?.number ?? null;
  const v2 = second ?? list.find((v) => v.number !== v1)?.number ?? null;
  const w1 = Number(weight);
  const w2 = Number(weight2);
  const okWeight = (w: number, s: string) => s.trim() !== "" && Number.isInteger(w) && w >= 0 && w <= 100;
  const problem = aliasProblem(alias, taken);
  const sum = split ? w1 + w2 : w1;
  const weights = okWeight(w1, weight) && (!split || okWeight(w2, weight2)) && sum === 100;
  const routing = [
    { stateMachineVersionArn: arnOf(v1), weight: w1 },
    ...(split ? [{ stateMachineVersionArn: arnOf(v2), weight: w2 }] : []),
  ];
  const input = { name: alias, ...(description ? { description } : {}), routingConfiguration: routing };
  const noVersion = list.length === 0;
  const valid = !problem && weights && v1 !== null && (!split || (v2 !== null && v2 !== v1));
  const failed = make.error ? errorWords(make.error) : null;
  const toggle = (on: boolean) => {
    setSplit(on);
    setWeight(on ? "90" : "100");
    setWeight2(on ? "10" : "0");
  };
  const sel = (value: string | null, set: (v: string) => void, label: string, testId: string) => (
    <Select value={value ?? undefined} onValueChange={set}>
      <SelectTrigger className="w-full bg-card" aria-label={label} data-testid={testId}><SelectValue placeholder="Choose a version" /></SelectTrigger>
      <SelectContent>{list.map((v) => <SelectItem key={v.number} value={v.number}>Version {v.number}</SelectItem>)}</SelectContent>
    </Select>
  );
  return (
    <FormPage
      title="Create alias"
      sub={<>Point an alias at one version, or split traffic between two. <span className="text-muted-foreground">Publish both versions first.</span></>}
      crumbs={[...base, { label: "State machines", to: "/resources/stepfunctions/machines" }, { label: name, to: machineHref(name) }, { label: "Aliases", to: machineHref(name, { tab: "aliases" }) }]}
      request={{ service: "stepfunctions", operation: "CreateStateMachineAlias", input }}
      callNote={<>Both ARNs are of versions of <span className="font-mono font-semibold text-foreground">{name}</span>, as the call requires.</>}
      refusal={failed ? { code: failed.code ?? "Error", message: failed.message } : null}
      banner={failed ? <Alert tone="bad" title={`CreateStateMachineAlias was refused${failed.code ? `: ${failed.code}` : ""}`} testId="alias-refused">{failed.message}</Alert> : undefined}
      submitLabel="Create alias"
      pending={make.isPending}
      disabled={tried && !valid}
      disabledWhy="Fix the alias first."
      onCancel={() => go.machine(name, { tab: "aliases" })}
      onSubmit={() => { setTried(true); if (valid) make.mutate(input, { onSuccess: () => go.machine(name, { tab: "aliases", done: "alias-created" }) }); }}
      testId="alias-create-page"
    >
      <FormSection title="Alias">
        <Field label="Alias name" error={tried && problem ? problem : undefined} help={<>1 to 80 characters of letters, digits, <span className="font-mono">- _ .</span>, with at least one letter or one of <span className="font-mono">- _ .</span> To avoid conflict with version ARNs, don't use an integer as the name.{taken.length > 0 && <span className="text-muted-foreground"> {taken.slice(0, 4).join(" and ")} already {taken.length === 1 ? "exists" : "exist"}.</span>}</>}>
          {(id) => <Input id={id} value={alias} onChange={(e) => setAlias(e.target.value)} autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="alias-name" />}
        </Field>
        <Field label="Description" optional hint="Up to 256 characters." className="mt-6">
          {(id) => <Input id={id} value={description} onChange={(e) => setDescription(e.target.value)} maxLength={256} className="bg-card" data-testid="alias-description" />}
        </Field>
      </FormSection>
      <FormSection title="Routing configuration">
        {noVersion ? <Alert tone="warn" title="This state machine has no published version" testId="alias-no-versions">Publish a version from Edit first: an alias points at versions.</Alert> : (
          <div className="grid gap-6">
            <div className="grid gap-6 sm:grid-cols-2">
              <Field label="Version">{() => sel(v1, setFirst, "Version", "alias-version-1")}</Field>
              <Field label="Traffic percentage">{(id) => <div className="relative"><Input id={id} inputMode="numeric" value={weight} onChange={(e) => setWeight(e.target.value)} className="bg-card pr-8" data-testid="alias-weight-1" /><span aria-hidden className="absolute top-1/2 right-3 -translate-y-1/2 text-muted-foreground">%</span></div>}</Field>
            </div>
            <div className="flex items-start gap-3">
              <Checkbox id="alias-split" checked={split} onCheckedChange={(c) => toggle(c === true)} className="mt-0.5" data-testid="alias-split" />
              <div className="grid gap-0.5">
                <Label htmlFor="alias-split">Split traffic between two versions</Label>
                <p className="text-[13px] text-muted-foreground">Clear it to point the alias at a single version.</p>
              </div>
            </div>
            {split && (
              <div className="grid gap-6 sm:grid-cols-2">
                <Field label="Version">{() => sel(v2, setSecond, "Second version", "alias-version-2")}</Field>
                <Field label="Traffic percentage">{(id) => <div className="relative"><Input id={id} inputMode="numeric" value={weight2} onChange={(e) => setWeight2(e.target.value)} className="bg-card pr-8" data-testid="alias-weight-2" /><span aria-hidden className="absolute top-1/2 right-3 -translate-y-1/2 text-muted-foreground">%</span></div>}</Field>
              </div>
            )}
            <RuleHelp data-testid="alias-weights-help">
              {split ? (
                <>Each percentage is 0 to 100 and the two must add up to <b>100</b>{okWeight(w1, weight) && okWeight(w2, weight2) ? `: ${w1} + ${w2} = ${w1 + w2}` : ""}. For every execution started from the alias, Step Functions picks a version at random by these weights. <span className="text-muted-foreground">Both versions must belong to {name}.</span></>
              ) : (
                <>With one version, all <b>100</b> percent of the executions started from the alias run it. <span className="text-muted-foreground">The version must belong to {name}.</span></>
              )}
            </RuleHelp>
            {tried && !weights && <p role="alert" className="text-[13px] text-bad" data-testid="alias-weights-error">The percentages must be whole numbers from 0 to 100 that add up to 100.</p>}
          </div>
        )}
      </FormSection>
    </FormPage>
  );
}
