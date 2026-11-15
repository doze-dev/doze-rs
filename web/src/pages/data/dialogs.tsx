// The three ways to give space back, each a Dialog over the Data page with its own address (`?dialog=empty&topic=`,
// `empty-all`, `erase`): what will be lost and what stays, said before anything is done; a typed phrase for the two that
// reach every topic; then what the broker did, in place, and Done.
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CheckIcon, EraserIcon, OctagonXIcon, TriangleAlertIcon, XIcon } from "lucide-react";
import { useState } from "react";
import { ApiFailure } from "@/api/client";
import { useEmptyForm } from "@/api/data";
import { afterWrite, detailOf, post } from "@/api/topic-writes";
import type { DataResponse } from "@/api/types/DataResponse";
import type { EmptiedAllAnswer } from "@/api/types/EmptiedAllAnswer";
import type { EmptiedAnswer } from "@/api/types/EmptiedAnswer";
import type { ErasedAnswer } from "@/api/types/ErasedAnswer";
import type { Refusal } from "@/api/types/Refusal";
import { Spans } from "@/components/app/spans";
import { Figures } from "@/components/app/stat";
import { Done, DoneFooter } from "@/components/app/write-done";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { number } from "@/lib/format";
import { bytes } from "@/lib/kafka";
import type { DataSearch } from "@/lib/search";
import { FieldError, RefusalCallout, StickyFooter } from "../topics/writes/parts";

type Which = NonNullable<DataSearch["dialog"]>;

/** Kafka's refusal, when the broker refused (422); an invalid form (400) carries only its sentence. */
function refusalOf(e: unknown): Refusal | undefined {
  return e instanceof ApiFailure && e.status === 422 ? detailOf<Refusal>(e) : undefined;
}

/** Lines with a mark: what stays (a check) or what goes (a cross). */
function Lines({ lines, goes = false, testId }: { lines: string[]; goes?: boolean; testId: string }) {
  const Mark = goes ? XIcon : CheckIcon;
  return (
    <ul className="flex flex-col gap-2 text-[13.5px] leading-[20px]" data-testid={testId}>
      {lines.map((l) => (
        <li key={l} className="flex gap-2.5">
          <Mark aria-hidden className={goes ? "mt-0.5 size-4 shrink-0 text-destructive" : "mt-0.5 size-4 shrink-0 text-good"} />
          <span className="min-w-0 text-foreground">{l}</span>
        </li>
      ))}
    </ul>
  );
}

/** Why it cannot be done now (a transaction under way): said before the button, which stays shut. */
function Blocked({ why }: { why: string }) {
  return (
    <div role="alert" className="flex gap-3 rounded-xl border border-refused/30 bg-refused/5 p-4 text-[13.5px] leading-[21px]" data-testid="blocked">
      <OctagonXIcon aria-hidden className="mt-0.5 size-5 shrink-0 text-refused" />
      <p className="min-w-0 text-foreground">{why}</p>
    </div>
  );
}

function Heading({ children }: { children: React.ReactNode }) {
  return <h3 className="mb-2.5 text-[11.5px] font-semibold tracking-[0.08em] text-muted-foreground uppercase">{children}</h3>;
}

/** The phrase typed to confirm, under its label. */
function Phrase({ phrase, value, onChange, error }: { phrase: string; value: string; onChange: (v: string) => void; error?: string }) {
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor="data-confirm">
        Type <code className="font-mono text-bright">{phrase}</code> to confirm
      </Label>
      <Input id="data-confirm" className="font-mono" autoComplete="off" spellCheck={false} value={value} onChange={(e) => onChange(e.target.value)} aria-invalid={!!error} data-testid="confirm" />
      <FieldError>{error}</FieldError>
    </div>
  );
}

