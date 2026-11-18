// A Kafka Streams member for the browser tests (KIP-1071): kafkajs has no streams group client, so this speaks
// StreamsGroupHeartbeat v1 itself over one socket (flexible versions, request header v2), joining with the word count Kafka
// Streams sends (subtopology 0 reads the input and re-keys into `<app>-words-repartition`; subtopology 1 reads that and
// counts into a store with `<app>-counts-changelog`), then saying it runs what it was given.
import { Socket } from "node:net";
import { array, type Bytes, i16, i32, Reader, str, TAGS } from "./share";

const NULL: Bytes = [0];
const u16 = (n: number): Bytes => [(n >> 8) & 0xff, n & 0xff];
const absent: Bytes = [0xff];
const present = (b: Bytes): Bytes => [1, ...b];

function topicInfo(name: string, configs: [string, string][]): Bytes {
  return [...str(name), ...i32(0), ...i16(-1), ...array(configs.map(([k, v]) => [...str(k), ...str(v), ...TAGS])), ...TAGS];
}

function wordCount(app: string, input: string): Bytes {
  const rep = `${app}-words-repartition`;
  const zero = [...str("0"), ...array([str(input)]), ...array([]), ...array([]), ...array([str(rep)]), ...array([]), ...array([]), ...TAGS];
  const one = [...str("1"), ...array([]), ...array([]), ...array([topicInfo(`${app}-counts-changelog`, [["cleanup.policy", "compact"]])]), ...array([]), ...array([topicInfo(rep, [["cleanup.policy", "delete"]])]), ...array([]), ...TAGS];
  return [...i32(0), ...array([zero, one]), ...TAGS];
}

type Tasks = [string, number[]][];
const tasks = (t: Tasks): Bytes => array(t.map(([s, ps]) => [...str(s), ...array(ps.map((p) => i32(p))), ...TAGS]));

function readTasks(r: Reader): Tasks | null {
  const n = r.varint() - 1;
  if (n < 0) return null;
  const out: Tasks = [];
  for (let i = 0; i < n; i++) {
    const s = r.str() ?? "";
    const ps: number[] = [];
    for (let k = r.varint() - 1; k > 0; k--) ps.push(r.i32());
    r.tags();
    out.push([s, ps]);
  }
  return out;
}

export class StreamsMember {
  private socket = new Socket();
  private correlation = 0;
  private buffered = Buffer.alloc(0);
  private waiting: ((b: Buffer) => void)[] = [];
  private beating?: ReturnType<typeof setInterval>;
  epoch = 0;
  owned: Tasks = [];
  statuses: [number, string][] = [];

  constructor(
    private address: string,
    readonly group: string,
    readonly member: string,
    readonly input: string,
  ) {}

  async connect() {
    const [host, port] = this.address.split(":");
    await new Promise<void>((ok, fail) => {
      this.socket.once("error", fail);
      this.socket.connect(Number(port), host, () => ok());
    });
    this.socket.on("data", (d) => {
      this.buffered = Buffer.concat([this.buffered, d]);
      while (this.buffered.length >= 4 && this.buffered.length >= 4 + this.buffered.readInt32BE(0)) {
        const n = this.buffered.readInt32BE(0);
        const frame = this.buffered.subarray(4, 4 + n);
        this.buffered = this.buffered.subarray(4 + n);
        this.waiting.shift()?.(frame);
      }
    });
  }

  private async ask(key: number, version: number, body: Bytes): Promise<Reader> {
    const head = [...i16(key), ...i16(version), ...i32(++this.correlation), ...i16(4), ...Buffer.from("e2e!"), ...TAGS];
    const frame = Buffer.from([...i32(head.length + body.length), ...head, ...body]);
    const answer = new Promise<Buffer>((ok) => this.waiting.push(ok));
    this.socket.write(frame);
    const r = new Reader(await answer);
    r.i32();
    r.tags();
    return r;
  }

  /** One StreamsGroupHeartbeat (v1): the join at epoch 0 with the topology, or a beat saying what it owns; the error code. */
  private async heartbeat(): Promise<number> {
    const join = this.epoch === 0;
    const owned = tasks(this.owned);
    const body = [
      ...str(this.group),
      ...str(this.member),
      ...i32(this.epoch),
      ...i32(-1),
      ...NULL,
      ...NULL,
      ...i32(join ? 300_000 : -1),
      ...(join ? present(wordCount(this.group, this.input)) : absent),
      ...owned,
      ...array([]),
      ...array([]),
      ...(join ? str(`process-${this.member}`) : NULL),
      ...(join ? present([...str("localhost"), ...u16(7070), ...TAGS]) : absent),
      ...(join ? array([]) : NULL),
      ...(join ? array([]) : NULL),
      ...(join ? array([]) : NULL),
      0,
      ...TAGS,
    ];
    const r = await this.ask(88, 1, body);
    r.i32();
    const error = r.i16();
    r.str();
    r.str();
    this.epoch = r.i32();
    r.i32();
    r.i32();
    r.i32();
    r.i32();
    this.statuses = [];
    for (let n = r.varint() - 1; n > 0; n--) {
      const code = r.i8();
      this.statuses.push([code, r.str() ?? ""]);
      r.tags();
    }
    const active = readTasks(r);
    if (active !== null) this.owned = active;
    return error;
  }

  /** Joins and beats until it runs at least one task with no status (or, with `ready` false, until the first answer); then
   * keeps beating every 2 s. */
  async join(ready = true): Promise<void> {
    let error = await this.heartbeat();
    for (let i = 0; ready && error === 0 && i < 50 && (this.owned.length === 0 || this.statuses.length > 0); i++) {
      await new Promise((r) => setTimeout(r, 100));
      error = await this.heartbeat();
    }
    if (error !== 0) throw new Error(`streams group ${this.group}: the heartbeat answered ${error}`);
    if (ready && this.owned.length === 0) throw new Error(`streams group ${this.group}: no task: ${JSON.stringify(this.statuses)}`);
    this.beating = setInterval(() => void this.heartbeat(), 2_000);
  }

  close() {
    clearInterval(this.beating);
    this.socket.destroy();
  }
}
