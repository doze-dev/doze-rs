// Delete event bus (eventbridge__bus-delete): the rules that go with it, named, and what an app that keeps sending to it still gets.
import { useNavigate } from "@tanstack/react-router";
import { PlainConfirm, useDeleteClearing } from "../../../kit";
import { list } from "./words";

export function BusDelete({ open, onOpenChange, bus, rules, archives = [] }: { open: boolean; onOpenChange: (o: boolean) => void; bus: string; rules: readonly string[]; /** The archives that take their events from this bus: they are their own resources and are not deleted with it. */ archives?: readonly string[] }) {
  const navigate = useNavigate();
  const del = useDeleteClearing({ service: "eventbridge", kind: "buses", name: bus });
  return (
    <PlainConfirm
      open={open}
      onOpenChange={(o) => { onOpenChange(o); if (!o) del.reset(); }}
      title="Delete event bus?"
      description={<>Delete <b className="font-mono font-semibold text-bright">{bus}</b>? {rules.length > 0 ? <>Deleting an event bus deletes the rules associated with that event bus: <b className="font-mono font-semibold text-bright">{list(rules)}</b>, with their targets.</> : "It has no rules."}{archives.length > 0 ? <> The {archives.length === 1 ? "archive" : "archives"} <b className="font-mono font-semibold text-bright" data-testid="bus-delete-archives">{list(archives)}</b> {archives.length === 1 ? "stays" : "stay"}, with {archives.length === 1 ? "its" : "their"} events: an archive is its own resource, and DeleteEventBus does not touch it.</> : null}</>}
      call="DeleteEventBus"
      pending={del.isPending}
      error={del.error}
      onConfirm={() => del.mutate({ clear: true }, { onSuccess: () => { void navigate({ to: "/resources/$service/$second", params: { service: "eventbridge", second: "buses" }, search: { gone: bus } }); } })}
      testId="bus-delete"
    >
      <p className="text-[13.5px] leading-[21px] text-muted-foreground">You can delete a custom or partner event bus, never the default one. AWS words this two ways: the user guide says the rules go with the bus, and the DeleteEventBus reference says all rules must be deleted first, so the console removes each rule's targets, then the rules, then the bus. An app that keeps sending to <b className="font-mono font-semibold text-foreground">{bus}</b> by name gets <b className="font-semibold text-foreground">ResourceNotFoundException</b> from PutEvents: Event bus {bus} does not exist.</p>
    </PlainConfirm>
  );
}
