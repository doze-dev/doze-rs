import { useState } from "react";
import { Chart, type Series } from "@doze/ui/components/app/chart";
import type { Point } from "@doze/ui/components/app/chart-scale";
import { CodeBlock, JsonBlock } from "@doze/ui/components/app/code-block";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { ToggleGroup, ToggleGroupItem } from "@doze/ui/components/ui/toggle-group";
import { Section, Specimen } from "../specimen";

const END = Date.UTC(2026, 10, 8, 8, 20);
const MIN = 60_000;
/** Deterministic points: every 5 minutes from `from` to END, a value from `f(i)`. */
const make = (span: number, step: number, f: (i: number) => number | null): Point[] => {
  const n = Math.floor(span / step);
  return Array.from({ length: n + 1 }, (_, i) => ({ t: END - span + i * step, v: f(i) }));
};
const placed = (i: number) => Math.max(0, Math.round(22 + 9 * Math.sin(i / 3) - i / 4));
const failed = (i: number) => Math.max(0, Math.round(3 + 2 * Math.sin(i / 2 + 1)));
const THREE_H = 180 * MIN;
const TWO: Series[] = [{ id: "a", label: "OrdersPlaced", points: make(THREE_H, 5 * MIN, placed) }, { id: "b", label: "OrdersFailed", points: make(THREE_H, 5 * MIN, failed) }];

const RANGES = { "1h": 60 * MIN, "3h": 180 * MIN, "1d": 1440 * MIN, "1w": 7 * 1440 * MIN } as const;

function Ranged() {
  const [r, setR] = useState<keyof typeof RANGES>("3h");
  const span = RANGES[r];
  const step = span / 36;
  return (
    <div className="w-full">
      <ToggleGroup type="single" variant="outline" size="sm" value={r} onValueChange={(v) => v && setR(v as keyof typeof RANGES)} aria-label="Range" className="mb-4">
        {Object.keys(RANGES).map((k) => (
          <ToggleGroupItem key={k} value={k} data-testid={`range-${k}`}>
            {k}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
      <Chart aria-label="Orders over the chosen range" series={[{ id: "a", label: "OrdersPlaced", points: make(span, step, placed) }]} from={END - span} to={END} testId="chart-ranged" />
    </div>
  );
}

const SPIKE: Series[] = [{ id: "d", label: "ApproximateNumberOfMessagesVisible", points: make(THREE_H, 5 * MIN, (i) => (i < 20 ? 1 : 1 + (i - 19) * 2)) }];
const GAPS: Series[] = [{ id: "g", label: "Invocations", points: make(THREE_H, 5 * MIN, (i) => (i >= 10 && i <= 15 ? null : i === 25 ? null : 12 + (i % 5))) }];
const FIVE: Series[] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"].map((label, k) => ({ id: label, label, points: make(THREE_H, 10 * MIN, (i) => 5 + k * 4 + Math.round(3 * Math.sin(i / 2 + k))) }));

export function ChartsPage() {
  return (
    <>
      <PageHeader title="Charts" sub="A time series drawn by hand in SVG: no chart library. Lines break where there is no data, the x axis ticks on UTC boundaries, the readout follows the pointer or the arrow keys, and it fades in (at once with reduced motion)." />

      <Section id="chart" title="Chart" source="components/app/chart.tsx" note="Point at it, or Tab to it and use the arrow keys. A series is a role, never a colour: the theme sets each, light and dark.">
        <Specimen label="Two series, 3 hours" stack>
          <Chart aria-label="Orders placed and failed, last 3 hours" series={TWO} from={END - THREE_H} to={END} unit="orders" />
        </Specimen>
        <Specimen label="Range switch" stack>
          <Ranged />
        </Specimen>
      </Section>

      <Section id="chart-lines" title="Thresholds, gaps and single points" source="components/app/chart.tsx" note="An alarm's threshold is a band on the bad side and a dashed line with its words. A gap in the data is a gap in the line, never a line to zero; a point with no neighbour is a dot.">
        <Specimen label="An alarm's threshold" stack>
          <Chart aria-label="A queue's depth against its alarm" series={SPIKE} from={END - THREE_H} to={END} threshold={{ value: 12, label: "Alarm at 12", bad: "above" }} testId="chart-threshold" />
        </Specimen>
        <Specimen label="Gaps (no data)" stack>
          <Chart aria-label="Invocations with gaps" series={GAPS} from={END - THREE_H} to={END} height={200} testId="chart-gaps" />
        </Specimen>
      </Section>

      <Section id="chart-series" title="Many series, and nothing to draw" source="components/app/chart.tsx" note="Past two series the area under a line is left out. A chart with no data says so in the space it will fill, and one that is being read holds the same space.">
        <Specimen label="Five series, long label" stack>
          <Chart aria-label="Five series" series={[...FIVE.slice(0, 4), { ...FIVE[4]!, label: "A series whose name is far longer than a legend or a readout expects, to see that both wrap or cut it" }]} from={END - THREE_H} to={END} height={220} testId="chart-five" />
        </Specimen>
        <Specimen label="A single point" stack>
          <Chart aria-label="One point" series={[{ id: "p", label: "One", points: [{ t: END - 90 * MIN, v: 4 }] }]} from={END - THREE_H} to={END} height={160} testId="chart-single" />
        </Specimen>
        <Specimen label="All zero" stack>
          <Chart aria-label="Zero" series={[{ id: "z", label: "Errors", points: make(THREE_H, 5 * MIN, () => 0) }]} from={END - THREE_H} to={END} height={160} testId="chart-zero" />
        </Specimen>
        <Specimen label="Empty" stack>
          <Chart aria-label="Nothing" series={[{ id: "e", label: "Nothing", points: [] }]} height={160} testId="chart-empty" empty="No data points in the last 3 hours. Metrics appear when something puts them." />
        </Specimen>
        <Specimen label="Loading" stack>
          <Chart aria-label="Loading" series={[]} loading height={160} testId="chart-loading" />
        </Specimen>
      </Section>

      <Section id="code" title="CodeBlock and JsonBlock" source="components/app/code-block.tsx" note="Moved here from the AWS console's pages. A block scrolls in its own box and takes the focus, so a keyboard can scroll it.">
        <Specimen label="JsonBlock" stack>
          <JsonBlock value={{ QueueName: "pick-lists", Attributes: { VisibilityTimeout: "60" }, tags: ["a", "b"] }} testId="json-block" className="w-full" />
        </Specimen>
        <Specimen label="CodeBlock, wrapped" stack>
          <CodeBlock text={'aws sqs create-queue --queue-name pick-lists --attributes VisibilityTimeout=60,MessageRetentionPeriod=345600,ReceiveMessageWaitTimeSeconds=0 --tags app=fulfilment,env=dev --endpoint-url http://127.0.0.1:14590'} className="w-full" testId="code-wrapped" />
        </Specimen>
        <Specimen label="CodeBlock, not wrapped, long lines" stack>
          <CodeBlock wrap={false} max="max-h-40" text={Array.from({ length: 12 }, (_, i) => `line ${i + 1}: ${"x".repeat(i === 3 ? 220 : 40)}`).join("\n")} className="w-full" testId="code-nowrap" />
        </Specimen>
      </Section>
    </>
  );
}
