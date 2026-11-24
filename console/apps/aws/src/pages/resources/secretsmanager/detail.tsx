// A secret's page (secretsmanager__secret-overview, secret-value, secret-rotation, secret-versions, secret-tags, secret-deleted): the name
// with its rotation and deletion state, what it says of itself and who reads it, and the tabs Overview, Rotation, Versions and Tags.
// A secret scheduled for deletion says so, offers Cancel deletion (RestoreSecret) and cannot be read.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { useUrlParam } from "@doze/ui/hooks/use-url-param";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { callKindOp, DetailError, RecentCalls, ResourcePage, ResourcePageSkeleton, TagsTab } from "../../../kit";
import { ago } from "../../../people";
import { useAllRows, useReaders } from "../cfg-api";
import { fieldOf, Say } from "../cfg-parts";
import { SECRETS, useDescribed } from "./api";
import { DeleteSecretDialog, EditDescription, EditKey, EditPolicy } from "./dialogs";
import { dayWords, functionName, rotationSentence } from "./model";
import { DetailsCard, PolicyCard, SampleCode, ValueCard } from "./overview";
import { TabRotation } from "./tab-rotation";
import { TabVersions } from "./tab-versions";
import { ValueEdit } from "./value-edit";
import { Link } from "@tanstack/react-router";
import { ArrowRightIcon } from "lucide-react";

export function SecretDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const { refetch } = detail;
  const qc = useQueryClient();
  const refresh = () => {
    void refetch();
    void qc.invalidateQueries({ queryKey: ["resources", "kindop", "secretsmanager"] });
  };
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={name} error={detail.error} onRetry={refresh} retrying={detail.isFetching} />;
  if (!detail.data) return <ResourcePageSkeleton />;
  return <SecretView d={detail.data} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refresh={refresh} />;
}

const chip = "inline-flex min-w-0 max-w-full items-center rounded-lg border bg-well px-3 py-1.5 font-mono text-[13px] text-bright";

