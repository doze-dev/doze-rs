// A request in full, as a Sheet over the page (boards request-detail, request-refused, request-fenced, request-too-large;
// classic pages/live/drawers.rs and refusals.rs). Its address is `?request=<id>`. The rules of a refusal come from the
// server, decided as the classic drawers decide them, against what the broker says of the topic now.
import { Link } from "@tanstack/react-router";
import { CheckIcon, FlaskConicalIcon, RefreshCwIcon, SendIcon, TriangleAlertIcon, XIcon } from "lucide-react";
import { useRequest } from "@/api/live";
import type { RequestResponse } from "@/api/types/RequestResponse";
import type { RequestRule } from "@/api/types/RequestRule";
import type { WireDetail } from "@/api/types/WireDetail";
import { CopyButton } from "@/components/app/copy-button";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { number } from "@/lib/format";
import { cn } from "@/lib/utils";
import { classTone, classWord, clockMs, labDid, took } from "./words";

const TONE: Record<string, string> = { ok: "text-good", warn: "text-warn", bad: "text-bad", refused: "text-refused" };

function bytes(n: number): string {
  if (n < 1024) return `${number(n)} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1).replace(/\.0$/, "")} KiB`;
  return `${(n / 1024 / 1024).toFixed(1).replace(/\.0$/, "")} MiB`;
}

function Kv({ rows }: { rows: [string, React.ReactNode][] }) {
  return (
    <dl className="grid grid-cols-[minmax(7rem,9rem)_minmax(0,1fr)] gap-x-4 text-[13.5px]">
      {rows.map(([k, v]) => (
        <div key={k} className="contents">
          <dt className="border-b py-2.5 text-muted-foreground">{k}</dt>
          <dd className="min-w-0 border-b py-2.5 break-words text-bright">{v}</dd>
        </div>
      ))}
    </dl>
  );
}

function Sub({ children }: { children: React.ReactNode }) {
  return <span className="mt-0.5 block text-[12.5px] text-muted-foreground">{children}</span>;
}

function Mono({ children }: { children: React.ReactNode }) {
  return <span className="font-mono text-[12.5px]">{children}</span>;
}

function asked(d: WireDetail): React.ReactNode {
  if (d.topics.length === 0) return d.what ?? (d.row.group ? <Mono>{d.row.group}</Mono> : "-");
  return (
    <>
      {d.topics.map(([t, parts]) => (
        <Mono key={t}>
          {t}
          {parts.length > 0 && `-${parts.join(", ")}`}{" "}
        </Mono>
      ))}
      {d.from_offset !== null && <Sub>from offset {number(d.from_offset)}</Sub>}
    </>
  );
}

function Facts({ d }: { d: WireDetail }) {
  const r = d.row;
  const rows: [string, React.ReactNode][] = [
    [
      "Client",
      <>
        <Mono>{r.client || "(no client.id)"}</Mono>
        <Sub>
          {d.software?.trim() || "software not announced"}
          {d.peer && ` · ${d.peer}`} · {d.principal}
        </Sub>
      </>,
    ],
    ["Asked for", asked(d)],
  ];
  if (r.group)
    rows.push([
      "Group",
      <>
        <Mono>{r.group}</Mono>
        {d.member && (
          <Sub>
            member {d.member}
            {d.generation !== null && `, generation ${d.generation}`}
          </Sub>
        )}
      </>,
    ]);
  if (d.txn_id)
    rows.push([
      "Transaction",
      <>
        <Mono>{d.txn_id}</Mono>
        {d.producer && (
          <Sub>
            producer {d.producer[0]}, epoch {d.producer[1]}
          </Sub>
        )}
      </>,
    ]);
  if (d.reason)
    rows.push([
      "Reason",
      <>
        {d.reason}
        <Sub>the client's own words, clipped</Sub>
      </>,
    ]);
  rows.push([
    "Answer",
    <>
      {r.detail || classWord(r.class, r.code)}
      <Sub>
        error_code {r.code} ({r.code_name}){d.response_bytes > 0 && ` · ${bytes(d.response_bytes)} on the wire`}
      </Sub>
    </>,
  ]);
  rows.push([
    "Took",
    <>
      <Mono>{took(r.took_us)}</Mono>
      {d.wait_ms !== null && d.wait_ms > 0 && <Sub>The client was willing to wait {number(d.wait_ms)} ms for records: a long poll, answered when data came or the wait ended.</Sub>}
      {d.took_spread && (
        <Sub>
          Folded: {number(r.count)} requests; slowest {Math.floor(d.took_spread[0] / 1000)} ms, fastest {Math.floor(d.took_spread[1] / 1000)} ms, median {Math.floor(d.took_spread[2] / 1000)} ms.
        </Sub>
      )}
    </>,
  ]);
  rows.push(["Correlation id", <Mono>{number(d.correlation)}</Mono>]);
  return <Kv rows={rows} />;
}

