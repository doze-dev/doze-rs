// A state machine's page (stepfunctions__machine-detail, machine-created, executions-none, machine-delete): the header with Edit, Actions
// and Start execution, the sentence (what is running, the last to fail), the Wired to strip (who starts it, what its Tasks call),
// the Details card, and the tabs Executions, Definition, Versions, Aliases and Tags. Edit, Create alias and Manage tags are pages of
// their own (`?do=`); Delete and Start execution are dialogs.
import { useQueryClient } from "@tanstack/react-query";
import { useSearch } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Callout } from "@doze/ui/components/app/callout";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { useDelete, useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { DetailError, FlowStrip, type FlowChip, PlainConfirm, ResourcePage, ResourcePageSkeleton, TagsView, useBlockers, useFeeders, useSetTags, useTags } from "../../../kit";
import { type Verb } from "../../../kit";
import { StartDialog } from "./dialogs";
import { DefinitionTab } from "./definition-tab";
import { DetailsCard, fieldOf } from "./details-card";
import { MachineExecutions } from "./executions-tab";
import { at } from "../deep/parts";
import { useGoTo, useMachineRuns } from "./hooks";
import { inputSkeleton, machineSentence, wiresOf } from "./model";
import { parseDefinition } from "./graph";
import { AliasesTab, VersionsTab } from "./versions-tab";
import { TagsPanel } from "./tags-page";

const TABS = [
  ["executions", "Executions"],
  ["definition", "Definition"],
  ["versions", "Versions"],
  ["aliases", "Aliases"],
  ["tags", "Tags"],
] as const;

/** What was just done, said as the board's banner. */
const DONE: Record<string, (name: string) => { title: string; returned: string }> = {
  created: (n) => ({ title: `Your state machine ${n} has been created.`, returned: "CreateStateMachine returned its ARN and creation date." }),
  "created-role": (n) => ({ title: `Your state machine ${n} and a new IAM role have been created.`, returned: "CreateRole made the role, PutRolePolicy gave it what the tasks call, and CreateStateMachine named it." }),
  saved: (n) => ({ title: `${n} was saved.`, returned: "UpdateStateMachine made a new revision. Running executions keep the definition and role they started with." }),
  published: (n) => ({ title: `A version of ${n} was published.`, returned: "PublishStateMachineVersion made an immutable snapshot of the saved revision." }),
  "alias-created": () => ({ title: "Alias created.", returned: "CreateStateMachineAlias pointed the alias at its versions." }),
  tags: () => ({ title: "Tags saved.", returned: "TagResource and UntagResource made the change." }),
};

export function MachineDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const { refetch } = detail;
  const qc = useQueryClient();
  const refresh = () => {
    void refetch();
    void qc.invalidateQueries({ queryKey: ["resources", "deep", service, kind, name] });
  };
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={name} error={detail.error} onRetry={refresh} retrying={detail.isFetching} />;
  if (!detail.data) return <ResourcePageSkeleton />;
  return <MachineView d={detail.data} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refresh={refresh} />;
}

