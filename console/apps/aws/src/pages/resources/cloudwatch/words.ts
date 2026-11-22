// What the CloudWatch pages say, apart from the alarm's own condition (model.ts). Pure.
import type { Tone } from "@doze/ui/components/app/status-dot";
import { type Alarm, metricName } from "./model";

export const stateTone = (state: string): Tone => (state === "ALARM" ? "bad" : state === "OK" ? "ok" : "idle");

/** What an alarm watches, as a person names it: its first dimension's value (the queue, the function), else its metric. */
export const watching = (a: Alarm): string => a.Dimensions?.[0]?.Value ?? metricName(a);

const hasActions = (a: Alarm) => [a.AlarmActions, a.OKActions, a.InsufficientDataActions].some((l) => (l?.length ?? 0) > 0);

/** The delete dialog's words for the alarms chosen. */
export function deleteWords(picked: readonly Alarm[]): string {
  const [one] = picked;
  if (picked.length === 1 && one) return `Delete ${one.AlarmName}. It stops watching ${watching(one)}${hasActions(one) ? " and its actions will not run again" : ""}.`;
  return `Delete ${picked.map((a) => a.AlarmName).join(", ")}. They stop watching their metrics${picked.some(hasActions) ? " and their actions will not run again" : ""}.`;
}
