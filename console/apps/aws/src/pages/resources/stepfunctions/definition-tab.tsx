// A state machine's Definition tab (stepfunctions__machine-definition): the Amazon States Language, read only, beside the graph drawn
// from it, and what the definition's timeouts and heartbeats mean.
import { PencilIcon } from "lucide-react";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { Button } from "@doze/ui/components/ui/button";
import { Card, CodeBlock } from "../deep/parts";
import { type Definition } from "./graph";
import { GraphView } from "./graph-view";
import { definitionNotes } from "./model";

/** The text of a definition as the console shows it: two-space JSON when it is JSON, as it is when not. */
export function shownDefinition(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

export function DefinitionTab({ definition, text, onEdit }: { definition: Definition | null; text: string; onEdit: () => void }) {
  const notes = definitionNotes(definition ?? undefined);
  return (
    <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_22rem]" data-testid="definition-tab">
      <div className="min-w-0">
        <div className="mb-4 flex items-center justify-between gap-3">
          <p className="text-[13.5px] text-muted-foreground">Read only. Edit opens it in Workflow Studio.</p>
          <Button type="button" variant="outline" className="bg-card" onClick={onEdit} data-testid="definition-edit"><PencilIcon /> Edit</Button>
        </div>
        <CodeBlock text={shownDefinition(text)} max="max-h-none" testId="definition-code" />
      </div>
      <div className="grid min-w-0 gap-5">
        <Card title="Graph" lede={definition?.StartAt ? <>Drawn from the definition. The first state is <span className="font-mono">{definition.StartAt}</span>.</> : "Drawn from the definition."} testId="definition-graph">
          {definition ? <GraphView definition={definition} /> : <p className="text-[13.5px] text-muted-foreground">The definition is not JSON, so there is no graph to draw.</p>}
        </Card>
        <p className="border-l-2 border-input pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="definition-notes"><Emphasis text={notes.text} names={notes.names} bold={[/\bTimeoutSeconds: \d+/, /\bHeartbeatSeconds \d+/]} /></p>
      </div>
    </div>
  );
}