function Rules({ rules }: { rules: RequestRule[] }) {
  return (
    <ul className="flex flex-col" data-testid="rules">
      {rules.map((r) => (
        <li key={r.title} className="grid grid-cols-[1.25rem_minmax(0,1fr)_auto] items-start gap-3 border-b py-3 last:border-b-0">
          {r.ok ? <CheckIcon className="mt-0.5 size-4 text-good" aria-label="Passes" /> : <XIcon className="mt-0.5 size-4 text-refused" aria-label="Does not pass" />}
          <div className="min-w-0 text-[13.5px] text-bright">
            {r.title}
            <span className="mt-0.5 block font-mono text-[12px] break-words text-muted-foreground">{r.sub}</span>
          </div>
          <span className="font-mono text-[12px] text-muted-foreground">{r.tag}</span>
        </li>
      ))}
    </ul>
  );
}

function H3({ children }: { children: React.ReactNode }) {
  return <h3 className="mt-6 mb-2 text-[14px] font-semibold text-bright">{children}</h3>;
}

/** The callout that carries Kafka's words. */
function Words({ tone, children }: { tone: "refused" | "warn" | "bad"; children: React.ReactNode }) {
  const c = tone === "warn" ? "border-warn/30 bg-warn/5 [&>svg]:text-warn" : tone === "bad" ? "border-bad/30 bg-bad/5 [&>svg]:text-bad" : "border-refused/30 bg-refused/5 [&>svg]:text-refused";
  return (
    <div className={cn("flex gap-3 rounded-xl border p-4 text-[13.5px]", c)} data-testid="kafka-words">
      <TriangleAlertIcon className="mt-0.5 size-[18px] shrink-0" aria-hidden />
      <div className="min-w-0 break-words text-foreground">{children}</div>
    </div>
  );
}

function kafkaWords(d: WireDetail): string {
  return d.record_note ?? d.note ?? d.words;
}

function WhoWhen({ d, client }: { d: WireDetail; client: boolean }) {
  const rows: [string, React.ReactNode][] = [];
  if (client)
    rows.push([
      "Client",
      <>
        <Mono>{d.row.client}</Mono>
        {d.software?.trim() && <span className="text-muted-foreground"> ({d.software.trim()})</span>}
      </>,
    ]);
  rows.push([
    "Answered",
    <Mono>
      {clockMs(d.row.at_ms)} · {Math.floor(d.row.took_us / 1000)} ms
    </Mono>,
  ]);
  rows.push(["Written", "Nothing"]);
  return (
    <div className="mt-4">
      <Kv rows={rows} />
    </div>
  );
}

function Actions({ children }: { children: React.ReactNode }) {
  return <div className="mt-6 flex flex-wrap gap-2.5">{children}</div>;
}

