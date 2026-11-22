// A secret's Overview tab (secretsmanager__secret-overview, secret-value): the details with their Actions, the value (retrieved on
// asking, as key and value rows with the version it came from), the resource permissions and the code that reads the secret.
import { EyeIcon, EyeOffIcon, PencilIcon } from "lucide-react";
import { useState } from "react";
import { ChevronDownIcon } from "lucide-react";
import { Alert } from "@doze/ui/components/app/alert";
import { CodeBlock } from "@doze/ui/components/app/code-block";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { DataTable } from "@doze/ui/components/app/data-table";
import { Button } from "@doze/ui/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useAbout } from "../../../api/queries";
import { callKindOp, Segmented } from "../../../kit";
import { Card, Facts, Note } from "../cfg-parts";
import { SECRETS, type SecretValue, usePolicy } from "./api";
import { shortId, valueRows } from "./model";
import { refusalOf } from "../kms/schedule";

export function DetailsCard({ name, arn, keyId, description, onAction }: { name: string; arn: string | null; keyId: string | null; description: string | null; onAction: (a: "description" | "key" | "delete") => void }) {
  return (
    <Card title="Secret details" testId="secret-details" actions={
      <DropdownMenu>
        <DropdownMenuTrigger asChild><Button type="button" variant="outline" className="bg-card" data-testid="secret-actions">Actions <ChevronDownIcon /></Button></DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-52">
          <DropdownMenuItem onSelect={() => onAction("description")} data-testid="action-description">Edit description</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => onAction("key")} data-testid="action-key">Edit encryption key</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => onAction("delete")} className="text-bad focus:text-bad" data-testid="action-delete">Delete secret</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    }>
      <Facts facts={[
        { label: "Secret name", value: <span className="font-mono text-[13px]">{name}</span> },
        { label: "ARN", value: <span className="font-mono text-[13px]">{arn ?? "–"}</span> },
        { label: "Encryption key", value: <span className="font-mono text-[13px]">{keyId ?? "aws/secretsmanager"}</span> },
        { label: "Description", value: description || <span className="text-muted-foreground">–</span> },
      ]} />
    </Card>
  );
}

/** `onRead`: the value has been read, so the call is on the record and the page asks for its calls again. */
export function ValueCard({ name, scheduled, onEdit, onRead }: { name: string; scheduled: boolean; onEdit: () => void; onRead?: () => void }) {
  const [value, setValue] = useState<SecretValue | null>(null);
  const [pending, setPending] = useState(false);
  const [refused, setRefused] = useState<{ code: string; message: string } | null>(null);
  const retrieve = async () => {
    setPending(true);
    setRefused(null);
    try {
      setValue(await callKindOp<SecretValue>(SECRETS, "GetSecretValue", { SecretId: name }));
      onRead?.();
    } catch (e) {
      setRefused(refusalOf(e));
    } finally {
      setPending(false);
    }
  };
  const text = value?.SecretString ?? null;
  const rows = text === null ? null : valueRows(text);
  return (
    <Card title="Secret value" testId="secret-value" actions={value && !scheduled ? <><Button type="button" variant="outline" className="bg-card" onClick={() => setValue(null)} data-testid="value-hide"><EyeOffIcon /> Hide</Button><Button type="button" variant="outline" className="bg-card" onClick={onEdit} data-testid="value-edit"><PencilIcon /> Edit</Button></> : undefined}>
      {scheduled ? (
        <p className="text-[13.5px] text-muted-foreground" data-testid="value-scheduled">You can&apos;t retrieve the value of a secret that is scheduled for deletion. Cancel the deletion first, then retrieve it.</p>
      ) : value === null ? (
        <>
          <p className="text-[13.5px] text-muted-foreground">Choose Retrieve secret value to see the current version, AWSCURRENT. The call appears in the last calls below.</p>
          <div><Button type="button" variant="outline" className="bg-card" onClick={() => void retrieve()} disabled={pending} data-testid="value-retrieve"><EyeIcon /> {pending ? "Retrieving…" : "Retrieve secret value"}</Button></div>
        </>
      ) : (
        <>
          <p className="text-[13.5px] text-muted-foreground" data-testid="value-version">Retrieved just now: version <span className="font-mono text-bright">{shortId(value.VersionId)}</span>, labeled {(value.VersionStages ?? []).join(", ") || "no label"}.</p>
          {rows ? (
            <DataTable aria-label="Secret value" data={rows} rowId={(r) => r.key} maxHeight={320} columns={[{ id: "k", header: "Key", accessorKey: "key", size: 260, meta: { mono: true } }, { id: "v", header: "Value", accessorKey: "value", meta: { mono: true } }]} />
          ) : text !== null ? (
            <div className="relative"><CopyButton text={text} label="Copy value" what="Copied" iconOnly variant="ghost" className="absolute top-1.5 right-1.5 z-10" /><CodeBlock text={text} testId="value-plain" /></div>
          ) : <Note>This secret holds binary data, which the console does not show.</Note>}
          <Note>The console shows only the AWSCURRENT version. To read AWSPREVIOUS or a custom label, call GetSecretValue with VersionStage from the AWS CLI or an SDK.</Note>
        </>
      )}
      {refused && <Alert tone="bad" title={`GetSecretValue was refused: ${refused.code}`} testId="value-refused">{refused.message}</Alert>}
    </Card>
  );
}

