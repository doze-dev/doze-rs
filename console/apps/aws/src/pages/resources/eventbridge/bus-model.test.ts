import { describe, expect, test } from "bun:test";
import { newRow } from "@doze/ui/components/app/key-values";
import { archiveSummary, busNameProblem, createArchiveInput, createBusInput, emptyBus, encryptionSummary, tagsSummary } from "./bus-model";

describe("the create-bus form", () => {
  test("the name", () => {
    expect(busNameProblem("larder-audit")).toBeNull();
    expect(busNameProblem("")).toBe("Name the bus.");
    expect(busNameProblem("default")).toContain("default bus");
    expect(busNameProblem("a/b")).toContain("/");
    expect(busNameProblem("a b")).toContain("letters, digits");
  });
  test("CreateEventBus leaves out what was not set", () => {
    expect(createBusInput({ ...emptyBus, name: "x" })).toEqual({ Name: "x" });
    const f = { ...emptyBus, name: "larder-audit", description: "d", cmk: true, kms: "alias/larder-app", dlq: "arn:aws:sqs:us-east-1:1:dlq", tags: [newRow("Environment", "dev"), newRow()] };
    expect(createBusInput(f)).toEqual({ Name: "larder-audit", Description: "d", KmsKeyIdentifier: "alias/larder-app", DeadLetterConfig: { Arn: "arn:aws:sqs:us-east-1:1:dlq" }, Tags: [{ Key: "Environment", Value: "dev" }] });
    expect(createBusInput({ ...f, cmk: false })).not.toHaveProperty("KmsKeyIdentifier");
  });
  test("the archive is its own call", () => {
    expect(createArchiveInput({ ...emptyBus, archiveName: "a-archive" }, "arn:x")).toEqual({ ArchiveName: "a-archive", EventSourceArn: "arn:x" });
  });
  test("what the collapsed sections say", () => {
    const f = { ...emptyBus, cmk: true, kms: "alias/larder-app", dlq: "arn:aws:sqs:us-east-1:1:pick-lists-dlq", archive: true, archiveName: "larder-audit-archive", tags: [newRow("Environment", "dev"), newRow("Team", "platform")] };
    expect(encryptionSummary(f)).toBe("Customer managed key alias/larder-app, with pick-lists-dlq as the dead-letter queue. Stored, not applied locally.");
    expect(archiveSummary(f)).toBe("Enabled, named larder-audit-archive.");
    expect(tagsSummary(f)).toBe("Environment=dev, Team=platform.");
    expect(tagsSummary(emptyBus)).toBe("No tags.");
  });
});
