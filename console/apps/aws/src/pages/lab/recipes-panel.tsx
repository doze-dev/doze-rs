import { Link, useNavigate } from "@tanstack/react-router";
import { EyeOffIcon, FlaskConicalIcon, PackageOpenIcon, SendIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useResourceList, useResourcesIndex } from "../../api/resources";
import { useLab } from "../../api/spaces";
import type { ResourceService } from "../../api/types/ResourceService";
import { Section } from "../card";
import { inputText, setDraft } from "./draft";
import { type Next, type Recipe, type RecipeSpec, SPECS } from "./recipes";

function NextLink({ to, children }: { to: Next; children: React.ReactNode }) {
  const cls = "rounded-sm text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none";
  if (to.kind === "live") {
    return (
      <Link to="/live" search={{ service: to.service, console: "shown" }} className={cls} data-testid="lab-recipe-next">
        {children}
      </Link>
    );
  }
  return (
    <Link to="/resources/$service/$kind/$name" params={{ service: to.service, kind: to.resKind, name: to.name }} search={to.view ? { view: to.view } : {}} className={cls} data-testid="lab-recipe-next">
      {children}
    </Link>
  );
}

function RecipeCard({ recipe, title, onRehearse }: { recipe: Recipe; title: string; onRehearse?: (r: Recipe) => void }) {
  const navigate = useNavigate();
  const open = () => {
    setDraft({ target: recipe.ask.target, op: recipe.ask.op, text: inputText(recipe.ask.input), note: `From the recipe: ${recipe.title}.` });
    void navigate({ to: "/lab", search: { tab: "send", api: recipe.ask.target, op: recipe.ask.op } });
  };
  return (
    <article className="flex min-w-0 flex-col rounded-xl border bg-card p-5" data-testid={`lab-recipe-${recipe.id}`} aria-label={recipe.title}>
      <div className="flex items-start justify-between gap-3">
        <h3 className="min-w-0 text-[15px] leading-5 font-semibold break-words text-bright" data-testid="lab-recipe-title">{recipe.title}</h3>
        <Badge variant="outline" className="shrink-0 text-[11px]">{title}</Badge>
      </div>
      <p className="mt-2 text-[13px] leading-5 text-muted-foreground">{recipe.line}</p>
      {recipe.secret && (
        <p className="mt-1.5 flex items-center gap-1.5 text-[12.5px] text-muted-foreground">
          <EyeOffIcon className="size-3.5" aria-hidden /> The value is hidden until you show it.
        </p>
      )}
      <div className="mt-4 flex flex-wrap gap-2">
        <Button type="button" variant="outline" size="sm" className="bg-card" onClick={open} data-testid="lab-recipe-open">
          <SendIcon /> Open in Send a call
        </Button>
        {onRehearse && (
          <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => onRehearse(recipe)} data-testid="lab-recipe-rehearse">
            <FlaskConicalIcon /> Rehearse a failure
          </Button>
        )}
      </div>
      <p className="mt-3 border-t pt-3 text-[12.5px] leading-[19px] text-muted-foreground">
        <NextLink to={recipe.next.to}>{recipe.next.text}</NextLink>
      </p>
    </article>
  );
}

/** The recipe of `spec` for the first thing of its kind, when there is one. Asking for the list is a call to the service, so this is drawn only
 * for a service that is awake, or one the person asked to look at. */
function RecipeSlot({ spec, titles, onRehearse }: { spec: RecipeSpec; titles: ReadonlyMap<string, string>; onRehearse?: (r: Recipe) => void }) {
  const list = useResourceList(spec.service, spec.kind);
  const page = list.data?.pages[0];
  const row = page?.rows[0];
  const recipe = useMemo(() => (row && page ? spec.build(row, page.columns) : null), [spec, row, page]);
  if (list.isPending) return <Skeleton className="h-44 rounded-xl" aria-label="Loading" />;
  if (!recipe) return null;
  return <RecipeCard recipe={recipe} title={titles.get(recipe.ask.target) ?? recipe.service} onRehearse={onRehearse} />;
}

type Kind = ResourceService["kinds"][number];
const kindOf = (s: ResourceService | undefined, id: string): Kind | undefined => s?.kinds.find((k) => k.id === id);

/** Recipes: a call to try with the person's own queues, tables, functions, topics, buckets, state machines, secrets and buses. A recipe
 * exists only for a thing that exists. A service that is resting is not asked (that would wake it): the person can ask for it. */
export function Recipes({ onRehearse }: { onRehearse?: (r: Recipe) => void }) {
  const index = useResourcesIndex(true, false);
  const lab = useLab();
  const [asked, setAsked] = useState<ReadonlySet<string>>(new Set());
  const titles = useMemo(() => new Map((lab.data?.targets ?? []).map((t) => [t.target, t.title])), [lab.data]);
  const services = index.data?.services;
  const slots = SPECS.map((spec) => {
    const svc = services?.find((s) => s.name === spec.service);
    const rested = !!svc?.served && svc.asleep;
    const present = rested ? asked.has(spec.id) : (kindOf(svc, spec.kind)?.count ?? 0) > 0;
    return { spec, svc, rested: rested && !present, present };
  });
  const present = slots.filter((s) => s.present);
  const resting = slots.filter((s) => s.rested);
  return (
    <Section title="Try it with your own things" testId="lab-recipes" lede="A call made from what you already have, with its input filled in. It opens in Send a call, where you can change it before it goes; or rehearse a failure of just those calls.">
      {index.isError && !index.data ? (
        <ErrorBlock what="Your resources could not be read" error={index.error} hint="Check that doze-aws is still running." onRetry={() => void index.refetch()} retrying={index.isFetching} testId="lab-recipes-error" />
      ) : !services ? (
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3" aria-busy aria-label="Loading">
          {[0, 1, 2].map((i) => <Skeleton key={i} className="h-44 rounded-xl" />)}
        </div>
      ) : (
        <>
          {present.length > 0 && (
            <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3" data-testid="lab-recipe-cards">
              {present.map((s) => <RecipeSlot key={s.spec.id} spec={s.spec} titles={titles} onRehearse={onRehearse} />)}
            </div>
          )}
          {present.length === 0 && (
            <EmptyState icon={PackageOpenIcon} title="Nothing to try yet" className="min-h-48" action={<Button asChild variant="outline" size="sm" className="bg-card"><Link to="/resources" data-testid="lab-recipes-resources">Open Resources</Link></Button>}>
              {resting.length > 0 ? "No awake service holds something to try. Some are resting, and looking in one wakes it." : "Make a queue, a table, a function, a topic or a bucket, and a call for it shows here, ready to send."}
            </EmptyState>
          )}
          {resting.length > 0 && (
            <p className="mt-4 flex flex-wrap items-center gap-x-3 gap-y-2 text-[12.5px] text-muted-foreground" data-testid="lab-recipes-resting">
              <span>Resting, so not looked at:</span>
              {resting.map((s) => (
                <Button key={s.spec.id} type="button" variant="ghost" size="xs" onClick={() => setAsked((a) => new Set(a).add(s.spec.id))} title={`Look for ${s.spec.kind} in ${s.svc?.title ?? s.spec.service}; this wakes it`}>
                  Look in {s.svc?.title ?? s.spec.service}
                </Button>
              ))}
            </p>
          )}
        </>
      )}
    </Section>
  );
}