function EmptyTopicBody({ topic, open }: { topic: string; open: boolean }) {
  const qc = useQueryClient();
  const form = useEmptyForm(topic, open);
  const send = useMutation({
    mutationFn: async () => {
      const a = await post<EmptiedAnswer | { missing: { code: string; message: string } }>("/api/v1/data/empty", { topic });
      // The topic was deleted since the dialog opened: said in the server's words.
      if ("missing" in a) throw new ApiFailure(404, a.missing);
      return a;
    },
    onSuccess: () => void afterWrite(qc),
  });
  if (send.data) {
    return (
      <div className="flex flex-col gap-5">
        <Done testId="emptied">{send.data.sentence}</Done>
        <DoneFooter />
      </div>
    );
  }
  const f = form.data;
  if (!f) return form.error ? <FieldError>{form.error.message}</FieldError> : <Skeleton className="h-48 w-full rounded-xl" />;
  const v = f.view;
  const refused = refusalOf(send.error);
  return (
    <div className="flex flex-col gap-5" data-testid="empty-form">
      <Figures
        aria-label="What goes"
        figures={[
          { label: "Records", value: number(v.records), ch: 6, testId: "empty-records" },
          { label: "On disk", value: bytes(v.bytes), ch: 8, testId: "empty-bytes" },
          { label: "Partitions", value: number(v.partitions), ch: 3 },
        ]}
      />
      {v.blocked && <Blocked why={v.blocked} />}
      {v.in_use.length > 0 && (
        <div className="flex gap-3 rounded-xl border border-warn/30 bg-warn/5 p-4 text-[13px] leading-[20px]" data-testid="in-use">
          <TriangleAlertIcon aria-hidden className="mt-0.5 size-5 shrink-0 text-warn" />
          <p className="min-w-0 break-words text-muted-foreground">
            <Spans spans={v.in_use} />
          </p>
        </div>
      )}
      <div>
        <Heading>What stays</Heading>
        <Lines lines={v.keeps} testId="keeps" />
      </div>
      {refused && <RefusalCallout r={refused} />}
      {send.error && !refused && <FieldError>{send.error.message}</FieldError>}
      <StickyFooter>
        <DialogClose asChild>
          <Button type="button" variant="outline">
            Cancel
          </Button>
        </DialogClose>
        <Button variant="destructive" disabled={!!v.blocked || send.isPending} onClick={() => send.mutate()} data-testid="empty-submit">
          <EraserIcon /> {f.button}
        </Button>
      </StickyFooter>
    </div>
  );
}

function EmptyAllBody({ r }: { r: DataResponse }) {
  const qc = useQueryClient();
  const [typed, setTyped] = useState("");
  const v = r.view;
  const phrase = r.words.empty_all_phrase;
  const send = useMutation({
    mutationFn: () => post<EmptiedAllAnswer>("/api/v1/data/empty-all", { confirm: typed }),
    onSuccess: () => void afterWrite(qc),
  });
  if (send.data) {
    return (
      <div className="flex flex-col gap-5">
        <Done testId="emptied-all">{send.data.sentence}</Done>
        <DoneFooter />
      </div>
    );
  }
  const refused = refusalOf(send.error);
  const invalid = send.error && !refused ? send.error.message : undefined;
  const topicsBytes = v.kinds.find((k) => k.kind === "topics")?.bytes ?? 0;
  const partitions = v.topics.reduce((a, t) => a + t.partitions, 0);
  return (
    <form className="flex flex-col gap-5" noValidate onSubmit={(e) => (e.preventDefault(), send.mutate())} data-testid="empty-all-form">
      <Figures
        aria-label="What goes"
        figures={[
          { label: "Topics", value: number(v.topics.length), ch: 4 },
          { label: "Partitions", value: number(partitions), ch: 4 },
          { label: "Frees about", value: bytes(topicsBytes), ch: 8, testId: "empty-all-bytes" },
        ]}
      />
      {v.txn_under_way && <Blocked why={`A transaction is under way (transactional id ${v.txn_under_way}). Emptying now would cut it in half: commit or abort it (or wait for its timeout), then try again.`} />}
      <div>
        <Heading>What stays</Heading>
        <Lines lines={r.words.empty_all_keeps} testId="keeps" />
      </div>
      <Phrase phrase={phrase} value={typed} onChange={(x) => (setTyped(x), send.reset())} error={invalid} />
      {refused && <RefusalCallout r={refused} />}
      <StickyFooter>
        <DialogClose asChild>
          <Button type="button" variant="outline">
            Cancel
          </Button>
        </DialogClose>
        <Button type="submit" variant="destructive" disabled={typed.trim() !== phrase || !!v.txn_under_way || send.isPending} data-testid="empty-all-submit">
          <EraserIcon /> Empty all topics
        </Button>
      </StickyFooter>
    </form>
  );
}

