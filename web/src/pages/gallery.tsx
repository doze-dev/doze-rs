import { zodResolver } from "@hookform/resolvers/zod";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { BellIcon, CalendarClockIcon, InboxIcon, MoreHorizontalIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { toast } from "sonner";
import { z } from "zod";
import { CodeValue } from "@/components/app/code-value";
import { Combobox, type Option } from "@/components/app/combobox";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { EmptyState } from "@/components/app/empty-state";
import { MultiSelect } from "@/components/app/multi-select";
import { PageHeader } from "@/components/app/page-header";
import { Figures, StatStrip } from "@/components/app/stat";
import { HeldPill, TailSpeed } from "@/components/app/tail-controls";
import { Done } from "@/components/app/write-done";
import { StatusDot } from "@/components/app/status-dot";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Form, FormControl, FormDescription, FormField, FormItem, FormLabel, FormMessage } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { number } from "@/lib/format";
import type { GallerySearch } from "@/lib/search";
import { WireDock } from "@/pages/live/wire-dock";

// zod compiles object parsers with `new Function` unless told not to; the CSP (no eval) would refuse it and Firefox logs that
// refusal as an error. Jitless parsing is plenty fast for a form.
z.config({ jitless: true });


const TOPICS: Option[] = ["orders", "orders.dlq", "payments.settled", "inventory.levels", "notifications.email", "clickstream", "pick-lists", "a-very-long-topic-name-that-needs-an-ellipsis.in-every-narrow-place.v2"].map((t, i) => ({ value: t, label: t, hint: number((i + 1) * 137) }));

const schema = z.object({
  name: z
    .string()
    .min(1, "Type a name.")
    .max(249, "At most 249 characters.")
    .regex(/^[a-zA-Z0-9._-]+$/, "Letters, digits, '.', '_' and '-' only (Kafka's rule)."),
  value: z.string().max(1024, "At most 1 KiB here."),
  partitions: z.enum(["1", "3", "6", "12"]),
  topic: z.string({ error: "Choose a topic." }).min(1, "Choose a topic."),
  watch: z.array(z.string()).min(1, "Choose at least one topic."),
  compact: z.boolean(),
  acks: z.enum(["0", "1", "all"]),
  confirm: z.boolean().refine((v) => v, "Tick to confirm."),
});
type FormValues = z.infer<typeof schema>;

type Row = { id: string; topic: string; partition: number; offset: number; key: string; size: number; state: "committed" | "open" | "aborted" };

function makeRows(n: number): Row[] {
  const topics = TOPICS.map((t) => t.value);
  return Array.from({ length: n }, (_, i) => ({
    id: `r${i}`,
    topic: topics[i % topics.length]!,
    partition: i % 6,
    offset: 1_000_000 + i * 7,
    key: i % 9 === 0 ? `ord-${i}-with-a-very-long-key-that-will-be-cut-by-the-column-${"x".repeat(40)}` : `ord-${88400 + i}`,
    size: 120 + ((i * 37) % 900),
    state: i % 17 === 0 ? "open" : i % 23 === 0 ? "aborted" : "committed",
  }));
}

const COLUMNS: DataColumn<Row>[] = [
  { accessorKey: "topic", header: "Topic", size: 200, meta: { mono: true } },
  { accessorKey: "partition", header: "Partition", size: 100, meta: { numeric: true } },
  { accessorKey: "offset", header: "Offset", size: 130, meta: { numeric: true }, cell: (c) => number(c.getValue<number>()) },
  { accessorKey: "key", header: "Key", meta: { mono: true } },
  { accessorKey: "size", header: "Bytes", size: 90, meta: { numeric: true } },
  {
    accessorKey: "state",
    header: "Transaction",
    size: 140,
    cell: (c) => {
      const v = c.getValue<Row["state"]>();
      return <StatusDot tone={v === "committed" ? "ok" : v === "open" ? "warn" : "bad"}>{v}</StatusDot>;
    },
  },
];

function Section({ id, title, children, description, className }: { id: string; title: string; description?: string; children: React.ReactNode; className?: string }) {
  return (
    <Card className={`min-w-0 ${className ?? ""}`} data-testid={`g-${id}`} id={id}>
      <CardHeader>
        <CardTitle className="text-[16px]">{title}</CardTitle>
        {description && <CardDescription>{description}</CardDescription>}
      </CardHeader>
      <CardContent className="flex flex-col gap-4">{children}</CardContent>
    </Card>
  );
}

