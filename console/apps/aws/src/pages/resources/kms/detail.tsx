// A key's page (kms__key-detail and its tabs): the alias as the title with the key's state and kind, what it says of itself, what uses
// it, the General configuration, and the tabs (Key policy, Cryptographic configuration, Tags, Key material and rotations, Aliases)
// each with the calls that touched the key. The key policy's edit and the rotation's edit are pages of their own (`?view=`).
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { useUrlParam } from "@doze/ui/hooks/use-url-param";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { callKindOp, DeletionWindow, DetailError, RecentCalls, ResourcePage, ResourcePageSkeleton, TagsTab, useTags } from "../../../kit";
import { useKeyUses } from "../cfg-api";
import { fieldOf } from "../cfg-parts";
import { KEYS, useRotations, useRotationStatus } from "./api";
import { GeneralConfiguration, KeySay, KeyStrip } from "./overview";
import { PolicyEdit } from "./policy-edit";
import { RotationEdit } from "./rotation-edit";
import { TabAliases } from "./tab-aliases";
import { TabCrypto } from "./tab-crypto";
import { TabPolicy } from "./tab-policy";
import { TabRotation } from "./tab-rotation";
import { aliasLabel, countsSentence, keyName, keySentence, kindOf, refsOf, stateWords, usedBy } from "./model";
import { timeMs } from "../model";

export function KmsDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const { refetch } = detail;
  const qc = useQueryClient();
  const refresh = () => {
    void refetch();
    void qc.invalidateQueries({ queryKey: ["resources", "kindop", "kms"] });
    void qc.invalidateQueries({ queryKey: ["resources", "tags", "kms", "keys", name] });
  };
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={name} error={detail.error} onRetry={refresh} retrying={detail.isFetching} />;
  if (!detail.data) return <ResourcePageSkeleton />;
  return <KeyView d={detail.data} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refresh={refresh} />;
}

function KeyView({ d, updatedAt, fetching, refresh }: { d: ResourceDetail; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const search = useSearch({ strict: false }) as { view?: string; done?: string };
  const navigate = useNavigate();
  const qc = useQueryClient();
  const id = fieldOf(d, "Key ID") ?? d.id;
  const t = { service: "kms", kind: "keys", name: d.name };
  const state = fieldOf(d, "State") ?? "";
  const spec = fieldOf(d, "Key spec") ?? "";
  const kind = kindOf(spec);
  const aliasRows = d.subs.find((s) => s.title === "Aliases")?.rows ?? [];
  const aliases = aliasRows.map((r) => aliasLabel(r.name)).sort();
  const label = aliases[0] ? `alias/${aliases[0]}` : id;
  const title = keyName({ aliases, id });
  const symmetric = kind === "Symmetric" && state !== "PendingDeletion";
  const status = useRotationStatus(id, symmetric);
  const rotations = useRotations(id, symmetric);
  const tags = useTags(t);
  const { uses } = useKeyUses();
  const used = useMemo(() => usedBy(uses, refsOf({ id, aliases }, fieldOf(d, "ARN") ?? undefined)), [uses, id, aliases, d]);
  const [tab, setTab] = useUrlParam("tab");
  const [restoring, setRestoring] = useState(false);
  const [restored, setRestored] = useState(false);
  const deletion = fieldOf(d, "Deletion date");
  const go = (view?: string) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: d.service, kind: d.kind, name: d.name }, search: view ? { view } : {} });
  const back = (to?: string) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: d.service, kind: d.kind, name: d.name }, search: to ? { tab: to } : {} });
  const facts = { state, stateWords: stateWords(state), deletion, kind };
  const last = rotations.data?.Rotations?.[0];
  const sentence = keySentence(label, facts, symmetric && status.data && rotations.data ? { enabled: status.data.KeyRotationEnabled, days: status.data.RotationPeriodInDays ?? null, last: last ? { type: last.RotationType ?? "", at: String(last.RotationDate ?? "") } : null, count: rotations.data.Rotations.length } : null);
  const shown: ResourceDetail = { ...d, name: title, sentence: null, wired: [] };
  const restore = async () => {
    setRestoring(true);
    try {
      await callKindOp(KEYS, "CancelKeyDeletion", { KeyId: id });
      setRestored(true);
      await qc.invalidateQueries({ queryKey: ["resources"] });
    } finally {
      setRestoring(false);
    }
  };
  if (search.view === "policy") return <PolicyEdit d={shown} id={id} state={state} back={() => back("policy")} />;
  if (search.view === "rotation") return <RotationEdit d={shown} id={id} state={state} current={status.data ?? null} back={() => back("rotation")} />;
  const calls = <RecentCalls calls={d.calls} noun="key" className="mt-10" />;
  const tagCount = tags.data ? tags.data.tags.length : null;
  const tabs = [
    { id: "policy", label: "Key policy", render: () => <><TabPolicy id={id} editable={state !== "PendingDeletion" && !fieldOf(d, "Managed by")?.startsWith("AWS")} go={() => go("policy")} />{calls}</> },
    { id: "crypto", label: "Cryptographic configuration", render: () => <><TabCrypto d={d} id={id} kind={kind} />{calls}</> },
    { id: "tags", label: "Tags", render: () => <>{<TagsTab target={t} noun="tag" explain="Tags are key-value labels for organising keys." subject="This key" />}{calls}</> },
    { id: "rotation", label: "Key material and rotations", render: () => <><TabRotation id={id} kind={kind} state={state} status={status.data ?? null} rotations={rotations.data?.Rotations ?? null} loading={status.isPending || rotations.isPending} go={() => go("rotation")} />{calls}</> },
    { id: "aliases", label: "Aliases", render: () => <><TabAliases id={id} title={title} aliases={aliasRows.map((r) => ({ name: aliasLabel(r.name), arn: r.cells[1] ?? "" }))} />{calls}</> },
  ];
  const banner = search.done === "created" ? <ResultBanner title={`Key ${title} created.`} returned={`CreateKey made the key${aliases[0] ? ` and CreateAlias gave it the alias ${aliases[0]}` : ""}; it is Enabled.`} onDismiss={() => back()} testId="created-banner" />
    : state === "PendingDeletion" && !restored ? <DeletionWindow mode="scheduled" noun="key" on={timeMs(deletion ?? "") ?? 0} onRestore={() => void restore()} restoring={restoring} />
    : restored ? <ResultBanner title={`Deletion of ${title} cancelled.`} returned="CancelKeyDeletion put the key back in Disabled, not Enabled; choose Enable to use it." testId="restored-banner" /> : undefined;
  return (
    <ResourcePage
      detail={shown}
      meta={<>Created {fieldOf(d, "Created") ? dateTimeUtc(timeMs(fieldOf(d, "Created")!) ?? 0) : "–"}. {countsSentence(aliases.length, tagCount)}</>}
      banner={banner}
      overview={<><KeySay text={sentence} label={label} uses={used} /><KeyStrip name={title} uses={used} aliases={aliases} /><GeneralConfiguration id={id} arn={fieldOf(d, "ARN")} state={state} stateWords={stateWords(state)} created={fieldOf(d, "Created")} description={fieldOf(d, "Description")} /></>}
      tabs={tabs}
      tab={tab ?? "policy"}
      onTab={(x) => setTab(x)}
      details={false}
      refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
      testId="key-page"
    />
  );
}

