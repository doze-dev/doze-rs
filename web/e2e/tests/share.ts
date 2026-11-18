// A share group member for the browser tests (KIP-932): kafkajs has no share consumer, so this speaks ShareGroupHeartbeat,
// ShareFetch and ShareAcknowledge itself over one socket (flexible versions, request header v2). The socket is the member's
// share session: closing it lets go of what the member holds, as Kafka does when a connection goes.
import { Socket } from "node:net";

export type Bytes = number[];

export const i8 = (n: number): Bytes => [n & 0xff];
export const i16 = (n: number): Bytes => [(n >> 8) & 0xff, n & 0xff];
export const i32 = (n: number): Bytes => [(n >>> 24) & 0xff, (n >>> 16) & 0xff, (n >>> 8) & 0xff, n & 0xff];
export const i64 = (n: number): Bytes => [...i32(Math.floor(n / 2 ** 32)), ...i32(n >>> 0)];
export function varint(n: number): Bytes {
  const out: Bytes = [];
  while (n > 0x7f) {
    out.push((n & 0x7f) | 0x80);
    n >>>= 7;
  }
  out.push(n);
  return out;
}
export const str = (s: string): Bytes => [...varint(Buffer.byteLength(s) + 1), ...Buffer.from(s)];
export const array = (items: Bytes[]): Bytes => [...varint(items.length + 1), ...items.flat()];
export const TAGS: Bytes = [0];

export class Reader {
  at = 0;
  constructor(private b: Buffer) {}
  i8() {
    return this.b.readInt8(this.at++);
  }
  i16() {
    const v = this.b.readInt16BE(this.at);
    this.at += 2;
    return v;
  }
  i32() {
    const v = this.b.readInt32BE(this.at);
    this.at += 4;
    return v;
  }
  varint() {
    let n = 0;
    let shift = 0;
    for (;;) {
      const byte = this.b[this.at++];
      n |= (byte & 0x7f) << shift;
      if (!(byte & 0x80)) return n;
      shift += 7;
    }
  }
  str(): string | null {
    const n = this.varint() - 1;
    if (n < 0) return null;
    const s = this.b.toString("utf8", this.at, this.at + n);
    this.at += n;
    return s;
  }
  uuid(): Bytes {
    const u = [...this.b.subarray(this.at, this.at + 16)];
    this.at += 16;
    return u;
  }
  tags() {
    for (let n = this.varint(); n > 0; n--) {
      this.varint();
      this.at += this.varint();
    }
  }
}

export class ShareMember {
  private socket = new Socket();
  private correlation = 0;
  private buffered = Buffer.alloc(0);
  private waiting: ((b: Buffer) => void)[] = [];
  epoch = 0;
  topicId: Bytes = [];

  constructor(
    private address: string,
    readonly group: string,
    readonly member: string,
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

  /** Sends one request (API key, version, body) and returns its response body, past the response header v1. */
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

  private beating?: ReturnType<typeof setInterval>;

  /** Joins the group reading `topic` and heartbeats until the assignment names it; then keeps heartbeating every 3 s. */
  async join(topic: string): Promise<void> {
    let error = await this.heartbeat(topic);
    for (let i = 0; error === 0 && this.topicId.length === 0 && i < 50; i++) {
      await new Promise((r) => setTimeout(r, 200));
      error = await this.heartbeat(null);
    }
    if (error !== 0 || this.topicId.length === 0) throw new Error(`share group ${this.group}: joining answered ${error}`);
    this.beating = setInterval(() => void this.heartbeat(null), 3_000);
  }

  /** One ShareGroupHeartbeat: the epoch 0 join with the topic, or a beat at the member's epoch; its error code. */
  private async heartbeat(topic: string | null): Promise<number> {
    const topics = topic === null ? [0] : array([str(topic)]);
    const body = [...str(this.group), ...str(this.member), ...i32(topic === null ? this.epoch : 0), 0, ...topics, ...TAGS];
    const r = await this.ask(76, 1, body);
    r.i32();
    const error = r.i16();
    r.str();
    r.str();
    this.epoch = r.i32();
    r.i32();
    if (r.i8() >= 0) {
      const topics = r.varint() - 1;
      if (topics > 0) this.topicId = r.uuid();
    }
    return error;
  }

  private session = 0;

  /** One ShareFetch of partition 0 of the joined topic: the top-level error and the ranges acquired. */
  async fetch(): Promise<{ error: number; acquired: [number, number, number][] }> {
    const partition = [...i32(0), ...array([]), ...TAGS];
    const topics = array([[...this.topicId, ...array([partition]), ...TAGS]]);
    const body = [...str(this.group), ...str(this.member), ...i32(this.session), ...i32(0), ...i32(1), ...i32(1 << 20), ...i32(500), ...i32(500), ...topics, ...array([]), ...TAGS];
    const r = await this.ask(78, 1, body);
    r.i32();
    const error = r.i16();
    r.str();
    r.i32();
    const acquired: [number, number, number][] = [];
    const ts = r.varint() - 1;
    for (let t = 0; t < ts; t++) {
      r.uuid();
      const ps = r.varint() - 1;
      for (let p = 0; p < ps; p++) {
        r.i32();
        r.i16();
        r.str();
        r.i16();
        r.str();
        r.i32();
        r.i32();
        r.tags();
        const n = r.varint() - 1;
        r.at += Math.max(0, n);
        const as = r.varint() - 1;
        for (let a = 0; a < as; a++) {
          const hi = r.i32();
          const first = hi * 2 ** 32 + (r.i32() >>> 0);
          const hi2 = r.i32();
          const last = hi2 * 2 ** 32 + (r.i32() >>> 0);
          acquired.push([first, last, r.i16()]);
          r.tags();
        }
        r.tags();
      }
      r.tags();
    }
    if (error === 0) this.session = this.session === 0 ? 1 : this.session + 1;
    return { error, acquired };
  }

  /** Acknowledges `first`..`first + types.length - 1` of partition 0, one type each; the partition's error code. */
  async acknowledge(first: number, types: number[]): Promise<number> {
    const batch = [...i64(first), ...i64(first + types.length - 1), ...array(types.map((t) => i8(t))), ...TAGS];
    const partition = [...i32(0), ...array([batch]), ...TAGS];
    const topics = array([[...this.topicId, ...array([partition]), ...TAGS]]);
    const r = await this.ask(79, 1, [...str(this.group), ...str(this.member), ...i32(this.session), ...topics, ...TAGS]);
    r.i32();
    const top = r.i16();
    this.session += 1;
    if (top !== 0) return top;
    r.str();
    const ts = r.varint() - 1;
    if (ts < 1) return 0;
    r.uuid();
    r.varint();
    r.i32();
    return r.i16();
  }

  close() {
    clearInterval(this.beating);
    this.socket.destroy();
  }
}
