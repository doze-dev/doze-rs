// A usage plan's form and the calls it makes: CreateUsagePlan with its throttle and quota, a stage added to or taken off a plan (a patch of
// `/apiStages`, valued `<api id>:<stage>`), and a key made and added in one step.
import type { Patch } from "./api";

export type PlanForm = { name: string; description: string; rate: string; burst: string; limit: string; period: "DAY" | "WEEK" | "MONTH" };
export const emptyPlan: PlanForm = { name: "", description: "", rate: "", burst: "", limit: "", period: "MONTH" };

const num = (s: string) => /^\d+(\.\d+)?$/.test(s.trim());
const whole = (s: string) => /^\d+$/.test(s.trim());

export function planProblem(f: PlanForm): string | null {
  if (f.name.trim() === "") return "Enter a name for the plan.";
  if ((f.rate !== "") !== (f.burst !== "")) return "Throttling needs both a rate and a burst.";
  if (f.rate !== "" && !(num(f.rate) && whole(f.burst))) return "The rate is a number of requests a second, the burst a whole number.";
  if (f.limit !== "" && !whole(f.limit)) return "The quota is a whole number of requests.";
  return null;
}

/** Throttle and quota are left out when their fields are empty: a plan may have either, both or neither. */
export function planInput(f: PlanForm): Record<string, unknown> {
  return {
    name: f.name || "<name>",
    ...(f.description ? { description: f.description } : {}),
    ...(f.rate !== "" ? { throttle: { rateLimit: Number(f.rate), burstLimit: Number(f.burst) } } : {}),
    ...(f.limit !== "" ? { quota: { limit: Number(f.limit), period: f.period } } : {}),
  };
}

export const stageValue = (api: string, stage: string): string => `${api}:${stage}`;
export const addStagePatch = (api: string, stage: string): Patch[] => [{ op: "add", path: "/apiStages", value: stageValue(api, stage) }];
export const removeStagePatch = (api: string, stage: string): Patch[] => [{ op: "remove", path: "/apiStages", value: stageValue(api, stage) }];

export const keyProblem = (name: string): string | null => (name.trim() === "" ? "Enter a name for the key." : null);
