// Callers as a person reads them: an IAM user by its name, any other key as itself with what IAM makes of it said on hover;
// and times as "how long ago".
import type { KeyPrincipal } from "./api/types/KeyPrincipal";

/** What a caller is shown as, and the words on hover (which always hold the key). */
export function who(p: Pick<KeyPrincipal, "key" | "kind" | "name" | "arn">): { label: string; title: string } {
  switch (p.kind) {
    case "user":
      return { label: p.name ?? p.key, title: `IAM user ${p.name ?? ""} (${p.arn ?? "no ARN"}), signing with ${p.key}` };
    case "session":
      return { label: p.key, title: `${p.key}: a temporary key (a role session or a session token). IAM names its holder only from the session token, which Live does not keep.` };
    case "console":
      return { label: "console", title: "This console's own calls." };
    case "root":
      return { label: p.key, title: `${p.key}: a key IAM did not issue, so it is taken for the account root (${p.arn ?? "root"}), allowed everything.` };
  }
}

/** `2 s ago`, `4 min ago`, `3 h ago`, `2 d ago`; `just now` under a second (and for a clock that runs a little ahead). */
export function ago(atMs: number, nowMs: number): string {
  const s = Math.floor((nowMs - atMs) / 1000);
  if (s < 1) return "just now";
  if (s < 60) return `${s} s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h} h ago`;
  return `${Math.floor(h / 24)} d ago`;
}

/** A client's label without its version (`AWS CLI 2.15.30` is `AWS CLI`). */
export function clientName(label: string): string {
  const words = label.split(" ");
  return words.length > 1 && /^\d/.test(words[words.length - 1] ?? "") ? words.slice(0, -1).join(" ") : label;
}