function SecretView({ d, updatedAt, fetching, refresh }: { d: ResourceDetail; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const search = useSearch({ strict: false }) as { view?: string; done?: string; until?: string };
  const navigate = useNavigate();
  const qc = useQueryClient();
  const name = d.name;
  const t = { service: d.service, kind: d.kind, name };
  const desc = useDescribed(name);
  const aliases = useAllRows("kms", "aliases");
  const readers = useReaders(d.calls, ["GetSecretValue"]);
  const [now] = useState(() => Date.now());
  const [tab, setTab] = useUrlParam("tab");
  const [dialog, setDialog] = useState<"description" | "key" | "delete" | "policy" | null>(null);
  const [policy, setPolicy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [restoring, setRestoring] = useState(false);
  const info = desc.data;
  const scheduled = Boolean(info?.DeletedDate);
  const keyId = info?.KmsKeyId ?? fieldOf(d, "Encryption key");
  const keyTarget = useMemo(() => (keyId ? (aliases.rows.find((r) => r.name === keyId)?.cells[aliases.columns.findIndex((c) => c.title === "Key")] ?? (keyId.includes("/") ? null : keyId)) : null), [keyId, aliases.rows, aliases.columns]);
  const go = (search?: { view?: string; done?: string; until?: string; tab?: string }) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: d.service, kind: d.kind, name }, search: search ?? {} });
  const restore = async () => {
    setRestoring(true);
    try {
      await callKindOp(SECRETS, "RestoreSecret", { SecretId: name });
      setNotice("restored");
      go();
      await qc.invalidateQueries({ queryKey: ["resources"] });
    } finally {
      setRestoring(false);
    }
  };
  if (search.view === "value") return <ValueEdit d={d} back={(done) => go(done ? { done } : undefined)} />;
  const rotation = info ? rotationSentence(info) : null;
  const read = readers.last !== null ? `${readers.who.join(" and ")} read${readers.who.length === 1 ? "s" : ""} this secret ${ago(readers.last, now)}, ${readers.count} time${readers.count === 1 ? "" : "s"} in the recent calls.` : null;
  const sentence = scheduled ? `${name} is scheduled for deletion, so its value cannot be read.` : [read, rotation].filter(Boolean).join(" ") || null;
  const fn = info?.RotationLambdaARN ? functionName(info.RotationLambdaARN) : null;
  // A secret waiting to be deleted is drawn as the board draws it: its state alone beside the name, no rotation pill.
  const badges = scheduled ? [{ text: "Scheduled for deletion", tone: "warn" as const }] : [info?.RotationEnabled ? { text: "Rotation enabled", tone: "good" as const } : { text: "Rotation off", tone: "neutral" as const }];
  const shown: ResourceDetail = { ...d, sentence: null, wired: [], badges };
  const until = search.until && /^\d+$/.test(search.until) ? Number(search.until) : null;
  const deletedAt = info?.DeletedDate ? info.DeletedDate * 1000 : null;
  const banner = scheduled ? (
    <Alert tone="warn" title={until ? `Scheduled for deletion: the recovery window ends on ${dayWords(until)}.` : "Scheduled for deletion"} testId="secret-scheduled">
      {until ? <>Until then you can cancel the deletion with Cancel deletion. After the window Secrets Manager deletes the secret permanently, as a background task with low priority, so there is no guarantee of a specific time after {dayWords(until)}.</> : <>The deletion was scheduled on <b>{deletedAt ? dateTimeUtc(deletedAt) : "an earlier day"}</b>. Secrets Manager deletes the secret for good when its recovery window (7 to 30 days) ends; DescribeSecret says when the deletion was asked for, and the end of the window is in the answer of DeleteSecret only. Until then you can cancel the deletion with Cancel deletion.</>}
    </Alert>
  ) : notice === "restored" ? <ResultBanner title="Deletion cancelled." returned="RestoreSecret removed the mark; the secret can be read again." onDismiss={() => setNotice(null)} testId="restored-banner" />
    : search.done === "created" ? <ResultBanner title={`Secret ${name} stored.`} returned="CreateSecret returned its ARN and the id of its first version, which has the label AWSCURRENT." onDismiss={() => go()} testId="created-banner" />
    : search.done === "value" ? <ResultBanner title="A new version was saved." returned="PutSecretValue made it AWSCURRENT; the version that held the label is AWSPREVIOUS." onDismiss={() => go()} testId="value-saved" />
    : undefined;
  const calls = <RecentCalls calls={d.calls} noun="secret" title={`Last calls that touched ${name}`} className="mt-10" pin={["GetSecretValue"]} />;
  const tabs = [
    { id: "overview", label: "Overview", render: () => (
      <div className="grid gap-5">
        <DetailsCard name={name} arn={info?.ARN ?? fieldOf(d, "ARN")} keyId={keyId} description={info?.Description ?? fieldOf(d, "Description")} onAction={scheduled ? undefined : (a) => setDialog(a)} />
        <ValueCard name={name} scheduled={scheduled} onEdit={() => go({ view: "value" })} onRead={() => void qc.invalidateQueries({ queryKey: ["resources", "one", d.service, d.kind, name] })} />
        {!scheduled && <PolicyCard name={name} scheduled={scheduled} onEdit={(cur) => { setPolicy(cur); setDialog("policy"); }} />}
        {!scheduled && <SampleCode name={name} />}
        {calls}
      </div>
    ) },
    { id: "rotation", label: "Rotation", render: () => <>{info ? <TabRotation name={name} d={info} scheduled={scheduled} /> : null}{calls}</> },
    { id: "versions", label: "Versions", render: () => <><TabVersions name={name} />{calls}</> },
    { id: "tags", label: "Tags", render: () => <><TagsTab target={t} noun="tag" subject="This secret" explain="Tags are key-value labels for organising secrets and for tag-based access control." heading={{ title: "Tags", lead: "Key/value labels you attach to the secret.", rules: <>Tag keys and values are case-sensitive. A secret can have up to 50 tags. Keys are 1 to 128 characters and unique, and can&apos;t start with the reserved prefix <span className="font-mono">aws:</span>. Values are 0 to 256 characters. Tags are not encrypted, and you can&apos;t change the tags of a deleted secret. Searching the list for a tag key finds the secrets that have it.</> }} manage={{ open: "Edit tags", title: "Edit tags", lead: "Key/value labels you attach to the secret.", remove: "Remove", save: "Save", rules: <>Saving adds new keys and overwrites keys that already exist; removed rows are removed with UntagResource. Keys can&apos;t start with <span className="font-mono">aws:</span>.</> }} />{calls}</> },
  ];
  const close = () => setDialog(null);
  // The reduced page of secretsmanager__secret-deleted: no strip of who reads it, no tabs and no Actions menu; Cancel deletion is the
  // header's main button.
  const reduced = <div className="grid gap-5">{tabs[0]!.render()}</div>;
  const cancel = <Button onClick={() => void restore()} disabled={restoring} data-testid="cancel-deletion">{restoring ? "Cancelling…" : "Cancel deletion"}</Button>;
  return (
    <>
      <ResourcePage
        detail={shown}
        meta={info?.Description ?? fieldOf(d, "Description") ?? undefined}
        banner={banner}
        overview={<>
          {sentence && <Say text={sentence} names={[name, ...(fn ? [fn] : [])]} className="mb-6" testId="detail-sentence" />}
          {!scheduled && <nav aria-label="Wired to" className="mb-8 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-[var(--radius-card)] border bg-card px-5 py-4" data-testid="wired-strip">
            {readers.who.length > 0 && <><span className="text-[13.5px] text-muted-foreground">Read by</span>{readers.who.map((w) => <span key={w} className={chip}>{w}</span>)}<ArrowRightIcon aria-hidden className="size-3.5 text-input" /></>}
            <span aria-current="page" className="inline-flex items-center rounded-lg border border-primary/40 bg-pick px-3 py-1.5 font-mono text-[13px] text-bright" data-testid="wired-self">{name}</span>
            <span className="text-[13.5px] text-muted-foreground">Encrypted with</span>
            {keyTarget ? <Link to="/resources/$service/$kind/$name" params={{ service: "kms", kind: "keys", name: keyTarget }} className={chip} data-testid="wired-link">{keyId}</Link> : <span className={chip}>{keyId ?? "aws/secretsmanager"}</span>}
            {fn && <><span className="text-[13.5px] text-muted-foreground">Rotated by</span><Link to="/resources/$service/$kind/$name" params={{ service: "lambda", kind: "functions", name: fn }} className={chip} data-testid="wired-link">{fn}</Link></>}
          </nav>}
          {scheduled && reduced}
        </>}
        menu={!scheduled}
        after={scheduled ? cancel : undefined}
        tabs={scheduled ? [] : tabs}
        tab={tab ?? "overview"}
        onTab={(x) => setTab(x)}
        details={false}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        testId="secret-page"
      />
      <EditDescription open={dialog === "description"} onClose={close} name={name} current={info?.Description ?? ""} />
      <EditKey open={dialog === "key"} onClose={close} name={name} current={keyId ?? null} />
      <EditPolicy open={dialog === "policy"} onClose={close} name={name} current={policy} />
      <DeleteSecretDialog open={dialog === "delete"} onClose={close} name={name} readers={readers.who} onSaved={(a) => go({ done: "scheduled", ...(a.DeletionDate ? { until: String(a.DeletionDate * 1000) } : {}) })} />
    </>
  );
}
