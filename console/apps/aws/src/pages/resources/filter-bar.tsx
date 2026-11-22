// A search of a list by property: the property (and how it compares, for those that compare more than one way), the text, and the
// filters already added as chips that can be removed. Enter adds a filter. Secrets Manager's and Parameter Store's lists use it.
import { SearchIcon, XIcon } from "lucide-react";
import { useState } from "react";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";

export type FilterProp = { value: string; label: string; /** How it compares, when more than one way; the first is the default. */ options?: { value: string; label: string }[]; /** Chosen from these rather than typed. */ choices?: { value: string; label: string }[] };
export type FilterToken = { prop: string; option?: string; text: string };

export function tokenLabel(t: FilterToken, props: readonly FilterProp[]): string {
  const p = props.find((x) => x.value === t.prop);
  const option = p?.options?.find((o) => o.value === t.option)?.label;
  return `${p?.label ?? t.prop}${option ? ` ${option}` : ""}: ${t.text}`;
}

export function FilterBar({ props, tokens, onChange, label, placeholder, testId = "filter" }: { props: readonly FilterProp[]; tokens: readonly FilterToken[]; onChange: (tokens: FilterToken[]) => void; label: string; placeholder: string; testId?: string }) {
  const [prop, setProp] = useState(props[0]!.value);
  const [option, setOption] = useState<string | undefined>(props[0]!.options?.[0]?.value);
  const [text, setText] = useState("");
  const current = props.find((p) => p.value === prop)!;
  const add = (value = text) => {
    if (value.trim() === "") return;
    onChange([...tokens, { prop, ...(current.options ? { option } : {}), text: value.trim() }]);
    setText("");
  };
  const choose = (v: string) => {
    setProp(v);
    setOption(props.find((p) => p.value === v)?.options?.[0]?.value);
    setText("");
  };
  return (
    <div className="grid gap-2" data-testid={testId}>
      <form className="flex flex-wrap items-center gap-2" role="search" aria-label={label} onSubmit={(e) => { e.preventDefault(); add(); }}>
        <Select value={prop} onValueChange={choose}>
          <SelectTrigger className="w-40 bg-card" aria-label="Filter property" data-testid={`${testId}-prop`}><SelectValue /></SelectTrigger>
          <SelectContent>{props.map((p) => <SelectItem key={p.value} value={p.value}>{p.label}</SelectItem>)}</SelectContent>
        </Select>
        {current.options && (
          <Select value={option} onValueChange={setOption}>
            <SelectTrigger className="w-36 bg-card" aria-label="How it compares" data-testid={`${testId}-option`}><SelectValue /></SelectTrigger>
            <SelectContent>{current.options.map((o) => <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>)}</SelectContent>
          </Select>
        )}
        {current.choices ? (
          <Select value="" onValueChange={(v) => add(v)}>
            <SelectTrigger className="w-48 bg-card" aria-label={`Choose ${current.label.toLowerCase()}`} data-testid={`${testId}-choice`}><SelectValue placeholder={`Choose ${current.label.toLowerCase()}`} /></SelectTrigger>
            <SelectContent>{current.choices.map((c) => <SelectItem key={c.value} value={c.value}>{c.label}</SelectItem>)}</SelectContent>
          </Select>
        ) : (
          <div className="relative w-full max-w-[22rem] min-w-52 flex-1">
            <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input value={text} onChange={(e) => setText(e.target.value)} placeholder={placeholder} aria-label={placeholder} className="bg-card pl-9 text-[13px]" data-testid={`${testId}-text`} />
          </div>
        )}
        {!current.choices && <Button type="submit" variant="outline" className="bg-card" data-testid={`${testId}-add`}>Add filter</Button>}
      </form>
      {tokens.length > 0 && (
        <ul className="flex flex-wrap items-center gap-2" aria-label="Filters in use">
          {tokens.map((t, i) => (
            <li key={`${t.prop}${t.option}${t.text}`} className="inline-flex items-center gap-1 rounded-full border bg-pick py-0.5 pr-1 pl-3 text-[13px] text-bright" data-testid="filter-chip">
              {tokenLabel(t, props)}
              <Button type="button" variant="ghost" size="icon-xs" aria-label={`Remove ${tokenLabel(t, props)}`} onClick={() => onChange(tokens.filter((_, j) => j !== i))} data-testid="filter-remove"><XIcon /></Button>
            </li>
          ))}
          <li><Button type="button" variant="ghost" size="sm" onClick={() => onChange([])} data-testid="filter-clear">Clear filters</Button></li>
        </ul>
      )}
    </div>
  );
}
