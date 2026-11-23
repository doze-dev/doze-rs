// The KMS keys S3 can encrypt with, from the key list and the alias list the console already reads: a key by its alias when it has
// one, and which keys S3 cannot use and why. Pure; the page reads the two lists.
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { listWords } from "./words";

export type KmsChoice = { value: string; label: string; hint?: string };
export type KmsKeys = { usable: KmsChoice[]; unusable: { label: string; why: string }[] };

/** The key list's columns, in the order the descriptor names them: Description, State, Spec, Usage, Managed by. */
const [DESCRIPTION, STATE, SPEC, USAGE, MANAGER] = [0, 1, 2, 3, 4];

/** Why S3 cannot use a key for default encryption; `null` when it can (an enabled, symmetric, encrypt-and-decrypt key). */
export function whyNot(cells: readonly (string | null)[]): string | null {
  const spec = cells[SPEC] ?? "";
  const usage = cells[USAGE] ?? "";
  if (cells[STATE] !== "Enabled") return `is ${(cells[STATE] ?? "not enabled").toLowerCase()}`;
  if (spec !== "SYMMETRIC_DEFAULT") return `is ${spec}${usage === "SIGN_VERIFY" ? " for signing" : usage === "GENERATE_VERIFY_MAC" ? " for MACs" : ""}`;
  if (usage !== "ENCRYPT_DECRYPT") return `is for ${usage}`;
  return null;
}

/** `keys`: the key rows (name = key ID); `aliases`: the alias rows (name = `alias/x`, first cell = its key's ID). A key with an alias is
 * chosen by the alias's ARN (`arn:aws:kms:us-east-1:111122223333:alias/larder-app`), one without by its own ARN. */
export function kmsKeys(keys: readonly ResourceRow[], aliases: readonly ResourceRow[], region: string, account: string): KmsKeys {
  const out: KmsKeys = { usable: [], unusable: [] };
  for (const k of keys) {
    if (k.cells[MANAGER] === "AWS") continue;
    const mine = aliases.filter((a) => a.cells[0] === k.id).map((a) => a.name);
    const label = mine[0] ?? k.name;
    const why = whyNot(k.cells);
    if (why) {
      out.unusable.push({ label, why });
      continue;
    }
    const arn = mine[0] ? `arn:aws:kms:${region}:${account}:${mine[0]}` : `arn:aws:kms:${region}:${account}:key/${k.id}`;
    const hint = [k.cells[DESCRIPTION], mine.length > 1 ? `also ${listWords(mine.slice(1))}` : null].filter(Boolean).join(", ");
    out.usable.push({ value: arn, label, ...(hint ? { hint } : {}) });
  }
  out.usable.sort((a, b) => a.label.localeCompare(b.label));
  return out;
}

/** `Only a symmetric encryption key in the bucket's Region can be used. alias/larder-app qualifies; alias/larder-signing is RSA_2048 for signing, so S3 cannot use it.` */
export function kmsHint(k: KmsKeys): string {
  const head = "Only a symmetric encryption key in the bucket's Region can be used.";
  const yes = k.usable.slice(0, 3).map((u) => u.label);
  const no = k.unusable[0];
  const parts = [yes.length > 0 && `${listWords(yes)} ${yes.length === 1 ? "qualifies" : "qualify"}`, no && `${no.label} ${no.why}, so S3 cannot use it`].filter(Boolean);
  return parts.length === 0 ? head : `${head} ${parts.join("; ")}.`;
}
