// Create table (dynamodb__table-create, table-create-custom): the form as one value, the CreateTable it makes, and what is wrong with it.
import { filled, type KeyValueRow, newRow } from "@doze/ui/components/app/key-values";
import type { KeyType } from "./model";
import { nameProblem } from "./model";

export type Mode = "PAY_PER_REQUEST" | "PROVISIONED";
export type EncryptionChoice = "owned" | "aws" | "customer";

export type CreateForm = {
  name: string;
  pk: string;
  pkType: KeyType;
  sk: string;
  skType: KeyType;
  /** Customize settings: the capacity mode, encryption and tags below apply only when this is on. */
  custom: boolean;
  mode: Mode;
  read: string;
  write: string;
  encryption: EncryptionChoice;
  key: string | null;
  tags: KeyValueRow[];
};

export const blankForm = (): CreateForm => ({ name: "", pk: "", pkType: "S", sk: "", skType: "S", custom: false, mode: "PAY_PER_REQUEST", read: "5", write: "5", encryption: "owned", key: null, tags: [newRow()] });

/** A whole number of units, at least 1, or null. */
export const units = (v: string): number | null => {
  const n = Number(v);
  return v.trim() !== "" && Number.isInteger(n) && n >= 1 ? n : null;
};

/** The CreateTable the form makes: its members in the order the board shows them. Null until there is a name and a partition key. */
export function createInput(f: CreateForm): Record<string, unknown> | null {
  if (!f.name || !f.pk) return null;
  const sk = f.sk.trim();
  const input: Record<string, unknown> = {
    TableName: f.name,
    AttributeDefinitions: [{ AttributeName: f.pk, AttributeType: f.pkType }, ...(sk ? [{ AttributeName: sk, AttributeType: f.skType }] : [])],
    KeySchema: [{ AttributeName: f.pk, KeyType: "HASH" }, ...(sk ? [{ AttributeName: sk, KeyType: "RANGE" }] : [])],
    BillingMode: f.custom ? f.mode : "PAY_PER_REQUEST",
  };
  if (f.custom && f.mode === "PROVISIONED") input.ProvisionedThroughput = { ReadCapacityUnits: units(f.read) ?? 5, WriteCapacityUnits: units(f.write) ?? 5 };
  if (f.custom && f.encryption !== "owned") input.SSESpecification = { Enabled: true, SSEType: "KMS", ...(f.encryption === "customer" && f.key ? { KMSMasterKeyId: f.key } : {}) };
  const tags = f.custom ? filled(f.tags) : [];
  if (tags.length > 0) input.Tags = tags.map((t) => ({ Key: t.key, Value: t.value }));
  return input;
}

export type Problems = { name?: string; pk?: string; sk?: string; read?: string; write?: string; key?: string };

/** What the form says is wrong, field by field (the service's own refusals come after the submit). */
export function problems(f: CreateForm): Problems {
  const p: Problems = {};
  const name = nameProblem(f.name);
  if (name) p.name = name;
  if (f.sk.trim() && f.sk.trim() === f.pk) p.sk = "The sort key is another attribute than the partition key.";
  if (f.custom && f.mode === "PROVISIONED") {
    if (units(f.read) === null) p.read = "ReadCapacityUnits is a whole number, at least 1.";
    if (units(f.write) === null) p.write = "WriteCapacityUnits is a whole number, at least 1.";
  }
  if (f.custom && f.encryption === "customer" && !f.key) p.key = "Choose the key.";
  return p;
}

/** Why the button is off, when it is. */
export function whyOff(f: CreateForm): string | null {
  if (!f.name) return "Give the table a name.";
  if (!f.pk) return "Give the table a partition key.";
  const p = problems(f);
  return p.name ?? p.sk ?? p.read ?? p.write ?? p.key ?? null;
}
