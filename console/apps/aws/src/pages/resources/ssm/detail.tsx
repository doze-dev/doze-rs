// A parameter's page (ssm__parameter-detail, parameter-detail-revealed, parameter-history, parameter-labels, parameter-tags): the name with
// its type and tier, the sentence and who reads it, and the tabs Overview, History and Tags. Edit is a form of its own (`?view=edit`).
import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { ArrowRightIcon, EyeIcon, EyeOffIcon } from "lucide-react";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { useUrlParam } from "@doze/ui/hooks/use-url-param";
import { useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { callKindOp, DetailError, RecentCalls, ResourcePage, ResourcePageSkeleton, TagsTab, useTags } from "../../../kit";
import { ago } from "../../../people";
import { useReaders } from "../cfg-api";
import { Card, Facts, fieldOf, Note, Say } from "../cfg-parts";
import { refusalOf } from "../kms/schedule";
import { ParameterForm, PARAMETERS } from "./form";
import { versionLine } from "./model";
import { TabHistory, useHistory } from "./tab-history";

export function SsmDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const { refetch } = detail;
  const qc = useQueryClient();
  const refresh = () => {
    void refetch();
    void qc.invalidateQueries({ queryKey: ["resources", "kindop", "ssm"] });
  };
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={name} error={detail.error} onRetry={refresh} retrying={detail.isFetching} />;
  if (!detail.data) return <ResourcePageSkeleton />;
  return <ParameterView d={detail.data} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refresh={refresh} />;
}

const chip = "inline-flex min-w-0 max-w-full items-center rounded-lg border bg-well px-3 py-1.5 font-mono text-[13px] text-bright";

