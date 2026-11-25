// The rule form's state and how it becomes the body the server checks and makes (`LabRuleBody`). Pure: `rule-dialog.tsx` draws it.
import type { LabEnd } from "../../api/types/LabEnd";
import type { LabRuleBody } from "../../api/types/LabRuleBody";

/** The rehearsals that are rules, by the `tab` of the guide's lessons. */
export const RULE_TABS = ["throttle", "unavailable", "internal", "access-denied", "slow", "drop"] as const;
export type RuleTab = (typeof RULE_TABS)[number];
export const isRuleTab = (t: string): t is RuleTab => (RULE_TABS as readonly string[]).includes(t);

export type Unit = "seconds" | "minutes" | "hours";
export const UNIT_SECS: Record<Unit, number> = { seconds: 1, minutes: 60, hours: 3600 };

export type RuleForm = {
  tab: RuleTab;
  name: string;
  services: string[];
  operations: string[];
  regions: string[];
  callers: string[];
  resources: string[];
  delay_ms: string;
  jitter_ms: string;
  share: "every" | "nth" | "percent";
  n: string;
  percent: string;
  ends: string;
  unit: Unit;
  max_hits: string;
};

/** A new form: every call, ten minutes, a one-second delay for Slow. */
export function newForm(tab: RuleTab = "throttle"): RuleForm {
  return { tab, name: "", services: [], operations: [], regions: [], callers: [], resources: [], delay_ms: "1000", jitter_ms: "0", share: "every", n: "4", percent: "25", ends: "10", unit: "minutes", max_hits: "" };
}

/** A form for what a recipe names: its service, operation and the resource it was made from. */
export function formFor(what: { service: string; op: string; resource?: string }, tab: RuleTab = "throttle"): RuleForm {
  return { ...newForm(tab), services: [what.service], operations: [what.op], resources: what.resource ? [what.resource] : [] };
}

const whole = (s: string): number => (s.trim() !== "" && /^\d+$/.test(s.trim()) ? Number(s.trim()) : 0);

/** How long the rule lasts, in seconds (0 when what was typed is not a whole number: the server says so in words). */
export const endsInSecs = (f: RuleForm): number => whole(f.ends) * UNIT_SECS[f.unit];

/** The rule as it is asked for. A number that is not a whole number is sent as 0, and the check answers in words what is wrong with it. */
export function toBody(f: RuleForm): LabRuleBody {
  const effect: LabRuleBody["effect"] = f.tab === "slow" ? { kind: "slow", delay_ms: whole(f.delay_ms), jitter_ms: whole(f.jitter_ms) } : f.tab === "drop" ? { kind: "drop" } : { kind: "fail", error: f.tab === "access-denied" ? "access-denied" : f.tab };
  const share: LabRuleBody["share"] = f.share === "nth" ? { kind: "nth", n: whole(f.n) } : f.share === "percent" ? { kind: "percent", percent: whole(f.percent) } : { kind: "every" };
  const hits = whole(f.max_hits);
  return {
    name: f.name.trim(),
    matcher: { services: f.services, operations: f.operations, regions: f.regions, callers: f.callers, resources: f.resources },
    effect,
    share,
    ends_in_secs: endsInSecs(f),
    ...(f.max_hits.trim() !== "" ? { max_hits: hits } : {}),
  };
}

/** How a rule ended, in words. */
export const END_WORDS: Record<LabEnd, string> = {
  expired: "Ran out of time",
  spent: "Reached its number of hits",
  cleared: "Stopped",
  replaced: "Replaced by a newer rule on the same calls",
};