function MachineView({ d, updatedAt, fetching, refresh }: { d: ResourceDetail; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const go = useGoTo();
  const search = useSearch({ strict: false }) as { tab?: string; done?: string };
  const [dialog, setDialog] = useState<"delete" | "start" | null>(null);
  const [opened, setOpened] = useState(0);
  const express = fieldOf(d, "Type") === "EXPRESS";
  const text = fieldOf(d, "Definition");
  const definition = useMemo(() => (text ? parseDefinition(text) : null), [text]);
  const runs = useMachineRuns(d.name, express);
  const feeders = useFeeders(d.calls, ["StartExecution"]);
  const wires = useMemo(() => wiresOf(definition ?? undefined), [definition]);
  const tab = TABS.some(([id]) => id === search.tab) ? (search.tab as (typeof TABS)[number][0]) : "executions";
  const done = search.done ? DONE[search.done]?.(d.name) : undefined;
  const sentence = machineSentence(d.name, express, runs.data, definition?.StartAt, fieldOf(d, "Logging") ?? "OFF");
  const clear = () => go.machine(d.name, search.tab ? { tab: search.tab } : {});
  const open = (to: "delete" | "start") => {
    setOpened((n) => n + 1);
    setDialog(to);
  };
  const onVerb = (v: Verb) => {
    if (v.id === "edit") go.machine(d.name, { do: "edit" });
    else if (v.id === "start") open("start");
  };
  const chips = (names: string[]): FlowChip[] => names.map((label) => ({ label }));
  const targets: FlowChip[] = wires.map((w) => ({ label: w.name, to: w }));
  const meta = <>Created <span data-testid="header-created">{at(fieldOf(d, "Creation date"))}</span>. Revision <span className="font-mono" data-testid="header-revision">{(fieldOf(d, "Revision ID") ?? "").slice(0, 8)}</span>.</>;
  const shown: ResourceDetail = { ...d, sentence: null, wired: [] };
  const tabs = [
    { id: "executions", label: "Executions", render: () => <MachineExecutions d={d} /> },
    { id: "definition", label: "Definition", render: () => <DefinitionTab definition={definition} text={text ?? ""} onEdit={() => go.machine(d.name, { do: "edit" })} /> },
    { id: "versions", label: "Versions", render: () => <VersionsTab d={d} /> },
    { id: "aliases", label: "Aliases", render: () => <AliasesTab d={d} /> },
    { id: "tags", label: "Tags", render: () => <TagsPanel d={d} /> },
  ];
  return (
    <>
      <ResourcePage
        detail={shown}
        meta={meta}
        banner={done ? <ResultBanner title={done.title} returned={done.returned} onDismiss={clear} testId="done-banner" /> : undefined}
        bannerAbove
        overview={
          <>
            {sentence && <p className="mb-6 max-w-[62rem] text-[15px] leading-6 text-muted-foreground" data-testid="detail-sentence"><Emphasis text={sentence} names={[d.name, runs.data?.failed?.name ?? "", runs.data?.failed?.state ?? "", runs.data?.failed?.error ?? "", definition?.StartAt ?? ""].filter(Boolean)} bold={[/\b\d+ (?=executions? running)/]} /></p>}
            <FlowStrip groups={[{ label: "Wired to", chips: chips(feeders) }, { chips: [{ label: d.name, self: true }] }, { chips: targets }]} />
            <DetailsCard d={d} />
          </>
        }
        tabs={tabs}
        tab={tab}
        onTab={(id) => go.machine(d.name, id === "executions" ? {} : { tab: id })}
        onVerb={onVerb}
        primary="start"
        primaryLast
        actions={[{ label: "Delete", onSelect: () => open("delete") }]}
        details={false}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        testId="machine-page"
      />
      <StartDialog key={`s${opened}`} machine={d.id} name={d.name} startAt={definition?.StartAt} input={inputSkeleton(definition)} open={dialog === "start"} onOpenChange={(o) => !o && setDialog(null)} />
      <DeleteMachine d={d} open={dialog === "delete"} onOpenChange={(o) => !o && setDialog(null)} feeders={feeders} />
    </>
  );
}

/** Delete state machine (stepfunctions__machine-delete): what goes with it, what is running, and how doze-aws differs from AWS. */
function DeleteMachine({ d, open, onOpenChange, feeders }: { d: ResourceDetail; open: boolean; onOpenChange: (o: boolean) => void; feeders: string[] }) {
  const del = useDelete("stepfunctions", "machines");
  const blockers = useBlockers({ service: d.service, kind: d.kind, name: d.name }, open);
  const go = useGoTo();
  const count = (title: string) => blockers.data?.blockers.find((b) => b.title === title)?.items.length ?? 0;
  const versions = count("Versions");
  const aliases = count("Aliases");
  const running = count("Running executions");
  const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "s"}`;
  const goes = [versions > 0 ? plural(versions, "version") : "", aliases > 0 ? plural(aliases, "alias").replace("aliass", "aliases") : ""].filter(Boolean).join(" and ");
  const starter = feeders[0];
  return (
    <PlainConfirm
      open={open}
      onOpenChange={(o) => { onOpenChange(o); if (!o) del.reset(); }}
      title="Delete state machine"
      description={<>Deleting <b className="font-mono font-semibold text-bright">{d.name}</b>{goes ? <> also deletes its <b className="font-semibold">{goes.replace("aliass", "aliases")}</b></> : null}.</>}
      verb="Delete state machine"
      call="DeleteStateMachine"
      pending={del.isPending}
      error={del.error}
      onConfirm={() => del.mutate(d.name, { onSuccess: () => { onOpenChange(false); go.list({ gone: d.name }); } })}
      testId="delete-dialog"
    >
      {running > 0 && (
        <Callout tone="warn" testId="delete-running">
          <b className="font-semibold">{plural(running, "execution")} {running === 1 ? "is" : "are"} running.</b>{starter ? <> <span className="font-semibold">{starter}</span> starts this machine, and its next StartExecution will find no state machine.</> : null}
        </Callout>
      )}
      <p className="border-l-2 border-input pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="delete-difference">
        doze-aws deletes the state machine at once and does not stop what is running: those executions carry on with the definition they started with. AWS instead holds it in the Deleting state until its executions finish, and ends the running ones at their next state transition.
      </p>
    </PlainConfirm>
  );
}

export { useSetTags, useTags, TagsView };
