import { expect, test } from "bun:test";
import { QueryClient, QueryObserver } from "@tanstack/react-query";
import { forgetDeleted, GONE_FOR_MS, mentions } from "./forget";

test("a name is found in a key at any depth, in a string, a list or the input of an operation", () => {
  expect(mentions(["resources", "one", "sqs", "queues", "orders"], "orders")).toBe(true);
  expect(mentions(["resources", "kindop", "eventbridge", "rules", "ListTargetsByRule", { Rule: "nightly", EventBusName: "larder" }], "nightly")).toBe(true);
  expect(mentions(["resources", "kindop", "eventbridge", "rules", "ListRules", { EventBusName: "larder" }], "nightly")).toBe(false);
  expect(mentions(["resources", "one", "sqs", "queues", "orders-2"], "orders")).toBe(false);
  expect(mentions(["resources", "kindop", "apigateway", "rest", { path: "/restapis/a1b2c3/stages" }], "a1b2c3")).toBe(true);
  expect(mentions(["resources", "kindop", "sqs", { arn: "arn:aws:sqs:us-east-1:000000000000:orders" }], "orders")).toBe(true);
});

/** A mounted read: the observer is what a page holds, so a refetch is a call made while the page is on screen. */
function mount(qc: QueryClient, key: unknown[], calls: string[]) {
  const o = new QueryObserver(qc, { queryKey: key, queryFn: () => { calls.push(key.join("/")); return Promise.resolve(1); }, retry: false });
  return o.subscribe(() => undefined);
}

test("after a delete the gone resource's reads are not made again for a minute, and the rest of the service's are", async () => {
  const qc = new QueryClient();
  const calls: string[] = [];
  const stops = [
    mount(qc, ["resources", "one", "sqs", "queues", "orders"], calls),
    mount(qc, ["resources", "tags", "sqs", "queues", "orders"], calls),
    mount(qc, ["resources", "list", "sqs", "queues", false, {}], calls),
    mount(qc, ["resources", "one", "sqs", "queues", "other"], calls),
    mount(qc, ["resources", "one", "sns", "topics", "orders"], calls),
  ];
  // A list of the service that nothing shows (a page that was left).
  await qc.fetchQuery({ queryKey: ["resources", "list", "sqs", "queues", true, {}], queryFn: () => Promise.resolve(1) });
  await new Promise((r) => setTimeout(r, 20));
  calls.length = 0;
  let clock = 1_000_000;
  // It settles once the reads on screen are made again, so a mutation that returns it opens the next page on current rows.
  await forgetDeleted(qc, ["sqs"], ["orders"], () => clock);
  expect(calls.sort()).toEqual(["resources/list/sqs/queues/false/[object Object]", "resources/one/sqs/queues/other"]);
  // A page that mounts a read of what is gone (a list that still shows its row) does not make it.
  calls.length = 0;
  const row = mount(qc, ["resources", "one", "sqs", "queues", "orders"], calls);
  await new Promise((r) => setTimeout(r, 20));
  expect(calls).toEqual([]);
  // The list nobody showed is dropped, so the page that opens next reads it afresh and draws no old rows.
  expect(qc.getQueryData(["resources", "list", "sqs", "queues", true, {}])).toBeUndefined();
  row();
  // Once the minute is over the name is read afresh when it is opened again (it may have been made anew).
  clock += GONE_FOR_MS + 1;
  const again = mount(qc, ["resources", "one", "sqs", "queues", "orders"], calls);
  await new Promise((r) => setTimeout(r, 20));
  expect(calls).toEqual(["resources/one/sqs/queues/orders"]);
  again();
  for (const stop of stops) stop();
});
