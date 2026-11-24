// What the KMS pages share: a key's state as a word with its dot, a key's id, the pages' crumbs.
import { StatusDot } from "@doze/ui/components/app/status-dot";
import type { KeyRow } from "./model";

/** A key's state as a word with a dot of its colour. */
export function StateDot({ state, words }: { state: string; words: string }) {
  const tone = state === "Enabled" ? "ok" : state === "Disabled" ? "idle" : "warn";
  return <StatusDot tone={tone} className="font-medium" data-testid="key-state">{words}</StatusDot>;
}

export const stateOf = (k: Pick<KeyRow, "state" | "stateWords">) => <StateDot state={k.state} words={k.stateWords} />;

export const crumbs = (group: string | undefined, title: string | undefined) => [{ label: "Resources", to: "/resources" }, ...(group ? [{ label: title ?? "KMS", to: "/resources/kms" }] : [])];
