// Switch Role (sts__switch-role): take on a role's permissions for a while. The form (the account by ID or alias, the role, how it is shown, its
// colour), the AssumeRole it makes, and what STS answered: a session the identity menu shows and Switch back ends. The credentials are real to
// SDKs; the console's own calls are never made with them.
import { useSearch } from "@tanstack/react-router";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Callout } from "@doze/ui/components/app/callout";
import { Input } from "@doze/ui/components/ui/input";
import { ApiFailure } from "@doze/ui/lib/api";
import { useAbout } from "../../../api/queries";
import { usePrincipals } from "../../../api/spaces";
import { useNow } from "../../card";
import { Field, FormPage, FormSection, callKindOp } from "../../../kit";
import { assumedOf, ROLES, USERS, useIamCalls, useRead } from "./api";
import { type Doc, needsExternalId } from "./model";
import { Segmented } from "./parts";
import { switchTo, type SessionColor } from "../../../shell/identity";

type Role = { RoleName: string; Path: string; AssumeRolePolicyDocument: Doc };

export function SwitchRole() {
  const search = useSearch({ strict: false }) as { account?: string; roleName?: string };
  const about = useAbout().data;
  const account = about?.account ?? "";
  const aliases = useRead<{ AccountAliases: string[] }>(USERS, "ListAccountAliases", {});
  const roles = useRead<{ Roles: Role[] }>(ROLES, "ListRoles", { MaxItems: 1000 });
  const calls = useIamCalls();
  const now = useNow();
  const principals = usePrincipals("all");
  const alias = aliases.data?.AccountAliases[0] ?? null;
  const [acct, setAcct] = useState<string | null>(search.account ?? null);
  const [role, setRole] = useState(search.roleName ?? "");
  const [display, setDisplay] = useState<string | null>(null);
  const [color, setColor] = useState<SessionColor | "">("");
  const [pending, setPending] = useState(false);
  const [tried, setTried] = useState(false);
  const [switched, setSwitched] = useState<{ name: string; inputs: string } | null>(null);
  const [failed, setFailed] = useState<unknown>(null);
  const account_ = acct ?? alias ?? account;
  // The banner says the form's last answer: it goes when the form is changed.
  const inputs = [acct, role, display, color].join("|");
  const done = switched?.inputs === inputs ? switched.name : null;
  const id = /^\d{12}$/.test(account_.trim()) ? account_.trim() : account_.trim() === alias ? account : null;
  const known = (roles.data?.Roles ?? []).find((r) => `${r.Path === "/" ? "" : r.Path.slice(1)}${r.RoleName}` === role.trim() || r.RoleName === role.trim());
  const external = known ? needsExternalId(known.AssumeRolePolicyDocument) : false;
  const accountProblem = id === null ? `No account has the alias ${account_.trim() || "(empty)"}: enter the 12-digit account ID${alias ? ` or ${alias}` : ""}.` : null;
  const roleProblem = role.trim() === "" ? "Enter the role name." : external ? "This role's trust policy requires an external ID: the console cannot switch to it. Call AssumeRole with ExternalId from the CLI or an SDK." : role.trim().length > 64 + (role.includes("/") ? 0 : 0) ? "The role name is at most 64 characters." : null;
  const suggested = `${role.trim().split("/").pop() || "role"} @ ${alias ?? id ?? account_}`;
  const shownName = display ?? suggested;
  const roleArn = `arn:aws:iam::${id ?? "000000000000"}:role/${role.trim() || "(a role)"}`;
  const input = { RoleArn: roleArn, RoleSessionName: `doze-console-${(role.trim().split("/").pop() || "role").replace(/[^\w+=,.@-]/g, "-").slice(0, 40)}`, DurationSeconds: 3600 };
  const keyNames = new Map((principals.data?.principals ?? []).filter((p) => p.kind === "user" && p.name).map((p) => [p.key, p.name as string]));
  const seen = role.trim() ? assumedOf(calls.data?.calls ?? [], role.trim().split("/").pop() ?? "", keyNames, now) : null;
  const last = seen && seen.count > 0 ? (calls.data?.calls ?? []).find((c) => c.service === "sts" && c.action === "AssumeRole" && c.caller !== "console" && (c.resource ?? "").endsWith(`/${role.trim()}`)) : null;
  const submit = async () => {
    setTried(true);
    if (accountProblem || roleProblem || id === null) return;
    setPending(true);
    setFailed(null);
    try {
      const out = (await callKindOp(ROLES, "AssumeRole", input)) as { Credentials: { AccessKeyId: string; SecretAccessKey: string; SessionToken: string; Expiration: number } };
      const c = out.Credentials;
      switchTo({ roleArn, role: role.trim(), account: id, display: shownName, color: color || null, accessKeyId: c.AccessKeyId, secretAccessKey: c.SecretAccessKey, sessionToken: c.SessionToken, expiresAt: c.Expiration * 1000 });
      setSwitched({ name: shownName, inputs });
    } catch (e) {
      setFailed(e);
      setSwitched(null);
    } finally {
      setPending(false);
    }
  };
  const refusal = failed ? errorWords(failed) : null;
  return (
    <FormPage
      title="Switch Role"
      sub={<>Take on a role&apos;s permissions for a while, then switch back. <span className="text-muted-foreground">The role comes from AssumeRole, which returns temporary credentials for it.</span></>}
      crumbs={[{ label: "Resources", to: "/resources" }, { label: "Identity", to: "/resources" }, { label: "STS", to: "/resources/sts" }]}
      request={{ service: "sts", operation: "AssumeRole", input }}
      callService={false}
      callNote={<>AssumeRole returns fresh credentials, an access key ID, a secret and a session token. DurationSeconds defaults to 3600 and can range from 900 to 43,200 (and no more than the role&apos;s maximum session duration, which AssumeRole enforces). <span className="mt-2 block">The session is real to SDKs; the local gateway accepts any signature, so what it can do follows the IAM mode: with soft the role&apos;s policies are checked and a refusal is only recorded, with enforce it is refused. The trust policy is evaluated only under enforce.</span></>}
      refusal={refusal ? { code: (failed instanceof ApiFailure ? failed.code : "Error"), message: refusal.message } : null}
      banner={done ? <Alert tone="good" title={`Switched to ${done}.`} testId="switched">AssumeRole answered. The identity menu in the top bar shows the role; Switch back returns you to your own identity.</Alert> : refusal ? <Alert tone="bad" title={`AssumeRole was refused${failed instanceof ApiFailure ? `: ${failed.code}` : ""}`} testId="switch-refused">{refusal.message}</Alert> : undefined}
      submitLabel="Switch Role"
      pending={pending}
      onSubmit={() => void submit()}
      onCancel={() => history.back()}
      testId="switch-role"
    >
      {role.trim() && seen && last && <p className="max-w-[56rem] text-[15px] leading-6 text-muted-foreground" data-testid="last-assumed">The role <b className="font-mono font-semibold text-bright">{role.trim()}</b> was last assumed at <b className="font-mono font-semibold text-bright">{new Date(last.at_ms).toISOString().slice(11, 16)}</b>{last.caller && keyNames.get(last.caller) ? <>, with <b className="font-mono font-semibold text-bright">{keyNames.get(last.caller)}</b>&apos;s key</> : null}.</p>}
      <FormSection title="Switch Role" description="The role's administrator gives you the account and the role name. The role link on the role's Summary page in IAM fills in this form." testId="section-switch">
        <Field label="Account ID or alias" error={tried ? (accountProblem ?? undefined) : undefined} help={<>The 12-digit account ID or the account alias.{alias ? <> <span className="font-mono">{alias}</span> is the alias of <span className="font-mono">{account}</span>.</> : null}</>} testId="field-account">
          {(fid) => <Input id={fid} value={account_} onChange={(e) => setAcct(e.target.value)} autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="switch-account" />}
        </Field>
        <Field label="Role name" error={tried || external ? (roleProblem ?? undefined) : undefined} help={<>Role names are case sensitive here. If the role was created with a path, type the whole path and name, such as <span className="font-mono">division_abc/subdivision_efg/roleToDoX</span>; path and name together must stay within 64 characters or the switch fails.{role.trim() ? <> <span className="font-mono">{role.trim()}</span> {role.includes("/") ? "has a path" : "has no path"}.</> : null}</>} hint={role.trim() && roles.isSuccess && !known && !external ? `IAM lists no role named ${role.trim()} in this account. With IAM off or soft, AssumeRole still answers and the session is for a role IAM does not know.` : undefined} testId="field-role">
          {(fid) => <Input id={fid} value={role} onChange={(e) => setRole(e.target.value)} placeholder="report-job" autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="switch-role-name" />}
        </Field>
        <Field label="Display name" optional help="Shown in the top bar in place of your identity while the role is active. A name is suggested from the account and the role, and you can change it." testId="field-display">
          {(fid) => <Input id={fid} value={shownName} onChange={(e) => setDisplay(e.target.value)} autoComplete="off" className="bg-card" data-testid="switch-display" />}
        </Field>
        <Field label="Display color" optional help="A color highlights the display name in the top bar and reminds you that your permissions have changed, for example green for a test role and red for production." testId="field-color">
          <Segmented value={color} onChange={setColor} label="Display color" options={[{ value: "red", label: "Red", dot: "bg-bad" }, { value: "green", label: "Green", dot: "bg-good" }]} testId="switch-color" />
        </Field>
      </FormSection>
      <Callout tone="info" lead="Roles that cannot be switched to here." testId="external-note">The console cannot switch to a role whose trust policy requires an external ID; call AssumeRole with <span className="font-mono">ExternalId</span> from the CLI or an SDK instead.</Callout>
    </FormPage>
  );
}
