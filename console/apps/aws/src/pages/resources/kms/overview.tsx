// What sits above a key's tabs (kms__key-detail): the sentence, the strip of what uses the key and the aliases it answers to, and
// the General configuration card.
import { Link } from "@tanstack/react-router";
import { ArrowRightIcon } from "lucide-react";
import { Fragment } from "react";
import { Card, Facts, Say } from "../cfg-parts";
import type { Use } from "./model";
import { StateDot } from "./parts";
import { Value } from "../value";

const chip = "inline-flex min-w-0 max-w-full items-center gap-2 rounded-lg border bg-well px-3 py-1.5 font-mono text-[13px] text-bright outline-none hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50";

/** `Used by [pick-lists, SQS queue] → [larder-app] Aliases [larder-app] [larder-queues]`: what the key encrypts, the key, its aliases. */
export function KeyStrip({ name, uses, aliases }: { name: string; uses: readonly Use[]; aliases: readonly string[] }) {
  if (uses.length === 0 && aliases.length === 0) return null;
  return (
    <nav aria-label="Wired to" className="mb-8 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-[14px] border bg-card px-5 py-4" data-testid="wired-strip">
      {uses.length > 0 && (
        <>
          <span className="text-[13.5px] text-muted-foreground">Used by</span>
          {uses.slice(0, 6).map((u) => (
            <Fragment key={`${u.service}/${u.name}`}>
              <Link to="/resources/$service/$kind/$name" params={{ service: u.service, kind: u.kind, name: u.name }} className={chip} data-testid="wired-link"><span className="truncate">{u.name}, {u.noun}</span></Link>
            </Fragment>
          ))}
          {uses.length > 6 && <span className="text-[13px] text-muted-foreground">and {uses.length - 6} more</span>}
          <ArrowRightIcon aria-hidden className="size-3.5 text-input" />
        </>
      )}
      <span aria-current="page" className="inline-flex items-center rounded-lg border border-primary/40 bg-pick px-3 py-1.5 font-mono text-[13px] text-bright" data-testid="wired-self">{name}</span>
      {aliases.length > 0 && <span className="text-[13.5px] text-muted-foreground">Aliases</span>}
      {aliases.map((a) => <span key={a} className={chip} data-testid="wired-alias">{a}</span>)}
    </nav>
  );
}

export function GeneralConfiguration({ id, arn, state, stateWords, created, description }: { id: string; arn: string | null; state: string; stateWords: string; created: string | null; description: string | null }) {
  return (
    <Card title="General configuration" testId="general-config">
      <Facts facts={[
        { label: "Key ID", value: <span className="font-mono text-[13px]">{id}</span> },
        { label: "ARN", value: <span className="font-mono text-[13px]">{arn ?? "–"}</span> },
        { label: "Status", value: <StateDot state={state} words={stateWords} /> },
        { label: "Creation date", value: <Value value={created} format="time" /> },
        { label: "Description", value: description || <span className="text-muted-foreground">–</span> },
      ]} />
    </Card>
  );
}

export const KeySay = ({ text, label, uses }: { text: string; label: string; uses: readonly Use[] }) => <Say text={text} names={[label, ...uses.map((u) => u.name)]} className="mb-6" testId="detail-sentence" />;
