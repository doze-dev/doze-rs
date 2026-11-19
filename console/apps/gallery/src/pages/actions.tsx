import { ArrowRightIcon, BoldIcon, ItalicIcon, Loader2Icon, PlusIcon, Rows3Icon, Rows4Icon, Trash2Icon, UnderlineIcon } from "lucide-react";
import { useState } from "react";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { HeldPill } from "@doze/ui/components/app/held-pill";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { Toggle } from "@doze/ui/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@doze/ui/components/ui/toggle-group";
import { LONG } from "../data";
import { Section, Specimen } from "../specimen";

const VARIANTS = ["default", "secondary", "outline", "ghost", "link", "destructive"] as const;
const BADGES = ["default", "secondary", "outline", "destructive", "ghost", "link"] as const;

export function ActionsPage() {
  const [held, setHeld] = useState(37);
  const [clicks, setClicks] = useState(0);
  const [density, setDensity] = useState("comfortable");
  return (
    <>
      <PageHeader title="Actions" sub="Buttons and the small things that behave like them. Tab through this page: every one shows where the focus is." />
      <Section id="button" title="Button" source="components/ui/button.tsx">
        <Specimen label="Variants">
          {VARIANTS.map((v) => (
            <Button key={v} variant={v} data-testid={`btn-${v}`} onClick={() => setClicks((n) => n + 1)}>
              {v === "default" ? "Create queue" : v === "destructive" ? "Delete" : v[0]!.toUpperCase() + v.slice(1)}
            </Button>
          ))}
          <output className="text-[12.5px] text-muted-foreground tabular-nums" data-testid="btn-clicks">
            {clicks} pressed
          </output>
        </Specimen>
        <Specimen label="Disabled">
          {VARIANTS.map((v) => (
            <Button key={v} variant={v} disabled>
              {v === "default" ? "Create queue" : v === "destructive" ? "Delete" : v[0]!.toUpperCase() + v.slice(1)}
            </Button>
          ))}
        </Specimen>
        <Specimen label="With an icon">
          <Button>
            <PlusIcon /> New topic
          </Button>
          <Button variant="outline">
            Next <ArrowRightIcon />
          </Button>
          <Button variant="destructive">
            <Trash2Icon /> Purge
          </Button>
          <Button variant="outline" size="icon" aria-label="Add">
            <PlusIcon />
          </Button>
          <Button variant="ghost" size="icon-sm" aria-label="Delete">
            <Trash2Icon />
          </Button>
        </Specimen>
        <Specimen label="Loading">
          <Button disabled aria-busy>
            <Loader2Icon className="animate-spin" /> Creating…
          </Button>
          <Button variant="outline" disabled aria-busy>
            <Loader2Icon className="animate-spin" /> Sending…
          </Button>
        </Specimen>
        <Specimen label="Sizes">
          <Button size="xs">Extra small</Button>
          <Button size="sm">Small</Button>
          <Button>Default</Button>
          <Button size="lg">Large</Button>
        </Specimen>
        <Specimen label="Long text">
          <Button variant="outline" className="max-w-[22rem]">
            <span className="truncate">{LONG}</span>
          </Button>
        </Specimen>
      </Section>

      <Section id="badge" title="Badge" source="components/ui/badge.tsx">
        <Specimen label="Variants">
          {BADGES.map((v) => (
            <Badge key={v} variant={v}>
              {v === "default" ? "FIFO" : v}
            </Badge>
          ))}
        </Specimen>
        <Specimen label="Long text">
          <Badge variant="secondary" className="max-w-[16rem] font-mono">
            <span className="truncate">{LONG}</span>
          </Badge>
        </Specimen>
      </Section>

      <Section id="toggle" title="Toggle and ToggleGroup" source="components/ui/toggle.tsx · toggle-group.tsx">
        <Specimen label="Toggle">
          <Toggle aria-label="Bold" data-testid="toggle-bold">
            <BoldIcon />
          </Toggle>
          <Toggle variant="outline" aria-label="Italic" defaultPressed>
            <ItalicIcon />
          </Toggle>
          <Toggle variant="outline" aria-label="Underline" disabled>
            <UnderlineIcon />
          </Toggle>
        </Specimen>
        <Specimen label="One of several">
          <ToggleGroup type="single" variant="outline" value={density} onValueChange={(v) => v && setDensity(v)} aria-label="Density" data-testid="density">
            <ToggleGroupItem value="comfortable" className="px-3">
              <Rows3Icon /> Comfortable
            </ToggleGroupItem>
            <ToggleGroupItem value="compact" className="px-3">
              <Rows4Icon /> Compact
            </ToggleGroupItem>
          </ToggleGroup>
          <span className="text-[12.5px] text-muted-foreground" data-testid="density-value">
            {density}
          </span>
        </Specimen>
      </Section>

      <Section id="copy" title="CopyButton" source="components/app/copy-button.tsx" note="Says Copied for 1.4 s in the width it had, so nothing beside it moves.">
        <Specimen label="Default">
          <CopyButton text="http://127.0.0.1:4566" what="Endpoint copied" />
          <CopyButton text="http://127.0.0.1:4566" label="Copy URL" variant="ghost" />
          <CopyButton text="http://127.0.0.1:4566" iconOnly variant="ghost" label="Copy the endpoint" />
        </Specimen>
      </Section>

      <Section id="held" title="HeldPill" source="components/app/held-pill.tsx" note="The rows a tail holds while the person reads. Its slot is always there, so nothing moves when it comes and goes.">
        <Specimen label="Holding">
          <HeldPill count={held} onShow={() => setHeld(0)} />
          <Button variant="outline" size="sm" onClick={() => setHeld((n) => n + 1200)}>
            Hold 1,200 more
          </Button>
        </Specimen>
      </Section>
    </>
  );
}
