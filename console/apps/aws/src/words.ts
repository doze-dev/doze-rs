// What the server's short words mean, said for a person. Each sentence restates the contract's own documentation
// (crates/aws-console/src/model.rs); a word this build does not know is shown as it is, never guessed at.
import type { CallRecord } from "./api/types/CallRecord";
import type { Outcome } from "./api/types/Outcome";

/** `--iam`: what IAM does with a request its policies refuse. */
export function iamWords(mode: string): { chip: string; sentence: string } {
  switch (mode) {
    case "off":
      return { chip: "IAM off", sentence: "IAM is off: no request is checked against a policy." };
    case "soft":
      return { chip: "IAM soft", sentence: "IAM is soft: a request its policies refuse is logged and let through." };
    case "enforce":
      return { chip: "IAM enforce", sentence: "IAM is enforced: a request its policies refuse is answered AccessDenied." };
    default:
      return { chip: `IAM ${mode}`, sentence: `IAM mode: ${mode}.` };
  }
}

/** `--auth`: whether signatures are checked. */
export function authWords(mode: string): string {
  switch (mode) {
    case "trust":
      return "Any access key is accepted; signatures are not checked.";
    case "verify":
      return "Every signature is checked.";
    default:
      return `Auth mode: ${mode}.`;
  }
}

export type OutcomeTone = "good" | "bad" | "denied" | "muted";

/** How a call ended, in the contract's four words. `label` is the word shown in capitals beside the status. */
export const OUTCOMES: Record<Outcome, { label: string; tone: OutcomeTone }> = {
  ok: { label: "OK", tone: "good" },
  error: { label: "ERROR", tone: "bad" },
  denied: { label: "DENIED", tone: "denied" },
  "not-built": { label: "NOT BUILT", tone: "muted" },
};

/** One sentence on why a call ended as it did. */
export function outcomeSentence(call: Pick<CallRecord, "outcome" | "status" | "error_code">): string {
  switch (call.outcome) {
    case "ok":
      return "Answered as asked.";
    case "denied":
      return "IAM refused it: the caller's policies do not allow this (--iam enforce).";
    case "not-built":
      if (call.error_code === "OptInRequired") return "doze-aws was started without this service (--services), so it answers every call to it with how to turn it on.";
      return "doze-aws does not do this yet: the operation is not built.";
    case "error":
      return call.status >= 500 ? "The server answered with an error of its own." : "The request was answered with an error: the caller's to fix.";
  }
}

/** What the would-be-denied marker says: only `--iam soft` lets a refused call through. */
export const WOULD_BE_DENIED = {
  short: "Would be denied",
  sentence: "IAM is soft: the caller's policies refuse this call, and it was let through. Under --iam enforce it would be DENIED.",
};

/** A service's name for a person: its title when the build has the service, else the name the call carried. */
export function serviceTitle(name: string | null, titles: ReadonlyMap<string, string>): string | null {
  if (name === null) return null;
  return titles.get(name) ?? name;
}