function ParameterView({ d, updatedAt, fetching, refresh }: { d: ResourceDetail; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const search = useSearch({ strict: false }) as { view?: string; done?: string; ver?: string };
  const navigate = useNavigate();
  const name = d.name;
  const t = { service: d.service, kind: d.kind, name };
  const tags = useTags(t);
  const history = useHistory(name);
  const readers = useReaders(d.calls, ["GetParameter", "GetParameters", "GetParametersByPath"]);
  const [tab, setTab] = useUrlParam("tab");
  const [value, setValue] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [refused, setRefused] = useState<{ code: string; message: string } | null>(null);
  const type = fieldOf(d, "Type") ?? "";
  const secure = type === "SecureString";
  const version = fieldOf(d, "Version") ?? "";
  const modified = fieldOf(d, "Last modified");
  const key = fieldOf(d, "Key");
  const go = (s?: { view?: string; done?: string; ver?: string }) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: d.service, kind: d.kind, name }, search: s ?? {} });
  if (search.view === "edit") return <ParameterForm d={d} />;
  const show = async () => {
    setPending(true);
    setRefused(null);
    try {
      const r = await callKindOp<{ Parameter: { Value: string } }>(PARAMETERS, "GetParameter", { Name: name, WithDecryption: true });
      setValue(r.Parameter.Value);
    } catch (e) {
      setRefused(refusalOf(e));
    } finally {
      setPending(false);
    }
  };
  const read = readers.last !== null ? ` ${readers.who.join(" and ")} read${readers.who.length === 1 ? "s" : ""} it ${readers.count} time${readers.count === 1 ? "" : "s"} in the recent calls, last ${ago(readers.last, Date.now())}.` : "";
  const sentence = secure
    ? `${name} is a SecureString at version ${version}, sealed under the KMS key ${key ?? "alias/aws/ssm"}.${value !== null ? " The value below was decrypted for this page view by a GetParameter call from the console." : ""}${read}`
    : `${name} is a ${type} at version ${version}.${read}`;
  const shown: ResourceDetail = { ...d, sentence: null, wired: [], badges: [{ text: type, tone: "neutral" }, { text: fieldOf(d, "Tier") ?? "Standard", tone: "neutral" }] };
  const calls = <RecentCalls calls={d.calls} noun="parameter" className="mt-10" />;
  const valueCell = secure ? (value === null ? <span className="font-mono" aria-label="Masked">************************</span> : <span className="font-mono text-bright" data-testid="parameter-value">{value}</span>) : <span className="font-mono" data-testid="parameter-value">{fieldOf(d, "Value") ?? "–"}</span>;
  const tabs = [
    { id: "overview", label: "Overview", render: () => (
      <div className="grid gap-5">
        <Card title="Details" lede="The fields DescribeParameters returns for this parameter." testId="parameter-details">
          <Facts columns={2} facts={[
            { label: "ARN", value: <span className="font-mono text-[13px]">{fieldOf(d, "ARN") ?? "–"}</span> },
            { label: "Name", value: <span className="font-mono text-[13px]">{name}</span> },
            { label: "Description", value: fieldOf(d, "Description") || <span className="text-muted-foreground">–</span> },
            { label: "Tier", value: fieldOf(d, "Tier") ?? "–" },
            { label: "Type", value: type },
            { label: "Data type", value: <span className="font-mono text-[13px]">{fieldOf(d, "Data type") ?? "text"}</span> },
            { label: "KMS key ID", value: secure ? <span className="font-mono text-[13px]">{key ?? "alias/aws/ssm"}</span> : <span className="text-muted-foreground">–</span> },
            { label: "Version", value: version },
            { label: "Last modified date", value: modified ? `${versionLine(version, modified).replace(/^Version \d+, last modified /, "").replace(/ UTC\.$/, "")} UTC` : "–" },
          ]} />
        </Card>
        <Card title="Value" lede={secure ? "A SecureString value is masked until you ask to see it." : `A ${type} value is shown as it is stored.`} testId="parameter-value-card" actions={secure ? (
          <>
            <Button type="button" variant="outline" className="bg-card" onClick={() => (value === null ? void show() : setValue(null))} disabled={pending} data-testid={value === null ? "value-show" : "value-hide"}>{value === null ? <><EyeIcon /> {pending ? "Reading…" : "Show"}</> : <><EyeOffIcon /> Hide</>}</Button>
            {value !== null && <CopyButton text={value} label="Copy value" what="Copied" />}
          </>
        ) : undefined}>
          <p className="break-all text-[14px]">{valueCell}</p>
          {secure && <Note>{value === null ? <>Show reads the value with GetParameter and WithDecryption set to true: KMS decrypts it with {key ?? "alias/aws/ssm"}, and the call is in Recent calls. </> : <>Decrypted by KMS with {key ?? "alias/aws/ssm"}. This is the same string that readers receive when they pass WithDecryption. </>}WithDecryption is what asks for decrypted values of SecureString parameters; for String and StringList it is ignored.</Note>}
          {refused && <Alert tone="bad" title={`GetParameter was refused: ${refused.code}`} testId="value-refused">{refused.message}</Alert>}
        </Card>
        {calls}
      </div>
    ) },
    { id: "history", label: "History", count: history.data ? history.data.Parameters.length : null, render: () => <><TabHistory name={name} />{calls}</> },
    { id: "tags", label: "Tags", count: tags.data ? tags.data.tags.length : null, render: () => <><TagsTab target={t} noun="tag" subject="This parameter" explain="Tags are key and value pairs that categorize a parameter." />{calls}</> },
  ];
  const banner = search.done === "saved" ? <ResultBanner title="Parameter saved." returned={`PutParameter returned version ${search.ver ?? version}; the earlier versions stay in History.`} onDismiss={() => go()} testId="saved-banner" /> : undefined;
  return (
    <ResourcePage
      detail={shown}
      meta={versionLine(version, modified)}
      banner={banner}
      overview={<>
        <Say text={sentence} names={[name]} className="mb-6" testId="detail-sentence" />
        {readers.who.length > 0 && (
          <nav aria-label="Wired to" className="mb-8 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-[14px] border bg-card px-5 py-4" data-testid="wired-strip">
            <span className="text-[13.5px] text-muted-foreground">Read by</span>{readers.who.map((w) => <span key={w} className={chip}>{w}</span>)}<ArrowRightIcon aria-hidden className="size-3.5 text-input" />
            <span aria-current="page" className="inline-flex items-center rounded-lg border border-primary/40 bg-pick px-3 py-1.5 font-mono text-[13px] text-bright" data-testid="wired-self">{name}</span>
            {secure && key?.startsWith("alias/") && <><span className="text-[13.5px] text-muted-foreground">Encrypted with</span><Link to="/resources/$service/$second" params={{ service: "kms", second: "keys" }} className={chip} data-testid="wired-link">{key}</Link></>}
          </nav>
        )}
      </>}
      tabs={tabs}
      tab={tab ?? "overview"}
      onTab={(x) => setTab(x)}
      details={false}
      primary="edit"
      primaryLast
      onVerb={(v) => { if (v.id === "edit") go({ view: "edit" }); }}
      refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
      testId="parameter-page"
    />
  );
}

