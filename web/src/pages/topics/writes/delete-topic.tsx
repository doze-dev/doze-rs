// Delete topic (boards topic-delete, topic-deleted): what goes, who uses it, the name typed to confirm. When the broker
// took it the topic's page is gone, so the dialog leaves for the topics list and says what was deleted there.
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Trash2Icon, TriangleAlertIcon } from "lucide-react";
import { useForm, useWatch } from "react-hook-form";
import { toast } from "sonner";
import type { DeleteForm } from "@/api/types/DeleteForm";
import type { DeletedAnswer } from "@/api/types/DeletedAnswer";
import { afterWrite, detailOf, post, topicPath, useDeleteForm } from "@/api/topic-writes";
import { Spans } from "@/components/app/spans";
import { Button } from "@/components/ui/button";
import { DialogClose } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { plural } from "@/lib/format";
import { CallBlock, FieldError, RefusalCallout, StickyFooter } from "./parts";

export function DeleteTopicBody({ topic, open }: { topic: string; open: boolean }) {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const form = useForm<{ confirm: string }>({ defaultValues: { confirm: "" } });
  const typed = useWatch({ control: form.control, name: "confirm" });
  const preview = useDeleteForm(topic, open);
  const send = useMutation({
    mutationFn: (v: { confirm: string }) => post<DeletedAnswer>(topicPath(topic, "/delete"), v),
    onSuccess: (a) => {
      const d = a.deleted;
      qc.removeQueries({ predicate: (q) => q.queryKey.includes(topic) });
      void afterWrite(qc);
      toast.success(`Deleted ${d.name}.`, {
        description: `${plural(d.records, "record")} and ${plural(d.partitions, "partition")} are gone from disk. A topic named ${d.name} made later is a new topic: it starts at offset 0.`,
      });
      void navigate({ to: "/topics", replace: true });
    },
  });
  const failed = detailOf<DeleteForm>(send.error);
  const shown = failed ?? preview.data;
  const v = shown?.view;
  return (
    <form className="flex flex-col gap-5" noValidate onSubmit={form.handleSubmit((x) => send.mutate(x))} data-testid="delete-form">
      <p className="text-[14px] leading-[22px] text-foreground" data-testid="delete-what">
        {v ? (
          <>
            This removes <b className="font-semibold">{plural(v.records, "record")}</b> in {plural(v.partitions, "partition")} from disk. It cannot be undone, and the name becomes free for a new, empty topic with a new id.
          </>
        ) : (
          " "
        )}
      </p>
      {shown && shown.in_use.length > 0 && (
        <div className="flex gap-3 rounded-xl border border-warn/30 bg-warn/5 p-4 text-[13px] leading-[20px]" data-testid="in-use">
          <TriangleAlertIcon aria-hidden className="mt-0.5 size-5 shrink-0 text-warn" />
          <p className="min-w-0 break-words text-muted-foreground">
            <Spans spans={shown.in_use} />
          </p>
        </div>
      )}
      <div className="flex flex-col gap-2">
        <Label htmlFor="dc">
          Type <code className="font-mono">{topic}</code> to confirm
        </Label>
        <Input id="dc" className="font-mono" autoComplete="off" spellCheck={false} aria-invalid={!!(failed?.view.error)} {...form.register("confirm", { onChange: () => send.reset() })} data-testid="confirm" />
        <FieldError>{failed?.view.error ?? undefined}</FieldError>
      </div>
      {failed?.refusal && <RefusalCallout r={failed.refusal} />}
      {v && <CallBlock lines={v.call} />}
      {send.error && !failed && <FieldError>{send.error.message}</FieldError>}
      <StickyFooter>
        <DialogClose asChild>
          <Button type="button" variant="outline">
            Cancel
          </Button>
        </DialogClose>
        <Button type="submit" variant="destructive" disabled={typed !== topic || send.isPending} data-testid="delete-submit">
          <Trash2Icon /> Delete topic
        </Button>
      </StickyFooter>
    </form>
  );
}
