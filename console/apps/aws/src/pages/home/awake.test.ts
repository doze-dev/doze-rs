import { describe, expect, test } from "bun:test";
import type { ServiceRow } from "../../api/types/ServiceRow";
import { awakeCount, awakeGroups } from "./awake";

const row = (name: string, group: string, served = true): ServiceRow => ({ name, title: name.toUpperCase(), group, served, built: 1, total: 2, apis: [] });
const seen = (service: string, last_ms: number) => ({ service, title: service, calls: 3, last_ms });

describe("awakeGroups", () => {
  const services = [row("sqs", "Messaging"), row("sns", "Messaging"), row("s3", "Storage"), row("kms", "Config", false)];
  test("groups keep the catalogue's order", () => {
    const g = awakeGroups(services, [], 300, 1_000_000);
    expect(g.map((x) => [x.name, x.rows.map((r) => r.name)])).toEqual([
      ["Messaging", ["sqs", "sns"]],
      ["Storage", ["s3"]],
      ["Config", ["kms"]],
    ]);
  });
  test("a call within the idle limit keeps a service awake; an older one, or none, is asleep", () => {
    const now = 10_000_000;
    const g = awakeGroups(services, [seen("sqs", now - 299_000), seen("sns", now - 301_000)], 300, now);
    const by = Object.fromEntries(g.flatMap((x) => x.rows).map((r) => [r.name, r.state]));
    expect(by).toEqual({ sqs: "awake", sns: "asleep", s3: "asleep", kms: "off" });
  });
  test("a service this run does not serve is never awake, whatever the record says", () => {
    const g = awakeGroups(services, [seen("kms", 9_999_999)], 300, 10_000_000);
    expect(g[2]?.rows[0]?.state).toBe("off");
  });
  test("the idle limit is the run's", () => {
    const g = awakeGroups([row("sqs", "M")], [seen("sqs", 0)], 9, 10_000);
    expect(g[0]?.rows[0]?.state).toBe("asleep");
    expect(awakeGroups([row("sqs", "M")], [seen("sqs", 5_000)], 9, 10_000)[0]?.rows[0]?.state).toBe("awake");
  });
  test("counts", () => {
    const g = awakeGroups(services, [seen("sqs", 9_999_000)], 300, 10_000_000);
    expect(awakeCount(g)).toEqual({ awake: 1, served: 3, all: 4 });
  });
});