function Out({ id, children }: { id: string; children: React.ReactNode }) {
  return (
    <output data-testid={id} className="block min-h-6 rounded-md bg-well px-3 py-1 font-mono text-[12.5px] break-all text-foreground">
      {children}
    </output>
  );
}

function TheForm() {
  const [submitted, setSubmitted] = useState<FormValues | null>(null);
  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { name: "", value: "", partitions: "3", topic: "", watch: [], compact: false, acks: "all", confirm: false },
  });
  return (
    <Form {...form}>
      <form
        noValidate
        className="grid items-start gap-5 md:grid-cols-2"
        data-testid="g-form-el"
        onSubmit={form.handleSubmit((v) => {
          setSubmitted(v);
          toast.success("Submitted", { description: `${v.name}: ${v.partitions} partitions` });
        })}
      >
        <FormField
          control={form.control}
          name="name"
          render={({ field }) => (
            <FormItem>
              <FormLabel>Topic name</FormLabel>
              <FormControl>
                <Input placeholder="orders" autoComplete="off" {...field} data-testid="f-name" />
              </FormControl>
              <FormDescription>Kafka's rule: letters, digits, '.', '_' and '-'.</FormDescription>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="partitions"
          render={({ field }) => (
            <FormItem>
              <FormLabel>Partitions</FormLabel>
              <Select value={field.value} onValueChange={field.onChange}>
                <FormControl>
                  <SelectTrigger className="w-40" data-testid="f-partitions">
                    <SelectValue />
                  </SelectTrigger>
                </FormControl>
                <SelectContent>
                  {["1", "3", "6", "12"].map((p) => (
                    <SelectItem key={p} value={p}>
                      {p} {p === "1" ? "partition" : "partitions"}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="topic"
          render={({ field }) => (
            <FormItem>
              <FormLabel>Copy configs from</FormLabel>
              <FormControl>
                <Combobox options={TOPICS} value={field.value || null} onChange={(v) => field.onChange(v ?? "")} placeholder="Choose a topic…" search="Search topics…" aria-label="Copy configs from" className="w-full" />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="watch"
          render={({ field }) => (
            <FormItem>
              <FormLabel>Topics to watch</FormLabel>
              <FormControl>
                <MultiSelect options={TOPICS} value={field.value} onChange={field.onChange} noun="topics" aria-label="Topics to watch" triggerClassName="w-full" className="min-w-0" />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="value"
          render={({ field }) => (
            <FormItem className="md:col-span-2">
              <FormLabel>First record's value</FormLabel>
              <FormControl>
                <Textarea rows={3} className="font-mono text-[13px]" placeholder='{"id": 1}' {...field} data-testid="f-value" />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="acks"
          render={({ field }) => (
            <FormItem>
              <FormLabel>acks</FormLabel>
              <FormControl>
                <RadioGroup value={field.value} onValueChange={field.onChange} className="flex gap-5" data-testid="f-acks">
                  {(["0", "1", "all"] as const).map((a) => (
                    <div key={a} className="flex items-center gap-2">
                      <RadioGroupItem value={a} id={`acks-${a}`} />
                      <Label htmlFor={`acks-${a}`} className="font-mono">
                        {a}
                      </Label>
                    </div>
                  ))}
                </RadioGroup>
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="compact"
          render={({ field }) => (
            <FormItem className="flex flex-row items-center justify-between gap-4 rounded-lg border bg-card p-3">
              <div>
                <FormLabel>Compacted</FormLabel>
                <FormDescription>cleanup.policy=compact</FormDescription>
              </div>
              <FormControl>
                <Switch checked={field.value} onCheckedChange={field.onChange} data-testid="f-compact" />
              </FormControl>
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="confirm"
          render={({ field }) => (
            <FormItem className="md:col-span-2">
              <div className="flex items-center gap-2">
                <FormControl>
                  <Checkbox checked={field.value} onCheckedChange={(c) => field.onChange(c === true)} data-testid="f-confirm" />
                </FormControl>
                <FormLabel className="font-normal">Create it now</FormLabel>
              </div>
              <FormMessage />
            </FormItem>
          )}
        />
        <div className="flex flex-wrap items-center gap-2 md:col-span-2">
          <Button type="submit" data-testid="f-submit">
            <PlusIcon /> Create topic
          </Button>
          <Button type="button" variant="outline" onClick={() => form.reset()}>
            Reset
          </Button>
        </div>
        <div className="md:col-span-2">
          <Out id="form-out">{submitted ? JSON.stringify(submitted) : "nothing submitted"}</Out>
        </div>
      </form>
    </Form>
  );
}

export function GalleryPage() {
  const search = useSearch({ from: "/gallery" });
  const navigate = useNavigate({ from: "/gallery" });
  const set = (patch: Partial<GallerySearch>, replace = true) => void navigate({ search: (s) => ({ ...s, ...patch }), replace, resetScroll: false });
  const [clicks, setClicks] = useState(0);
  const [menu, setMenu] = useState("none");
  const [deleted, setDeleted] = useState("none");
  const [confirmText, setConfirmText] = useState("");
  const [from, setFrom] = useState("");
  const [fromApplied, setFromApplied] = useState("none");
  const [popOpen, setPopOpen] = useState(false);
  const [dialogOpen, setDialogOpen] = useState(false);
  // The record Sheet has its own address. Opened from this page it pushed an entry, so closing goes Back; opened by a deep
  // link there is nothing of ours to go back to, so closing replaces the address.
  const pushed = useRef(false);
  const openRecord = (id: string) => {
    pushed.current = true;
    set({ record: id }, false);
  };
  const closeRecord = () => {
    if (pushed.current) {
      pushed.current = false;
      window.history.back();
    } else set({ record: undefined });
  };
  const rowsN = search.rows === 1000 ? 1000 : 20;
  const rows = useMemo(() => makeRows(rowsN), [rowsN]);
  const record = search.record ? rows.find((r) => r.id === search.record) : undefined;
  return (
    <>
      <PageHeader crumbs={[{ label: "Home", to: "/" }]} title="Component gallery" sub="Every control the console is made of, each doing its real job. The browser tests drive this page in Firefox, WebKit and Chromium." />
      <div className="grid gap-6 xl:grid-cols-2" data-testid="gallery">
        <Section id="buttons" title="Button" description="Variants and sizes; the count proves a click lands.">
          <div className="flex flex-wrap items-center gap-2">
            <Button onClick={() => setClicks((c) => c + 1)} data-testid="btn-primary">
              <PlusIcon /> Primary
            </Button>
            <Button variant="outline">Outline</Button>
            <Button variant="secondary">Secondary</Button>
            <Button variant="ghost">Ghost</Button>
            <Button variant="destructive">
              <Trash2Icon /> Delete
            </Button>
            <Button variant="link">Link</Button>
            <Button disabled>Disabled</Button>
            <Button size="sm" variant="outline">
              Small
            </Button>
          </div>
          <Out id="btn-out">{`clicks: ${clicks}`}</Out>
        </Section>

        <Section id="segmented" title="ToggleGroup (segmented) and Tabs" description="Both are in the address: Back restores them.">
          <div className="flex flex-wrap items-center gap-3">
            <span className="text-[13px] text-muted-foreground" id="density-l">
              Density
            </span>
            <ToggleGroup type="single" variant="outline" value={search.density ?? "comfortable"} onValueChange={(v) => v && set({ density: v === "comfortable" ? undefined : (v as GallerySearch["density"]) })} aria-labelledby="density-l" data-testid="density" className="bg-card">
              <ToggleGroupItem value="comfortable" className="px-3">
                Comfortable
              </ToggleGroupItem>
              <ToggleGroupItem value="compact" className="px-3">
                Compact
              </ToggleGroupItem>
            </ToggleGroup>
          </div>
          <Tabs value={search.tab ?? "messages"} onValueChange={(v) => set({ tab: v === "messages" ? undefined : (v as GallerySearch["tab"]) }, false)} data-testid="tabs">
            <TabsList>
              <TabsTrigger value="messages">Messages</TabsTrigger>
              <TabsTrigger value="partitions">Partitions</TabsTrigger>
              <TabsTrigger value="config">Config</TabsTrigger>
            </TabsList>
            <TabsContent value="messages" className="pt-2 text-[13.5px] text-muted-foreground">
              The newest records of the topic.
            </TabsContent>
            <TabsContent value="partitions" className="pt-2 text-[13.5px] text-muted-foreground">
              Start, end and last stable offset of each partition.
            </TabsContent>
            <TabsContent value="config" className="pt-2 text-[13.5px] text-muted-foreground">
              Every config, where it comes from and what it does.
            </TabsContent>
          </Tabs>
        </Section>

        <Section id="form" title="Form: Input, Textarea, Select, Combobox, MultiSelect, RadioGroup, Switch, Checkbox" description="react-hook-form with a zod schema: the errors are Kafka's rules, the submit shows what was sent.">
          <TheForm />
        </Section>

        <Section id="overlays" title="Dialog, Sheet, DropdownMenu, Popover, Tooltip" description="Esc closes, the focus returns to the opener; the Sheet has its own address (Back closes it).">
          <div className="flex flex-wrap items-center gap-2">
            <Dialog
              open={dialogOpen}
              onOpenChange={(o) => {
                setDialogOpen(o);
                if (!o) setConfirmText("");
              }}
            >
              <DialogTrigger asChild>
                <Button variant="outline" data-testid="open-dialog">
                  <Trash2Icon /> Delete topic…
                </Button>
              </DialogTrigger>
              <DialogContent data-testid="dialog">
                <DialogHeader>
                  <DialogTitle>Delete orders.dlq?</DialogTitle>
                  <DialogDescription>Its records go with it; a consumer of it gets UNKNOWN_TOPIC_OR_PARTITION. Type the name to confirm.</DialogDescription>
                </DialogHeader>
                <Input value={confirmText} onChange={(e) => setConfirmText(e.target.value)} placeholder="orders.dlq" aria-label="Topic name" data-testid="dialog-input" autoFocus />
                <DialogFooter>
                  <DialogClose asChild>
                    <Button variant="outline">Cancel</Button>
                  </DialogClose>
                  <Button
                    variant="destructive"
                    disabled={confirmText !== "orders.dlq"}
                    data-testid="dialog-confirm"
                    onClick={() => {
                      setDeleted("orders.dlq");
                      setDialogOpen(false);
                      setConfirmText("");
                      toast.success("Deleted orders.dlq");
                    }}
                  >
                    Delete topic
                  </Button>
                </DialogFooter>
              </DialogContent>
            </Dialog>
            <Button variant="outline" onClick={() => openRecord("r7")} data-testid="open-sheet">
              <InboxIcon /> Open record r7
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" size="icon" aria-label="Row actions" data-testid="open-menu">
                  <MoreHorizontalIcon />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start">
                <DropdownMenuLabel>orders</DropdownMenuLabel>
                <DropdownMenuSeparator />
                {["Produce", "Add partitions", "Delete records"].map((a) => (
                  <DropdownMenuItem key={a} onSelect={() => setMenu(a)}>
                    {a}
                  </DropdownMenuItem>
                ))}
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="destructive" onSelect={() => setMenu("Delete topic")}>
                  Delete topic
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <Popover open={popOpen} onOpenChange={setPopOpen}>
              <PopoverTrigger asChild>
                <Button variant="outline" data-testid="open-popover">
                  <CalendarClockIcon /> From time
                </Button>
              </PopoverTrigger>
              <PopoverContent className="w-72" align="start">
                <form
                  className="flex flex-col gap-3"
                  onSubmit={(e) => {
                    e.preventDefault();
                    setFromApplied(from || "none");
                    setPopOpen(false);
                  }}
                >
                  <Label htmlFor="from-time">From time, UTC</Label>
                  <Input id="from-time" value={from} onChange={(e) => setFrom(e.target.value)} placeholder="2026-11-13 14:30" data-testid="popover-input" />
                  <p className="text-[12px] text-muted-foreground">The first record at or after this time, in every partition.</p>
                  <Button type="submit" size="sm" data-testid="popover-apply">
                    Apply
                  </Button>
                </form>
              </PopoverContent>
            </Popover>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="icon" aria-label="What is a high watermark?" data-testid="tooltip-trigger">
                  <BellIcon />
                </Button>
              </TooltipTrigger>
              <TooltipContent data-testid="tooltip">The offset after the last record every replica has.</TooltipContent>
            </Tooltip>
          </div>
          <Out id="overlay-out">{`deleted: ${deleted} · menu: ${menu} · from: ${fromApplied}`}</Out>
          <Sheet open={!!search.record} onOpenChange={(o) => !o && closeRecord()}>
            <SheetContent className="w-full sm:max-w-xl" data-testid="sheet">
              <SheetHeader>
                <SheetTitle className="font-mono">orders · {record ? `${record.partition} @ ${number(record.offset)}` : search.record}</SheetTitle>
                <SheetDescription>One record, completely: key, value, headers, both timestamps.</SheetDescription>
              </SheetHeader>
              <div className="flex flex-col gap-3 px-4">
                <span className="text-[12.5px] text-muted-foreground">Key</span>
                <CodeValue value={record?.key ?? "unknown"} label="Key" />
                <span className="text-[12.5px] text-muted-foreground">Value</span>
                <pre className="rounded-lg border bg-well p-3 font-mono text-[12.5px]">{`{"id": ${record?.offset ?? 0}, "total": 12.5}`}</pre>
              </div>
            </SheetContent>
          </Sheet>
        </Section>

        <Section id="display" title="Card, Badge, status, StatStrip, CodeValue, EmptyState, Toast">
          <div className="flex flex-wrap items-center gap-2">
            <Badge>New</Badge>
            <Badge variant="secondary">compact</Badge>
            <Badge variant="outline" className="font-mono">
              v3
            </Badge>
            <Badge variant="destructive">refused</Badge>
            <StatusDot tone="ok">Running</StatusDot>
            <StatusDot tone="warn">Rebalancing</StatusDot>
            <StatusDot tone="bad">Not reachable</StatusDot>
            <StatusDot tone="idle">Not checked</StatusDot>
          </div>
          <StatStrip
            stats={[
              { label: "Topics", value: "7", unit: "21 partitions", ch: 4 },
              { label: "Records", value: number(1_234_567), unit: "in the log", ch: 9 },
              { label: "Last request", value: "14:32:07", unit: "UTC", mono: true, ch: 9, tip: "Exact: 14:32:07.412" },
            ]}
          />
          {/* Counts that would otherwise be a sentence: labelled figures (CON-805, CON-1345). */}
          <Figures
            aria-label="Where the values come from"
            figures={[
              { label: "Configs", value: "33" },
              { label: "Set on this topic", value: "1" },
              { label: "Set for the broker", value: "1" },
              { label: "Kafka's default", value: "31" },
            ]}
          />
          {/* A write's result, in place (CON-1354). */}
          <Done testId="gallery-done">
            <b className="font-semibold text-bright">orders has 6 partitions now.</b> It had 3.
          </Done>
          <div className="flex items-center gap-3">
            <TailSpeed testId="gallery-speed" />
            <HeldPill count={37} onShow={() => toast("Shown")} />
          </div>
          <CodeValue value="/Users/srini/Library/Application Support/doze/kafka/data/orders-0/00000000000000000000.log" label="Path" />
          <EmptyState
            icon={InboxIcon}
            title="No records yet"
            action={
              <Button variant="outline" size="sm" onClick={() => toast("Produce opens here in slice R1")} data-testid="toast-btn">
                Produce a record
              </Button>
            }
          >
            This topic is empty. Produce one, or point a client at it.
          </EmptyState>
        </Section>

        <Section id="table" className="xl:col-span-2" title="DataTable" description="Fixed columns, ellipsis with the whole value on hover, sticky header, stable row ids; virtual past 500 rows.">
          <div className="flex items-center gap-3">
            <span className="text-[13px] text-muted-foreground" id="rows-l">
              Rows
            </span>
            <ToggleGroup type="single" variant="outline" value={String(rowsN)} onValueChange={(v) => v && set({ rows: v === "20" ? undefined : 1000 })} aria-labelledby="rows-l" data-testid="rows" className="bg-card">
              <ToggleGroupItem value="20" className="px-3">
                20
              </ToggleGroupItem>
              <ToggleGroupItem value="1000" className="px-3">
                1,000
              </ToggleGroupItem>
            </ToggleGroup>
          </div>
          <DataTable data={rows} columns={COLUMNS} rowId={(r) => r.id} aria-label="Records" maxHeight={420} onRowClick={(r) => openRecord(r.id)} />
        </Section>

        <Section id="wire-dock" className="xl:col-span-2" title="WireDock (Live, R3)" description="The tape filtered to one topic or one group, for the topic and group pages to mount: <WireDock topic> and <WireDock group>.">
          <WireDock topic="orders" />
          <WireDock group="fulfilment" />
        </Section>
      </div>
    </>
  );
}
