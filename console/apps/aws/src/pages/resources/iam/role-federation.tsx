// The two federated choices of Create role (iam__role-create): Web identity and SAML 2.0 federation. Each is a provider this account
// has registered (CreateOpenIDConnectProvider, CreateSAMLProvider) and what the trust policy needs of it: the audience of a web identity
// token, the access a SAML assertion is for. The trust policy they build is in the panel beside (`trustDocument`).
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Field, FormSection } from "../../../kit";
import { USERS, useRead } from "./api";
import { providerName, SAML_AUDIENCE, type TrustChoice } from "./model";
import { Note, RowChoice } from "./parts";

type Patch = (patch: Partial<TrustChoice>) => void;

/** Choose one of the providers: their names (the URL of an OpenID Connect provider, the name of a SAML one) and the ARN each stands for. */
function ProviderField({ label, arns, value, onChange, reading, testId }: { label: string; arns: readonly string[]; value: string; onChange: (arn: string) => void; reading: boolean; testId: string }) {
  return (
    <Field label={label}>
      {(id) => (
        <Select value={value} onValueChange={onChange} disabled={arns.length === 0}>
          <SelectTrigger id={id} className="w-full max-w-lg bg-card font-mono" data-testid={testId}><SelectValue placeholder={reading ? "Reading…" : arns.length === 0 ? "None registered" : "Choose a provider"} /></SelectTrigger>
          <SelectContent>{arns.map((a) => <SelectItem key={a} value={a} className="font-mono">{providerName(a)}</SelectItem>)}</SelectContent>
        </Select>
      )}
    </Field>
  );
}

export function WebIdentityFields({ t, set, tried, problem }: { t: TrustChoice; set: Patch; tried: boolean; problem: string | null }) {
  const providers = useRead<{ OpenIDConnectProviderList: { Arn: string }[] }>(USERS, "ListOpenIDConnectProviders", {});
  const arns = (providers.data?.OpenIDConnectProviderList ?? []).map((p) => p.Arn);
  const one = useRead<{ Url: string; ClientIDList: string[] }>(USERS, "GetOpenIDConnectProvider", { OpenIDConnectProviderArn: t.provider }, t.provider !== "");
  const clients = one.data?.ClientIDList ?? [];
  return (
    <FormSection title="Web identity" description="Choose the OpenID Connect provider whose tokens may assume the role, and the audience a token must be issued for." testId="section-web">
      <ProviderField label="Identity provider" arns={arns} value={t.provider} reading={providers.isPending} onChange={(provider) => set({ provider, audience: "" })} testId="trust-provider" />
      {providers.isSuccess && arns.length === 0 && <Note testId="no-providers">No OpenID Connect provider is registered in this account. CreateOpenIDConnectProvider registers one (its URL, the client IDs it issues tokens for, and a thumbprint); it is offered here once it exists.</Note>}
      {t.provider !== "" && (
        <Field label="Audience" error={tried && t.audience.trim() === "" ? (problem ?? undefined) : undefined} help={clients.length > 0 ? "A token's audience must be one of the provider's client IDs; the trust policy names it as the aud condition." : "The provider lists no client ID; type the audience the tokens carry."}>
          {(id) => clients.length > 0 ? (
            <Select value={t.audience} onValueChange={(audience) => set({ audience })}>
              <SelectTrigger id={id} className="w-full max-w-lg bg-card font-mono" data-testid="trust-audience"><SelectValue placeholder="Choose an audience" /></SelectTrigger>
              <SelectContent>{clients.map((c) => <SelectItem key={c} value={c} className="font-mono">{c}</SelectItem>)}</SelectContent>
            </Select>
          ) : <Input id={id} value={t.audience} onChange={(e) => set({ audience: e.target.value })} placeholder="sts.amazonaws.com" autoComplete="off" spellCheck={false} className="max-w-lg bg-card font-mono" data-testid="trust-audience" />}
        </Field>
      )}
    </FormSection>
  );
}

export function SamlFields({ t, set, tried, problem }: { t: TrustChoice; set: Patch; tried: boolean; problem: string | null }) {
  const providers = useRead<{ SAMLProviderList: { Arn: string }[] }>(USERS, "ListSAMLProviders", {});
  const arns = (providers.data?.SAMLProviderList ?? []).map((p) => p.Arn);
  return (
    <FormSection title="SAML 2.0 federation" description="Choose the SAML provider whose signed assertions may assume the role, and the access they are for." testId="section-saml">
      <ProviderField label="SAML provider" arns={arns} value={t.provider} reading={providers.isPending} onChange={(provider) => set({ provider })} testId="trust-provider" />
      {providers.isSuccess && arns.length === 0 && <Note testId="no-providers">No SAML provider is registered in this account. CreateSAMLProvider registers one from its metadata document; it is offered here once it exists.</Note>}
      {tried && t.provider === "" && problem && <p className="text-[13px] text-bad" role="alert">{problem}</p>}
      <RowChoice name="saml-access" value={t.console ? "console" : "programmatic"} onChange={(v) => set({ console: v === "console" })} options={[
        { value: "programmatic", title: "Allow programmatic access only", description: <>The trust policy names <span className="font-mono">{SAML_AUDIENCE.programmatic}</span> as the SAML:aud condition: assertions for the API.</> },
        { value: "console", title: "Allow programmatic and AWS Management Console access", description: <>It names <span className="font-mono">{SAML_AUDIENCE.console}</span>, the AWS sign-in page, as the SAML:aud condition.</> },
      ]} />
    </FormSection>
  );
}