function Detail({ r }: { r: RequestResponse }) {
  const d = r.request;
  const topic = d.topics[0]?.[0];
  return (
    <>
      <Facts d={d} />
      {d.parts.length > 1 && (
        <>
          <H3>Per partition</H3>
          <table className="w-full table-fixed text-[13px]">
            <thead>
              <tr className="text-left text-[12px] text-muted-foreground uppercase">
                <th className="py-1.5 font-medium">Partition</th>
                <th className="py-1.5 font-medium">Result</th>
                <th className="w-28 py-1.5 text-right font-medium">Offset</th>
              </tr>
            </thead>
            <tbody>
              {d.parts.map((p) => (
                <tr key={`${p.topic}-${p.partition}`} className="border-t">
                  <td className="truncate py-1.5 font-mono">
                    {p.topic}-{p.partition}
                  </td>
                  <td className="py-1.5">{p.code === 0 ? "Done" : <Mono>{p.code_name}</Mono>}</td>
                  <td className="py-1.5 text-right font-mono tabular-nums">{p.offset !== null ? number(p.offset) : "-"}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {d.parts_more > 0 && <p className="mt-2 text-[12.5px] text-muted-foreground">And {d.parts_more} more, not kept.</p>}
        </>
      )}
      <div className="mt-5 rounded-xl border bg-well p-4 text-[13.5px]">
        <b className="font-semibold text-bright">What the broker kept.</b> <span className="text-muted-foreground">This summary only. Request bodies and record bytes are not recorded: to see the record, open the topic at that offset.</span>
      </div>
      {r.fetch && (
        <>
          <H3>Copy as</H3>
          <div className="flex flex-col gap-3">
            {(
              [
                ["kcat", r.fetch.kcat],
                ["Kafka console consumer", r.fetch.console],
              ] as const
            ).map(([label, cmd]) => (
              <div key={label}>
                <div className="mb-1.5 flex items-center justify-between">
                  <span className="text-[13px] text-muted-foreground">{label}</span>
                  <CopyButton text={cmd} what={`${label} command copied`} />
                </div>
                <pre className="overflow-x-auto rounded-lg border bg-well p-3 font-mono text-[12.5px] text-bright">{cmd}</pre>
              </div>
            ))}
          </div>
        </>
      )}
      <Actions>
        {topic && (
          <Button asChild>
            <Link to="/topics/$topic" params={{ topic }}>
              Open {topic}
            </Link>
          </Button>
        )}
        {r.fetch && (
          <Button variant="outline" asChild className="bg-card">
            <Link to="/topics/$topic" params={{ topic: r.fetch.topic }} search={{ partition: r.fetch.partition, offset: r.fetch.offset, limit: 1 } as never}>
              <RefreshCwIcon /> Fetch it again
            </Link>
          </Button>
        )}
      </Actions>
    </>
  );
}

function Refused({ r }: { r: RequestResponse }) {
  const d = r.request;
  const denied = d.row.class === "denied";
  const closed = d.row.class === "closed";
  const topic = d.topics[0]?.[0];
  return (
    <>
      <Words tone={denied ? "warn" : d.row.class === "failed" || closed ? "bad" : "refused"}>
        <b className="font-semibold text-bright">{closed ? "Why" : "Kafka's words"}</b>
        <br />
        {closed ? d.row.detail : kafkaWords(d)}
      </Words>
      <H3>The rule</H3>
      <p className="mb-1 text-[13.5px] text-muted-foreground">{d.plain}</p>
      <Rules rules={r.rules} />
      {d.pass && (
        <>
          <H3>What would pass</H3>
          <p className="text-[13.5px] text-muted-foreground">{d.pass}</p>
        </>
      )}
      {r.keyed && (
        <div className="mt-3">
          <div className="mb-1.5 flex items-center justify-between">
            <span className="text-[13px] text-muted-foreground">Give it a key</span>
            <CopyButton text={r.keyed} what="Command copied" />
          </div>
          <pre className="overflow-x-auto rounded-lg border bg-well p-3 font-mono text-[12.5px] text-bright">{r.keyed}</pre>
          <p className="mt-2 text-[12.5px] text-muted-foreground">A key with an empty value is also a valid record: a tombstone, which deletes that key at compaction.</p>
        </div>
      )}
      <WhoWhen d={d} client />
      {!denied && !closed && topic && (
        <Actions>
          <Button asChild>
            <Link to="/topics/$topic" params={{ topic }} search={{ produce: 1 } as never}>
              <SendIcon /> Produce with a key
            </Link>
          </Button>
          <Button variant="outline" asChild className="bg-card">
            <Link to="/topics/$topic" params={{ topic }} search={{ tab: "config" } as never}>
              Config
            </Link>
          </Button>
          <Button variant="outline" asChild className="bg-card">
            <Link to="/topics/$topic" params={{ topic }} search={{ tab: "log" } as never}>
              The log
            </Link>
          </Button>
        </Actions>
      )}
    </>
  );
}

function Fenced({ r }: { r: RequestResponse }) {
  const d = r.request;
  const topic = d.topics[0]?.[0];
  return (
    <>
      <Words tone="refused">
        <b className="font-semibold text-bright">Kafka's words</b>
        <br />
        {kafkaWords(d)}
      </Words>
      <H3>How it came to this</H3>
      {d.story.length === 0 ? (
        <p className="text-[13.5px] text-muted-foreground">No earlier request of this transactional id or producer is on the wire any more (the recorder keeps a bounded tape), so the story is only this request.</p>
      ) : (
        <ol className="flex flex-col gap-0 border-l pl-4" data-testid="story">
          {d.story.map((s) => (
            <li key={s.id} className="relative py-1.5 text-[13.5px]">
              <span aria-hidden className="absolute top-3 -left-[21px] size-2 rounded-full bg-muted-foreground" />
              <span className="mr-3 font-mono text-[12px] text-muted-foreground">{clockMs(s.at_ms).slice(0, 8)}</span>
              <b className="font-semibold text-bright">{s.api}</b> <Mono>v{s.version}</Mono> {s.detail}
            </li>
          ))}
          <li className="relative py-1.5 text-[13.5px]">
            <span aria-hidden className="absolute top-3 -left-[21px] size-2 rounded-full bg-refused" />
            <span className="mr-3 font-mono text-[12px] text-muted-foreground">{clockMs(d.row.at_ms).slice(0, 8)}</span>A request arrived from <Mono>{d.row.client}</Mono>
            {d.producer && (
              <>
                {" "}
                carrying <b className="font-semibold">epoch {d.producer[1]}</b>
              </>
            )}
            : refused, and nothing written.
          </li>
        </ol>
      )}
      <div className="mt-4">
        <Rules rules={r.rules} />
      </div>
      <H3>What would pass</H3>
      <p className="text-[13.5px] text-muted-foreground">
        The producer asks again with <Mono>InitProducerId</Mono> for its transactional id, gets a newer epoch and starts at sequence 0. Clients do this on their own after the error.
      </p>
      <WhoWhen d={d} client={false} />
      {topic && (
        <Actions>
          <Button asChild>
            <Link to="/topics/$topic" params={{ topic }} search={{ tab: "producers" } as never}>
              {d.producer ? `How fencing works: the epoch of producer ${d.producer[0]}` : "How fencing works: the producers of this topic"}
            </Link>
          </Button>
        </Actions>
      )}
    </>
  );
}

function TooLarge({ r }: { r: RequestResponse }) {
  const d = r.request;
  const topic = d.topics[0]?.[0];
  const limit = r.limits.max_message_bytes;
  return (
    <>
      <Words tone="refused">
        <b className="font-semibold text-bright">Kafka's words</b>
        <br />
        {d.words}
        <br />
        <span className="font-mono text-[12.5px]">
          {number(d.request_bytes)} B request{limit && ` against max.message.bytes ${number(limit[0])}`}.
        </span>
      </Words>
      <H3>The rule</H3>
      <p className="mb-1 text-[13.5px] text-muted-foreground">
        A batch larger than the limit is refused whole. The topic's own <Mono>max.message.bytes</Mono> wins; with none set, the broker's <Mono>message.max.bytes</Mono> applies.
      </p>
      <Rules rules={r.rules} />
      <H3>What would pass</H3>
      <p className="text-[13.5px] text-muted-foreground">{d.pass}</p>
      <WhoWhen d={d} client />
      {topic && (
        <Actions>
          <Button asChild>
            <Link to="/topics/$topic" params={{ topic }} search={{ tab: "config" } as never}>
              Raise max.message.bytes
            </Link>
          </Button>
          <Button variant="outline" asChild className="bg-card">
            <Link to="/topics/$topic" params={{ topic }}>
              Open {topic}
            </Link>
          </Button>
        </Actions>
      )}
    </>
  );
}

function Body({ r }: { r: RequestResponse }) {
  switch (r.kind) {
    case "detail":
      return <Detail r={r} />;
    case "refused":
      return <Refused r={r} />;
    case "fenced":
      return <Fenced r={r} />;
    case "too_large":
      return <TooLarge r={r} />;
  }
}

function title(r: RequestResponse): string {
  const row = r.request.row;
  if (r.kind === "detail") return `${row.api} v${row.version}${row.transactional ? " (txn)" : ""}`;
  if (row.class === "closed") return "Connection closed";
  return `${row.code_name} (${row.code})`;
}

/** The Sheet of `?request=<id>`. */
export function RequestSheet({ id, onClose }: { id: number | undefined; onClose: () => void }) {
  const q = useRequest(id);
  const r = q.data?.found ?? undefined;
  const gone = q.data?.gone ?? (q.isError ? q.error.message : null);
  const row = r?.request.row;
  return (
    <Sheet open={id !== undefined} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-[34rem]" data-testid="request-sheet" data-kind={r?.kind}>
        <SheetHeader className="border-b px-6 pt-6 pb-5">
          <SheetTitle className={cn("pr-8 text-[20px] font-medium text-bright", r && r.kind !== "detail" && row?.class !== "closed" && "font-mono text-[18px]")}>{r ? title(r) : gone ? "Request not found" : "Request"}</SheetTitle>
          <SheetDescription asChild>
            <div className="flex min-h-5 flex-wrap items-center gap-x-1.5 text-[13px]">
              {row ? (
                <>
                  <span className={cn("inline-flex items-center gap-1.5 font-semibold", TONE[classTone(row.class)])}>
                    <span aria-hidden className="size-2 rounded-full bg-current" />
                    {classWord(row.class, row.code)}
                  </span>
                  <span>
                    · {row.api} v{row.version} · {row.target || row.client} · <span className="font-mono">{clockMs(row.at_ms)}</span>
                  </span>
                </>
              ) : (
                <span> </span>
              )}
            </div>
          </SheetDescription>
        </SheetHeader>
        <div className="flex flex-col px-6 py-5" data-testid="request-body">
          {gone ? (
            <p className="text-[13.5px] text-muted-foreground" data-testid="request-gone">
              {gone}
            </p>
          ) : !r ? (
            <div className="flex flex-col gap-3" aria-busy>
              <Skeleton className="h-20 rounded-xl" />
              <Skeleton className="h-48 rounded-xl" />
            </div>
          ) : (
            <>
              {row?.lab && (
                <div className="mb-4 flex items-start gap-3 rounded-xl border border-primary/30 bg-primary/5 p-4 text-[13.5px]" data-testid="lab-tag">
                  <FlaskConicalIcon className="mt-0.5 size-[18px] shrink-0 text-link" aria-hidden />
                  <p>
                    <b className="font-semibold text-bright">A Lab rule touched this request.</b> Rule {row.lab.rule}, <span className="font-mono">{row.lab.name}</span>, {labDid(row.lab.what)}.{" "}
                    <Link to="/lab" className="font-medium text-link hover:underline">
                      Open the Lab
                    </Link>
                  </p>
                </div>
              )}
              <Body r={r} />
            </>
          )}
        </div>
      </SheetContent>
    </Sheet>
  );
}
