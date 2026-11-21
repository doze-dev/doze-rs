// What doze-aws does with a trust policy, said by the mode IAM is in (crates/aws-iam/src/sts.rs, needs.rs): under --iam enforce AssumeRole needs
// the policy to allow the caller; with IAM soft or off it mints the session without evaluating it, which is the only time it is "stored, not applied".
import { useAbout } from "../../../api/queries";
import { Note } from "./parts";

export function trustWords(mode: string): { stored: boolean; text: string } {
  if (mode === "enforce") return { stored: false, text: "IAM is enforcing here: AssumeRole is allowed only to a principal this trust policy names, under its conditions." };
  return { stored: true, text: `IAM is ${mode === "off" ? "off" : "soft"} here: AssumeRole mints credentials without evaluating this trust policy. Under --iam enforce it is evaluated.` };
}

/** The note under a trust policy. `extra` is said before it (what the policy holds). */
export function TrustNote({ extra, className, testId = "trust-note" }: { extra?: React.ReactNode; className?: string; testId?: string }) {
  const mode = useAbout().data?.iam ?? "soft";
  const w = trustWords(mode);
  return (
    <Note className={className} testId={testId}>
      {extra}{extra ? " " : ""}{w.stored ? <><b>Stored, not applied locally.</b> <span className="dim">{w.text}</span></> : <span>{w.text}</span>}
    </Note>
  );
}
