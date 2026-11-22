// Create key, as plain functions: what the form holds, what each choice allows, what the form's problems are, and the calls it makes.
import { filled, type KeyValueRow } from "@doze/ui/components/app/key-values";
import { buildPolicy, statementWords } from "./model";

export type Kind = "symmetric" | "asymmetric";
export type Usage = "ENCRYPT_DECRYPT" | "GENERATE_VERIFY_MAC" | "SIGN_VERIFY" | "KEY_AGREEMENT";

export type KeyForm = {
  kind: Kind;
  usage: Usage;
  spec: string;
  multiRegion: boolean;
  alias: string;
  description: string;
  tags: KeyValueRow[];
  /** ARNs of the IAM users and roles. */
  admins: string[];
  canDelete: boolean;
  users: string[];
  /** The policy as edited on the policy step; `null` while it is the one the choices build. */
  policy: string | null;
};

export const blankKey = (): KeyForm => ({ kind: "symmetric", usage: "ENCRYPT_DECRYPT", spec: "SYMMETRIC_DEFAULT", multiRegion: false, alias: "", description: "", tags: [], admins: [], canDelete: true, users: [], policy: null });

/** What a key of this kind is for. Symmetric keys encrypt and decrypt, or make MACs (an HMAC key, though the console files it under Symmetric as the AWS console does). */
export const usagesFor = (kind: Kind): { value: Usage; label: string; body: string }[] =>
  kind === "symmetric"
    ? [{ value: "ENCRYPT_DECRYPT", label: "Encrypt and decrypt", body: "" }, { value: "GENERATE_VERIFY_MAC", label: "Generate and verify MAC", body: "An HMAC key; you then choose an HMAC key spec." }]
    : [{ value: "SIGN_VERIFY", label: "Sign and verify", body: "Sign messages and verify signatures with the key pair." }, { value: "ENCRYPT_DECRYPT", label: "Encrypt and decrypt", body: "RSA keys only." }, { value: "KEY_AGREEMENT", label: "Key agreement", body: "Derive a shared secret with an elliptic curve key (DeriveSharedSecret)." }];

const RSA = ["RSA_2048", "RSA_3072", "RSA_4096"];
const ECC = ["ECC_NIST_P256", "ECC_NIST_P384", "ECC_NIST_P521", "ECC_SECG_P256K1", "ECC_NIST_EDWARDS25519"];
const HMAC = ["HMAC_224", "HMAC_256", "HMAC_384", "HMAC_512"];

/** The key specs a kind and usage allow; the first is the default. */
export function specsFor(kind: Kind, usage: Usage): string[] {
  if (kind === "symmetric") return usage === "GENERATE_VERIFY_MAC" ? HMAC : ["SYMMETRIC_DEFAULT"];
  if (usage === "ENCRYPT_DECRYPT") return RSA;
  if (usage === "KEY_AGREEMENT") return ECC.filter((s) => s !== "ECC_NIST_EDWARDS25519");
  return [...RSA, ...ECC];
}

/** The form after a choice of kind, then usage, keeps what still holds and takes the first spec otherwise. */
export function choose(f: KeyForm, patch: Partial<Pick<KeyForm, "kind" | "usage" | "spec">>): KeyForm {
  const kind = patch.kind ?? f.kind;
  let usage = patch.usage ?? (kind === f.kind ? f.usage : usagesFor(kind)[0]!.value);
  if (!usagesFor(kind).some((u) => u.value === usage)) usage = usagesFor(kind)[0]!.value;
  let spec = patch.spec ?? f.spec;
  if (!specsFor(kind, usage).includes(spec)) spec = specsFor(kind, usage)[0]!;
  return { ...f, kind, usage, spec };
}

/** What an alias may be: 1 to 256 characters of letters, digits, `/`, `_`, `-` and `:`, not beginning with `aws/` (nor `alias/`: the console adds it), and not one that exists. */
export function aliasProblem(alias: string, taken: readonly string[]): string | null {
  if (alias === "") return "Enter an alias. The console needs one so the key can be found by name.";
  if (alias.startsWith("alias/")) return "Do not type alias/; the console adds it.";
  if (alias.startsWith("aws/")) return "An alias cannot begin with aws/, which is reserved for AWS managed keys.";
  if (alias.length > 256) return "An alias is at most 256 characters.";
  if (!/^[a-zA-Z0-9/_:-]+$/.test(alias)) return "An alias may hold letters, digits and / _ - only.";
  if (taken.includes(alias)) return `${alias} is already an alias in this Region.`;
  return null;
}

export const descriptionProblem = (d: string): string | null => (d.length > 8192 ? "A description is at most 8,192 characters." : null);

/** The policy the choices build. */
export const policyOf = (f: KeyForm, account: string): string => JSON.stringify(buildPolicy({ account, admins: f.admins, canDelete: f.canDelete, users: f.users, usage: f.usage }), null, 2);
/** The policy that is sent: the edited one, else what the choices build. */
export const policySent = (f: KeyForm, account: string): string => f.policy ?? policyOf(f, account);

/** The members of CreateKey the form sets. The policy is left out until the step that sets it (`withPolicy`); the description and tags when there are none. */
export function createInput(f: KeyForm, o: { account: string; withPolicy: boolean; withLabels: boolean }): Record<string, unknown> {
  const tags = filled(f.tags);
  return {
    KeySpec: f.spec,
    KeyUsage: f.usage,
    ...(f.multiRegion ? { MultiRegion: true } : {}),
    ...(o.withLabels && f.description !== "" ? { Description: f.description } : {}),
    ...(o.withPolicy ? { Policy: policySent(f, o.account) } : {}),
    ...(o.withLabels && tags.length > 0 ? { Tags: tags.map((t) => ({ TagKey: t.key, TagValue: t.value })) } : {}),
  };
}

export const aliasInput = (f: KeyForm, keyId = "<the new key's id>") => ({ AliasName: `alias/${f.alias}`, TargetKeyId: keyId });

/** The calls the form makes on Finish, as the preview takes them. */
export function callsOf(f: KeyForm, o: { account: string; withPolicy: boolean; withLabels: boolean }): { service: string; operation: string; input: Record<string, unknown> }[] {
  return [{ service: "kms", operation: "CreateKey", input: createInput(f, o) }, ...(o.withLabels && f.alias !== "" ? [{ service: "kms", operation: "CreateAlias", input: aliasInput(f) }] : [])];
}

/** What the JSON tab of the panel shows of a request: the policy as `{ 2 statements }`, as the boards do. */
export function shownInput(_operation: string, input: unknown): unknown {
  const i = input as Record<string, unknown>;
  return typeof i.Policy === "string" ? { ...i, Policy: `{ ${statementWords(i.Policy)} }` } : input;
}

const USES: Record<string, string> = { ENCRYPT_DECRYPT: "encrypts and decrypts", SIGN_VERIFY: "signs and verifies", GENERATE_VERIFY_MAC: "makes and verifies MACs", KEY_AGREEMENT: "agrees keys" };

/** `larder-app encrypts and decrypts, larder-signing signs and verifies.`: what the account's other keys are for. */
export function othersWords(keys: readonly { aliases: string[]; id: string; usage: string; state: string }[]): string | null {
  const parts = keys.filter((k) => k.state !== "PendingDeletion" && k.aliases.length > 0 && USES[k.usage]).sort((a, b) => a.aliases[0]!.localeCompare(b.aliases[0]!)).slice(0, 4).map((k) => `${k.aliases[0]} ${USES[k.usage]}`);
  return parts.length === 0 ? null : `${parts.join(", ")}.`;
}