export function PolicyCard({ name, scheduled, onEdit }: { name: string; scheduled: boolean; onEdit: (current: string | null) => void }) {
  const q = usePolicy(name, !scheduled);
  const text = q.data?.ResourcePolicy ?? null;
  let pretty = text;
  try { pretty = text ? JSON.stringify(JSON.parse(text), null, 2) : null; } catch { /* as stored */ }
  return (
    <Card title="Resource permissions" testId="secret-policy" lede="The resource-based policy attached to this secret." actions={<Button type="button" variant="outline" className="bg-card" disabled={q.isPending || scheduled} onClick={() => onEdit(text)} data-testid="policy-edit-open"><PencilIcon /> Edit permissions</Button>}>
      {q.isPending && !scheduled ? <Skeleton className="h-24 rounded-xl" /> : pretty ? <CodeBlock text={pretty} max="max-h-[22rem]" testId="policy-json" /> : <p className="text-[13.5px] text-muted-foreground" data-testid="policy-none">{scheduled ? "The policy cannot be read while the secret is scheduled for deletion." : "No resource policy is attached. You can add one with Edit permissions."}</p>}
    </Card>
  );
}

const SAMPLES = [{ value: "python", label: "Python" }, { value: "javascript", label: "JavaScript" }, { value: "cli", label: "AWS CLI" }] as const;

export function SampleCode({ name }: { name: string }) {
  const about = useAbout().data;
  const [lang, setLang] = useState<(typeof SAMPLES)[number]["value"]>("python");
  const endpoint = about?.endpoint ?? "http://127.0.0.1:4566";
  const region = about?.region ?? "us-east-1";
  const text = {
    python: `import boto3\nclient = boto3.client(\n    "secretsmanager",\n    region_name="${region}",\n    endpoint_url="${endpoint}",\n)\nresponse = client.get_secret_value(SecretId="${name}")\nsecret = response["SecretString"]`,
    javascript: `import { SecretsManagerClient, GetSecretValueCommand } from "@aws-sdk/client-secrets-manager";\nconst client = new SecretsManagerClient({ region: "${region}", endpoint: "${endpoint}" });\nconst response = await client.send(new GetSecretValueCommand({ SecretId: "${name}" }));\nconst secret = response.SecretString;`,
    cli: `aws secretsmanager get-secret-value \\\n  --endpoint-url ${endpoint} \\\n  --secret-id ${name}`,
  }[lang];
  return (
    <Card title="Sample code" testId="sample-code" lede="Choose a programming language and copy the code snippet that reads this secret with GetSecretValue. The endpoint line points the SDK at doze-aws instead of AWS." actions={<Segmented label="Language" value={lang} onChange={setLang} options={SAMPLES} testId="sample-lang" />}>
      <div className="relative"><CopyButton text={text} label="Copy" what="Copied" iconOnly variant="ghost" className="absolute top-1.5 right-1.5 z-10" /><CodeBlock text={text} testId="sample-text" /></div>
    </Card>
  );
}
