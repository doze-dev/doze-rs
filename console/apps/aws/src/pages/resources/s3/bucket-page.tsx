// A bucket: its header (Region, when it was made, versioning), a sentence for the tab on screen, what it is wired to, and four
// tabs, Objects, Properties, Permissions and Management, each a page of what is set on it and each setting's Edit a page of its own.
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { DetailError, ResourcePage, ResourcePageSkeleton } from "../../../kit";
import { Calls } from "../detail-page";
import { deepKey } from "../deep/api";
import type { OwnProps } from "../deep/registry";
import { bucketOf, useBucket, useBucketNav } from "./api";
import { ManagementTab, sentenceOf as managementSentenceOf, useManagement } from "./management";
import { ObjectsTab } from "./objects";
import { FlashBanner, useFlash } from "./parts";
import { PermissionsTab, sentenceOf as permissionsSentenceOf, usePermissions } from "./permissions";
import { PropertiesTab, sentenceOf as propertiesSentenceOf, useProperties } from "./properties";
import { longDate, regionWords, versioningWords, wiredChip, callKey } from "./words";

const TABS = ["objects", "properties", "permissions", "management"] as const;
type Tab = (typeof TABS)[number];

export function BucketPage({ name, search }: OwnProps) {
  const bucket = useBucket(name!);
  const go = useBucketNav(name!);
  const qc = useQueryClient();
  const [flash, dismiss] = useFlash();
  // A prefix searched for that matched nothing: the Objects tab says so with nothing around it (no sentence, strip or calls).
  const [lean, setLean] = useState(false);
  const tab: Tab = TABS.find((t) => t === search.tab) ?? "objects";
  const props = useProperties(name!, tab === "properties");
  const perms = usePermissions(name!, tab === "permissions");
  const mgmt = useManagement(name!, tab === "management");
  const d = bucket.detail;
  if (bucket.isError && !d) return <DetailError service="s3" kind="buckets" name={name!} error={bucket.error} onRetry={() => void bucket.refetch()} retrying={bucket.isFetching} />;
  if (!d || !bucket.versioning) return <ResourcePageSkeleton />;
  const bare = tab === "objects" && lean;
  const sentence = bare ? "" : tab === "properties" ? propertiesSentenceOf(name!, props) : tab === "permissions" ? permissionsSentenceOf(name!, perms) : tab === "management" ? managementSentenceOf(name!, mgmt, bucket.versioning) : d.sentence;
  // The Wired to strip is the Objects tab's: the board draws it there only.
  const detail = { ...d, name: name!, sentence, wired: tab === "objects" && !bare ? d.wired : [], badges: [{ text: regionWords(bucket.region), tone: "neutral" as const }], verbs: [], deep: [] };
  const refresh = () => {
    void bucket.refetch();
    void qc.invalidateQueries({ queryKey: deepKey(bucketOf(name!)) });
    void qc.invalidateQueries({ queryKey: ["resources", "tags"] });
  };
  const here = (action: string) => go({ do: action, ...(tab === "objects" && search.p ? { p: search.p } : {}) });
  return (
    <ResourcePage
      detail={detail}
      details={false}
      meta={<>Created {longDate(bucket.created)}. Versioning is {versioningWords(bucket.versioning)}.</>}
      wiredSelf={0}
      wiredText={(w) => wiredChip(w.label, w.to.name)}
      banner={flash ? <FlashBanner flash={flash} dismiss={dismiss} /> : undefined}
      tab={tab}
      onTab={(id) => go(id === "objects" ? {} : { tab: id })}
      actions={[
        { label: "Empty bucket", onSelect: () => go({ do: "empty" }) },
        { label: "Delete bucket", onSelect: () => go({ do: "delete" }) },
      ]}
      refresh={<RefreshControl updatedAt={bucket.dataUpdatedAt} fetching={bucket.isFetching} onRefresh={refresh} interval={false} />}
      tabs={[
        {
          id: "objects",
          label: "Objects",
          render: () => (
            <div className="grid gap-6">
              <ObjectsTab name={name!} search={search} onLean={setLean} />
              {!lean && <Calls d={d} note={(c) => callKey(name!, c.resource)} />}
            </div>
          ),
        },
        { id: "properties", label: "Properties", render: () => <PropertiesTab name={name!} region={bucket.region} created={bucket.created} p={props} go={here} /> },
        { id: "permissions", label: "Permissions", render: () => <PermissionsTab name={name!} p={perms} go={here} /> },
        { id: "management", label: "Management", render: () => <ManagementTab name={name!} q={mgmt} go={here} /> },
      ]}
    />
  );
}

