// Delete event bus (eventbridge__bus-delete): the rules that go with it, named, and what an app that keeps sending to it still gets.
import { useNavigate } from "@tanstack/react-router";
import { PlainConfirm } from "../../../kit";
import { useDeleteClearing } from "../../../kit";
import { list } from "./words";

export function BusDelete({ open, onOpenChange, bus, rules }: { open: boolean; onOpenChange: (o: boolean) => void; bus: string; rules: readonly string[] }) {
  const navigate = useNavigate();
  const del = useDeleteClearing({ service: "eventbridge", kind: "buses", name: bus });
  return (
    <PlainConfirm
      open={open}
      onOpenChange={(o) => { onOpenChange(o); if (!o) del.reset(); }}
      title="Delete event bus?"
      description={<>Delete <b className="font-mono font-semibold text-bright">{bus}</b>? {rules.length > 0 ? <>Deleting an event bus deletes the rules associated with that event bus: <b className="font-mono font-semibold text-bright">{list(rules)}</b>, with their targets.</> : "It has no rules."}</>}
      call="DeleteEventBus"
      pending={del.isPending}
      error={del.error}
      onConfirm={() => del.mutate({ clear: true }, { onSuccess: () => void navigate({ to: "/resources/$service/$second", params: { service: "eventbridge", second: "buses" }, search: { gone: bus } }) })}
      testId="bus-delete"
    >
      <p className="text-[13.5px] leading-[21px] text-muted-foreground">You can delete a custom or partner event bus, never the default one. AWS words this two ways: the user guide says the rules go with the bus, and the DeleteEventBus reference says all rules must be deleted first, so the console removes each rule's targets, then the rules, then the bus. An app that keeps sending to <b className="font-mono font-semibold text-foreground">{bus}</b> by name still gets a <b className="font-semibold text-foreground">200</b> from PutEvents, and the event is dropped.</p>
    </PlainConfirm>
  );
}
