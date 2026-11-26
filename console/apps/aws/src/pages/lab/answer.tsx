import { Link } from "@tanstack/react-router";
import { EyeIcon, EyeOffIcon, RadioIcon } from "lucide-react";
import { useState } from "react";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { micros } from "@doze/ui/lib/format";
import type { LabAnswer } from "../../api/types/LabAnswer";
import { Card, CodeBlock } from "../card";
import { hideSecret } from "./recipes";

/** What the call got back: the output, or the service's refusal in its words, with the call's place in Live. A secret's value (what
 * GetSecretValue answers) is hidden until the person shows it. */
export function Answer({ answer, op }: { answer: LabAnswer; op: string }) {
  const [shown, setShown] = useState(false);
  const output = answer.output ?? "";
  const hidden = hideSecret(output);
  const hasSecret = hidden !== output;
  const refused = answer.error;
  return (
    <Card
      testId="lab-answer"
      title={refused ? <StatusDot tone="bad">{refused.code}</StatusDot> : <StatusDot tone="ok">Answered</StatusDot>}
      lede={`${op} took ${micros(Number(answer.took_us))}${refused ? `, status ${refused.status}` : ""}.`}
      actions={
        answer.seq !== null ? (
          <Button asChild variant="outline" size="sm" className="bg-card">
            <Link to="/live" search={{ call: Number(answer.seq), console: "shown" }} data-testid="lab-in-live">
              <RadioIcon /> Open in Live
            </Link>
          </Button>
        ) : undefined
      }
    >
      {refused ? (
        <div className="grid grid-cols-[minmax(0,1fr)] gap-2">
          <p className="text-[13.5px] text-foreground" data-testid="lab-refusal">{refused.message}</p>
          {answer.seq === null && <p className="text-[12.5px] text-muted-foreground">It was turned away before the service was asked, so Live has no row for it.</p>}
        </div>
      ) : (
        <div className="grid grid-cols-[minmax(0,1fr)] gap-3">
          {hasSecret && (
            <div className="flex items-center gap-3 text-[13px] text-muted-foreground">
              <span>The secret&apos;s value is hidden.</span>
              <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => setShown((s) => !s)} aria-pressed={shown} data-testid="lab-reveal">
                {shown ? <EyeOffIcon /> : <EyeIcon />} {shown ? "Hide value" : "Show value"}
              </Button>
            </div>
          )}
          <CodeBlock code={hasSecret && !shown ? hidden : output} what="Output copied" testId="lab-output" />
        </div>
      )}
    </Card>
  );
}
