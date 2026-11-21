import { expect, test } from "bun:test";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { endpointName, pendingWhy, shortArn, shortId, subRow, topicSubRow } from "./sub-model";
import { PROTOCOLS, countWord, disabledCount, protocolLabel } from "./protocols";

const row = (name: string, cells: (string | null)[]): ResourceRow => ({ name, label: "", id: name, cells, link: null });

test("a confirmed subscription is its ARN: the topic and the id come out of it", () => {
  const s = subRow(row("arn:aws:sns:us-east-1:111122223333:orders-created:5f2a91c4-9a47-4c05-b6e1-7d0f2a58c931", ["orders-created", "sqs", "arn:aws:sqs:us-east-1:111122223333:invoice-queue"]));
  expect(s).toMatchObject({ pending: false, topic: "orders-created", topicArn: "arn:aws:sns:us-east-1:111122223333:orders-created", protocol: "sqs", id: "5f2a91c4-9a47-4c05-b6e1-7d0f2a58c931" });
  expect(shortId(s.id)).toBe("5f2a91c4…");
  expect(endpointName("sqs", s.endpoint)).toBe("invoice-queue");
  expect(shortArn(s.endpoint)).toBe("arn:aws:sqs:us-east-1:…:invoice-queue");
});

test("a subscription waiting for its endpoint is named by what tells it apart", () => {
  const s = subRow(row("pending|arn:aws:sns:us-east-1:1:alerts|http|http://127.0.0.1:9000/in", ["alerts", "http", "http://127.0.0.1:9000/in"]));
  expect(s).toMatchObject({ pending: true, topic: "alerts", protocol: "http", endpoint: "http://127.0.0.1:9000/in", id: null });
  expect(shortId(s.id)).toBe("PendingConfirmation");
  expect(pendingWhy("email")).toBe("Stored: doze-aws sends no email");
  const t = topicSubRow({ cells: ["PendingConfirmation", "email", "ops@larder.example"], link: { name: "pending|arn:aws:sns:us-east-1:1:alerts|email|ops@larder.example" } }, "arn:aws:sns:us-east-1:1:alerts");
  expect(t).toMatchObject({ pending: true, protocol: "email", endpoint: "ops@larder.example" });
});

test("five of the nine protocols are off here, and each says why", () => {
  expect(PROTOCOLS).toHaveLength(9);
  expect(disabledCount()).toBe(5);
  expect(countWord(disabledCount())).toBe("Five");
  expect(protocolLabel("lambda")).toBe("AWS Lambda");
  expect(PROTOCOLS.filter((p) => !p.active).every((p) => p.about.startsWith("Not available in doze-aws"))).toBe(true);
});