function EraseBody({ r }: { r: DataResponse }) {
  const qc = useQueryClient();
  const [typed, setTyped] = useState("");
  const v = r.view;
  const phrase = r.words.erase_phrase;
  const send = useMutation({
    mutationFn: () => post<ErasedAnswer>("/api/v1/data/erase", { confirm: typed }),
    onSuccess: () => void afterWrite(qc),
  });
  if (send.data) {
    return (
      <div className="flex flex-col gap-5">
        <Done testId="erased">{send.data.sentence}</Done>
        {send.data.registry_error && <FieldError>The schema registry could not be erased: {send.data.registry_error}</FieldError>}
        <DoneFooter />
      </div>
    );
  }
  const refused = refusalOf(send.error);
  const invalid = send.error && !refused ? send.error.message : undefined;
  const freed = v.kinds.filter((k) => k.kind === "topics" || k.kind === "strays").reduce((a, k) => a + k.bytes, 0);
  const figures = [
    { label: "Topics", value: number(v.topics.length), ch: 4 },
    { label: "Groups", value: number(v.groups), ch: 4 },
    { label: "Transactional ids", value: number(v.transactional_ids), ch: 4 },
    ...(v.subjects !== null ? [{ label: "Schema subjects", value: number(v.subjects), ch: 4 }] : []),
    { label: "Frees about", value: bytes(freed), ch: 8, testId: "erase-bytes" },
  ];
  return (
    <form className="flex flex-col gap-5" noValidate onSubmit={(e) => (e.preventDefault(), send.mutate())} data-testid="erase-form">
      <Figures aria-label="What goes" figures={figures} />
      {v.txn_under_way && <Blocked why={`A transaction is under way (transactional id ${v.txn_under_way}). Erasing now would cut it in half: commit or abort it (or wait for its timeout), then try again.`} />}
      <div className="grid gap-5 sm:grid-cols-2">
        <div>
          <Heading>Goes</Heading>
          <Lines lines={r.words.erase_removes} goes testId="removes" />
        </div>
        <div>
          <Heading>Stays</Heading>
          <Lines lines={r.words.erase_keeps} testId="keeps" />
        </div>
      </div>
      <Phrase phrase={phrase} value={typed} onChange={(x) => (setTyped(x), send.reset())} error={invalid} />
      {refused && <RefusalCallout r={refused} />}
      <StickyFooter>
        <DialogClose asChild>
          <Button type="button" variant="outline">
            Cancel
          </Button>
        </DialogClose>
        <Button type="submit" variant="destructive" disabled={typed.trim() !== phrase || !!v.txn_under_way || send.isPending} data-testid="erase-submit">
          Erase everything
        </Button>
      </StickyFooter>
    </form>
  );
}

const TITLES: Record<Which, { title: (t?: string) => React.ReactNode; description: string }> = {
  empty: {
    title: (t) => (
      <>
        Empty <code className="font-mono">{t}</code>?
      </>
    ),
    description: "Every record goes and its disk is freed now. The topic stays, ready for the next record.",
  },
  "empty-all": { title: () => "Empty all topics?", description: "Every record of every topic goes, compacted topics' too, and the disk is freed now. The topics stay." },
  erase: { title: () => "Erase everything?", description: "The broker comes back as a new one would be, but for who may use it. This cannot be undone." },
};

/** The dialog the address names, kept on screen while it animates out; each opening starts a fresh body. */
export function DataDialogs({ dialog, topic, data, onClose }: { dialog?: Which; topic?: string; data?: DataResponse; onClose: () => void }) {
  const [shown, setShown] = useState<{ d: Which; at: number; topic: string } | null>(null);
  const [was, setWas] = useState<string | undefined>(undefined);
  const key = dialog ? `${dialog}:${topic ?? ""}` : undefined;
  if (was !== key) {
    setWas(key);
    if (dialog) setShown({ d: dialog, at: (shown?.at ?? 0) + 1, topic: topic ?? "" });
  }
  if (!shown) return null;
  const open = dialog === shown.d;
  const t = TITLES[shown.d];
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex max-h-[min(92vh,60rem)] flex-col gap-0 overflow-hidden p-0 sm:max-w-2xl" data-testid={`dialog-${shown.d}`} aria-describedby="data-dialog-d">
        <DialogHeader className="border-b px-6 pt-5 pb-4">
          <DialogTitle className="text-[19px] font-semibold text-bright">{t.title(shown.topic)}</DialogTitle>
          <DialogDescription id="data-dialog-d">{t.description}</DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-6 py-5" data-testid="dialog-body">
          {shown.d === "empty" && <EmptyTopicBody key={shown.at} topic={shown.topic} open={open} />}
          {shown.d === "empty-all" && (data ? <EmptyAllBody key={shown.at} r={data} /> : <Skeleton className="h-48 w-full rounded-xl" />)}
          {shown.d === "erase" && (data ? <EraseBody key={shown.at} r={data} /> : <Skeleton className="h-48 w-full rounded-xl" />)}
        </div>
      </DialogContent>
    </Dialog>
  );
}
