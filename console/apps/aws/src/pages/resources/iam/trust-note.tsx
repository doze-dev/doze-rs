// What doze-aws does with a trust policy, said by the mode IAM is in (crates/aws-iam/src/sts.rs, needs.rs): under --iam enforce AssumeRole needs
// the policy to allow the caller; with IAM soft or off it mints the session without evaluating it, which is the only time it is "stored, not applied".
import { useAbout } from "../../../api/queries";
import { Note } from "./parts";

export function trustWords(mode: string): { stored: boolean; text: string } {
  if (mode === "enforce") return { stored: false, text: "IAM is enforcing here: AssumeRole is allowed only to a principal this trust policy names, under its conditions." };
  return { stored: true, text: `IAM is ${mode === "off" ? "off" : "soft"} here: AssumeRole mints credentials without evaluating this trust policy. Under --iam enforce it is evaluated.` };
}

/** What doze-aws does with the trust policy of a role that federated users assume: in no IAM mode is it evaluated (`needs.rs` asks nothing of the two federated calls). */
export const FEDERATED_WORDS = "AssumeRoleWithWebIdentity checks the token's signature, its issuer, its audience against the provider's client IDs and its expiry, and AssumeRoleWithSAML checks the signed assertion against the provider's metadata; neither evaluates this trust policy, whatever IAM mode is on.";

/** The note under a trust policy. `extra` is said before it (what the policy holds). `federated`: the policy is for a web identity or SAML role. */
export function TrustNote({ extra, className, testId = "trust-note", federated = false }: { extra?: React.ReactNode; className?: string; testId?: string; federated?: boolean }) {
  const mode = useAbout().data?.iam ?? "soft";
  const w = federated ? { stored: true, text: FEDERATED_WORDS } : trustWords(mode);
  return (
    <Note className={className} testId={testId}>
      {extra}{extra ? " " : ""}{w.stored ? <><b>Stored, not applied locally.</b> <span className="dim">{w.text}</span></> : <span>{w.text}</span>}
    </Note>
  );
}
