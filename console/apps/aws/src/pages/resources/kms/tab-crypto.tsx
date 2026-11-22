// The Cryptographic configuration tab (kms__key-crypto): what was fixed when the key was made, in the words of the console and the
// service's own names, and the algorithms an asymmetric key allows.
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { Card, Facts, Note, fieldOf } from "../cfg-parts";
import { useDescribed } from "./api";
import type { KeyRow } from "./model";

const USAGES: Record<string, string> = { ENCRYPT_DECRYPT: "Encrypt and decrypt", SIGN_VERIFY: "Sign and verify", GENERATE_VERIFY_MAC: "Generate and verify MAC", KEY_AGREEMENT: "Key agreement" };
const ORIGINS: Record<string, string> = { AWS_KMS: "AWS KMS", EXTERNAL: "External (imported key material)" };

const list = (title: string, items: readonly string[] | undefined) => (items && items.length > 0 ? [{ label: title, value: <span className="font-mono text-[12.5px]" data-testid={`fact-${title}`}>{items.join(", ")}</span> }] : []);

/** What a spec is, in a sentence. */
export function specWords(spec: string): string {
  if (spec === "SYMMETRIC_DEFAULT") return "SYMMETRIC_DEFAULT is a 256-bit AES-GCM key.";
  if (spec.startsWith("HMAC_")) return `${spec} is an HMAC key: it generates and verifies message authentication codes.`;
  if (spec.startsWith("RSA_")) return `${spec} is an RSA key pair of ${spec.slice(4)} bits.`;
  return `${spec} is an elliptic curve key pair.`;
}

export function TabCrypto({ d, id, kind }: { d: ResourceDetail; id: string; kind: KeyRow["kind"] }) {
  const q = useDescribed(id);
  const spec = fieldOf(d, "Key spec") ?? "";
  const usage = fieldOf(d, "Key usage") ?? "";
  const origin = fieldOf(d, "Origin") ?? "";
  const m = q.data?.KeyMetadata;
  const facts = [
    { label: "Key type", value: kind },
    { label: "Key spec", value: <span className="font-mono text-[13px]">{spec}</span> },
    { label: "Key usage", value: USAGES[usage] ?? usage },
    { label: "Origin", value: ORIGINS[origin] ?? origin },
    ...(fieldOf(d, "Multi-Region") === "Yes" ? [{ label: "Regionality", value: "Multi-Region key" }] : []),
    ...list("Encryption algorithms", kind === "Asymmetric" ? m?.EncryptionAlgorithms : undefined),
    ...list("Signing algorithms", m?.SigningAlgorithms),
    ...list("Key agreement algorithms", m?.KeyAgreementAlgorithms),
    ...list("MAC algorithms", m?.MacAlgorithms),
  ];
  return (
    <div className="grid gap-5" data-testid="crypto-tab">
      <Card title="Cryptographic configuration" lede="Fixed when the key was created. None of these can be changed." testId="crypto-card"><Facts facts={facts} /></Card>
      <Note testId="crypto-note">{specWords(spec)} {origin === "AWS_KMS" ? `Origin AWS KMS means KMS generated the key material${kind === "Symmetric" ? ", so on-demand and automatic rotation settings are available" : ""}.` : "Imported key material is rotated by importing new material."} {kind === "Asymmetric" ? "The algorithms the key allows are listed above." : "Encryption algorithms are listed only for asymmetric keys."}</Note>
    </div>
  );
}
