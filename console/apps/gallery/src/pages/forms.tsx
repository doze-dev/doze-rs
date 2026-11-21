import { zodResolver } from "@hookform/resolvers/zod";
import { useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { Combobox } from "@doze/ui/components/app/combobox";
import { MultiSelect } from "@doze/ui/components/app/multi-select";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { UtcDateTimePicker } from "@doze/ui/components/app/utc-datetime-picker";
import { Done } from "@doze/ui/components/app/write-done";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Form, FormControl, FormDescription, FormField, FormItem, FormLabel, FormMessage } from "@doze/ui/components/ui/form";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { LONG, OPTIONS } from "../data";
import { Section, Specimen } from "../specimen";

const schema = z.object({
  name: z
    .string()
    .min(1, "A queue needs a name.")
    .max(80, "A name is 80 characters at most.")
    .regex(/^[A-Za-z0-9_-]+$/, "Letters, digits, - and _ only."),
  timeout: z.string().refine((v) => /^\d+$/.test(v) && Number(v) <= 43_200, "Whole seconds, from 0 to 43,200 (12 hours)."),
  fifo: z.boolean(),
});
type Values = z.infer<typeof schema>;

function TheForm() {
  const [made, setMade] = useState<Values | null>(null);
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: { name: "", timeout: "30", fifo: false } });
  return (
    <Form {...form}>
      <form noValidate className="grid w-full max-w-[40rem] items-start gap-5 md:grid-cols-2" data-testid="the-form" onSubmit={form.handleSubmit(setMade)}>
        <FormField
          control={form.control}
          name="name"
          render={({ field }) => (
            <FormItem>
              <FormLabel>Queue name</FormLabel>
              <FormControl>
                <Input placeholder="orders" autoComplete="off" {...field} data-testid="f-name" />
              </FormControl>
              <FormDescription>Letters, digits, - and _; 80 at most.</FormDescription>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="timeout"
          render={({ field }) => (
            <FormItem>
              <FormLabel>Visibility timeout, seconds</FormLabel>
              <FormControl>
                <Input inputMode="numeric" className="font-mono" {...field} data-testid="f-timeout" />
              </FormControl>
              <FormDescription>How long a received message stays hidden.</FormDescription>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="fifo"
          render={({ field }) => (
            <FormItem className="flex items-center gap-3 md:col-span-2">
              <FormControl>
                <Switch checked={field.value} onCheckedChange={field.onChange} data-testid="f-fifo" />
              </FormControl>
              <FormLabel>First in, first out</FormLabel>
            </FormItem>
          )}
        />
        <div className="flex items-center gap-3 md:col-span-2">
          <Button type="submit" data-testid="f-submit">
            Create queue
          </Button>
          <Button type="button" variant="ghost" onClick={() => (form.reset(), setMade(null))}>
            Reset
          </Button>
        </div>
        {made && (
          <div className="md:col-span-2">
            <Done testId="f-done">
              Queue <code className="text-bright">{made.name}</code> would be created{made.fifo ? " as FIFO" : ""}, hiding a received message for {made.timeout} s.
            </Done>
          </div>
        )}
      </form>
    </Form>
  );
}

export function FormsPage() {
  const [one, setOne] = useState<string | null>("orders");
  const [pattern, setPattern] = useState<string | null>(null);
  const [many, setMany] = useState<string[]>(["orders", "payments"]);
  const [when, setWhen] = useState<number | null>(Date.UTC(2026, 10, 19, 14, 30, 0));
  const [kind, setKind] = useState("standard");
  const [region, setRegion] = useState("us-east-1");
  return (
    <>
      <PageHeader title="Forms" sub="Fields, choices and pickers. Each is shown as it rests, disabled, and wrong; the form at the end validates as a console's create form does." />

      <Section id="input" title="Input and Textarea" source="components/ui/input.tsx · textarea.tsx">
        <Specimen label="Default">
          <div className="grid w-64 gap-1.5">
            <Label htmlFor="i-name">Name</Label>
            <Input id="i-name" placeholder="orders" data-testid="input-default" />
          </div>
          <div className="grid w-64 gap-1.5">
            <Label htmlFor="i-filled">Endpoint</Label>
            <Input id="i-filled" defaultValue="http://127.0.0.1:4566" className="font-mono" />
          </div>
        </Specimen>
        <Specimen label="Disabled">
          <Input disabled defaultValue="us-east-1" className="w-64" aria-label="Region, fixed" />
        </Specimen>
        <Specimen label="Error">
          <div className="grid w-64 gap-1.5">
            <Label htmlFor="i-bad">Queue name</Label>
            <Input id="i-bad" defaultValue="orders queue" aria-invalid aria-describedby="i-bad-why" />
            <p id="i-bad-why" className="text-[12.5px] text-bad">
              Letters, digits, - and _ only.
            </p>
          </div>
        </Specimen>
        <Specimen label="Long text">
          <Input defaultValue={LONG} className="w-64 font-mono" aria-label="A long value" />
        </Specimen>
        <Specimen label="Textarea">
          <Textarea className="w-[26rem] font-mono text-[13px]" rows={4} defaultValue={'{\n  "orderId": "o-1042",\n  "lines": 3\n}'} aria-label="Message body" />
          <Textarea className="w-64" disabled placeholder="Disabled" aria-label="Disabled" />
        </Specimen>
      </Section>

      <Section id="choice" title="Checkbox, RadioGroup and Switch" source="components/ui/checkbox.tsx · radio-group.tsx · switch.tsx">
        <Specimen label="Checkbox">
          <Label className="flex items-center gap-2 font-normal">
            <Checkbox defaultChecked data-testid="checkbox" /> Encrypt at rest
          </Label>
          <Label className="flex items-center gap-2 font-normal">
            <Checkbox /> Content-based deduplication
          </Label>
          <Label className="flex items-center gap-2 font-normal text-muted-foreground">
            <Checkbox disabled /> Disabled
          </Label>
          <Label className="flex items-center gap-2 font-normal text-muted-foreground">
            <Checkbox disabled checked /> Disabled, on
          </Label>
        </Specimen>
        <Specimen label="RadioGroup">
          <RadioGroup value={kind} onValueChange={setKind} className="flex gap-5" aria-label="Queue type" data-testid="radio">
            {(
              [
                ["standard", "Standard"],
                ["fifo", "FIFO"],
              ] as const
            ).map(([v, l]) => (
              <Label key={v} className="flex items-center gap-2 font-normal">
                <RadioGroupItem value={v} /> {l}
              </Label>
            ))}
            <Label className="flex items-center gap-2 font-normal text-muted-foreground">
              <RadioGroupItem value="none" disabled /> Not offered
            </Label>
          </RadioGroup>
        </Specimen>
        <Specimen label="Switch">
          <Label className="flex items-center gap-2 font-normal">
            <Switch defaultChecked data-testid="switch" /> Enforce IAM
          </Label>
          <Label className="flex items-center gap-2 font-normal">
            <Switch /> Record calls
          </Label>
          <Label className="flex items-center gap-2 font-normal text-muted-foreground">
            <Switch disabled /> Disabled
          </Label>
        </Specimen>
      </Section>

      <Section id="select" title="Select" source="components/ui/select.tsx" note="A short list. Opens on a click or Enter; arrows move, typing finds, Esc closes and keeps the value.">
        <Specimen label="Default">
          <Select value={region} onValueChange={setRegion}>
            <SelectTrigger className="w-56 bg-card" aria-label="Region" data-testid="select">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {["us-east-1", "us-west-2", "eu-west-1", "ap-southeast-2"].map((r) => (
                <SelectItem key={r} value={r}>
                  {r}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <span className="font-mono text-[12.5px] text-muted-foreground" data-testid="select-value">
            {region}
          </span>
        </Specimen>
        <Specimen label="Small, disabled">
          <Select defaultValue="5s">
            <SelectTrigger size="sm" className="w-40 bg-card" aria-label="Every">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="5s">Every 5 s</SelectItem>
              <SelectItem value="30s">Every 30 s</SelectItem>
            </SelectContent>
          </Select>
          <Select disabled defaultValue="x">
            <SelectTrigger className="w-40 bg-card" aria-label="Disabled">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="x">Disabled</SelectItem>
            </SelectContent>
          </Select>
        </Specimen>
      </Section>

      <Section id="combobox" title="Combobox and MultiSelect" source="components/app/combobox.tsx · multi-select.tsx" note="A long list, with a search field. With `pattern`, what is typed can be chosen as it is (a name or a glob).">
        <Specimen label="One">
          <Combobox options={OPTIONS} value={one} onChange={setOne} aria-label="Queue" placeholder="Choose a queue…" search="Search queues…" />
          <span className="font-mono text-[12.5px] text-muted-foreground" data-testid="combobox-value">
            {one ?? "none"}
          </span>
        </Specimen>
        <Specimen label="Empty, pattern">
          <Combobox options={OPTIONS} value={pattern} onChange={setPattern} aria-label="Name or pattern" placeholder="A name or a pattern…" pattern />
          <span className="font-mono text-[12.5px] text-muted-foreground" data-testid="pattern-value">
            {pattern ?? "none"}
          </span>
        </Specimen>
        <Specimen label="Several" className="items-start">
          <MultiSelect options={OPTIONS} value={many} onChange={setMany} noun="queues" aria-label="Queues" />
          <span className="pt-2 font-mono text-[12.5px] text-muted-foreground" data-testid="multi-value">
            {many.join(",") || "all"}
          </span>
        </Specimen>
      </Section>

      <Section id="datetime" title="UtcDateTimePicker" source="components/app/utc-datetime-picker.tsx" note="A moment in UTC: never the browser's own date widget, which differs in every engine and speaks the local zone.">
        <Specimen label="Chosen">
          <UtcDateTimePicker value={when} onChange={setWhen} id="when" />
        </Specimen>
      </Section>

      <Section id="form" title="Form" source="components/ui/form.tsx" note="react-hook-form with a schema: submit it empty to see the errors, each under its field and announced.">
        <Specimen label="Create" stack>
          <TheForm />
        </Specimen>
      </Section>
    </>
  );
}
