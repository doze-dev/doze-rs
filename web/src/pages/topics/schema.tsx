// A topic's Schema tab: the subjects named for it (its values, its keys, a record on it) and those whose ids its newest records
// carry (a subject named for its record alone), each with its latest version laid out; the ids no subject holds here.
import { Link, useParams } from "@tanstack/react-router";
import { FileJson2Icon, PlusIcon } from "lucide-react";
import { useTopicSchemas } from "@/api/schemas";
import { EmptyState } from "@/components/app/empty-state";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { number } from "@/lib/format";
import { Callout } from "../groups/parts";
import { PageError } from "../page-error";
import { KindBadge, SchemaBlock } from "../schemas/parts";

const POSITION: Record<string, string> = { value: "its values", key: "its keys", record: "a record on it", carried: "carried by its records" };

export function TopicSchemaPage() {
  const { topic } = useParams({ from: "/topics/$topic" });
  const res = useTopicSchemas(topic);
  const d = res.data;
  if (res.isError && !d) return <PageError error={res.error} onRetry={() => void res.refetch()} />;
  if (!d) return <Skeleton className="h-80 rounded-xl" />;
  if (d.off) {
    return (
      <EmptyState icon={FileJson2Icon} title="No schema registry" action={<Button asChild variant="outline"><Link to="/schemas">Open Schemas</Link></Button>}>
        {d.off}
      </EmptyState>
    );
  }
  return (
    <div className="flex flex-col gap-5" data-testid="topic-schemas">
      <p className="text-[13.5px] text-muted-foreground">
        Subjects named for {topic} (<code className="font-mono">{topic}-value</code>, <code className="font-mono">{topic}-key</code>, <code className="font-mono">{topic}-&lt;record&gt;</code>), and those whose ids the newest {number(d.scanned)} records carry.
      </p>
      {d.unknown.length > 0 && (
        <Callout tone="warn" data-testid="unknown-ids">
          <b className="font-semibold text-bright">Records carry {d.unknown.length === 1 ? "an id" : "ids"} no subject holds here: {d.unknown.join(", ")}.</b> They are shown raw (40403, schema not found): written with another registry, or their subject was deleted for good.
        </Callout>
      )}
      {d.subjects.length === 0 ? (
        <EmptyState
          icon={FileJson2Icon}
          title={`No schema for ${topic}`}
          action={
            <Button asChild>
              <Link to="/schemas/new" search={{ subject: `${topic}-value` }}>
                <PlusIcon /> Register {topic}-value
              </Link>
            </Button>
          }
        >
          No subject is named for it and none of its newest records is in the registry's wire format.
        </EmptyState>
      ) : (
        d.subjects.map((s) => (
          <section key={s.subject} className="rounded-xl border bg-card p-5" data-testid="topic-subject">
            <div className="mb-3 flex flex-wrap items-center gap-3">
              <Link to="/schemas/subjects/$subject" params={{ subject: s.subject }} className="font-mono text-[15px] font-medium text-link hover:underline">
                {s.subject}
              </Link>
              <KindBadge kind={s.kind} />
              <Badge variant="outline" className="rounded-full bg-card font-normal text-muted-foreground">
                {POSITION[s.position] ?? s.position}
              </Badge>
              <span className="ml-auto font-mono text-[12.5px] text-muted-foreground tabular-nums">
                v{s.latest_version} · id {s.latest_id} · {s.level}
                {s.carried > 0 && ` · on ${number(s.carried)} of the newest records`}
              </span>
            </div>
            <SchemaBlock text={s.schema} className="[&_pre]:max-h-72" />
          </section>
        ))
      )}
    </div>
  );
}
