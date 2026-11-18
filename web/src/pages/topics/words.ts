// Small words the Topics pages share, as the classic pages say them.
import type { Cleanup } from "@/api/types/Cleanup";
import type { Standing } from "@/api/types/Standing";
import type { Tone } from "@/components/app/status-dot";

export function policyWord(c: Cleanup): string {
  return c === "both" ? "compact,delete" : c;
}

export const STANDING: Record<Standing, { tone: Tone; word: string }> = {
  committed: { tone: "ok", word: "Committed" },
  above_lso: { tone: "warn", word: "Above LSO" },
  aborted: { tone: "idle", word: "Aborted" },
};
