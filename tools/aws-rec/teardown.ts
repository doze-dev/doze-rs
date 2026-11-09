#!/usr/bin/env bun
// Deletes everything the recorder created: names starting with `doze-rec-`, and KMS keys tagged doze-rec=true.
//   bun teardown.ts                 dry run: lists what it would delete, deletes nothing (the default)
//   bun teardown.ts --delete        deletes, then lists again and prints a residue report
//   bun teardown.ts --only sqs,s3   restrict to some kinds     bun teardown.ts --kinds   list the kinds
// Exit code 0 only when nothing is left (KMS keys already pending deletion are reported, not counted as residue).
import { aws, assertRecorder, ours, PREFIX } from "./lib.ts";

type Item = { id: string; label: string; tagged?: boolean };
type Kind = { name: string; list: () => Promise<Item[]>; remove: (i: Item) => Promise<void>; afterEach?: () => Promise<void> };

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const arr = <T>(x: T[] | undefined | null): T[] => x ?? [];
const tail = (s: string) => s.split(/[:/]/).pop() ?? s;
const attempt = async (f: () => Promise<unknown>) => { try { await f(); } catch { /* best effort before the real delete */ } };

const kinds: Kind[] = [
  {
    name: "cloudformation",
    list: async () => {
      const r = await aws(["cloudformation", "list-stacks", "--stack-status-filter", "CREATE_COMPLETE", "CREATE_FAILED", "CREATE_IN_PROGRESS", "ROLLBACK_COMPLETE", "ROLLBACK_FAILED", "ROLLBACK_IN_PROGRESS", "UPDATE_COMPLETE", "UPDATE_IN_PROGRESS", "UPDATE_ROLLBACK_COMPLETE", "UPDATE_ROLLBACK_FAILED", "DELETE_FAILED", "REVIEW_IN_PROGRESS", "IMPORT_COMPLETE", "IMPORT_ROLLBACK_COMPLETE"]);
      return arr<any>(r?.StackSummaries).filter((s) => ours(s.StackName)).map((s) => ({ id: s.StackName, label: s.StackName }));
    },
    remove: async (i) => {
      await attempt(() => aws(["cloudformation", "update-termination-protection", "--no-enable-termination-protection", "--stack-name", i.id]));
      const wait = () => aws(["cloudformation", "wait", "stack-delete-complete", "--stack-name", i.id]);
      await aws(["cloudformation", "delete-stack", "--stack-name", i.id]);
      try {
        await wait();
      } catch (e) {
        // DELETE_FAILED: a resource refused to go (for example one that was never created). Keep it out of the stack and finish; the
        // other kinds sweep whatever it left, by name, in the retry pass.
        const failed = arr<any>((await aws(["cloudformation", "list-stack-resources", "--stack-name", i.id]))?.StackResourceSummaries)
          .filter((r) => r.ResourceStatus === "DELETE_FAILED").map((r) => r.LogicalResourceId as string);
        if (!failed.length) throw e;
        await aws(["cloudformation", "delete-stack", "--stack-name", i.id, "--retain-resources", ...failed]);
        await wait();
      }
    },
  },
  {
    name: "apigateway-usage-plans",
    list: async () => arr<any>((await aws(["apigateway", "get-usage-plans"]))?.items).filter((p) => ours(p.name)).map((p) => ({ id: p.id, label: p.name })),
    remove: async (i) => { await aws(["apigateway", "delete-usage-plan", "--usage-plan-id", i.id]); },
  },
  {
    name: "apigateway-keys",
    list: async () => arr<any>((await aws(["apigateway", "get-api-keys"]))?.items).filter((k) => ours(k.name)).map((k) => ({ id: k.id, label: k.name })),
    remove: async (i) => { await aws(["apigateway", "delete-api-key", "--api-key", i.id]); },
  },
  {
    name: "apigateway-rest",
    list: async () => arr<any>((await aws(["apigateway", "get-rest-apis"]))?.items).filter((a) => ours(a.name)).map((a) => ({ id: a.id, label: a.name })),
    remove: async (i) => { await aws(["apigateway", "delete-rest-api", "--rest-api-id", i.id]); },
    afterEach: () => sleep(31_000), // DeleteRestApi is limited to one call per 30 seconds per account
  },
  {
    name: "apigateway-http",
    list: async () => arr<any>((await aws(["apigatewayv2", "get-apis"]))?.Items).filter((a) => ours(a.Name)).map((a) => ({ id: a.ApiId, label: a.Name })),
    remove: async (i) => { await aws(["apigatewayv2", "delete-api", "--api-id", i.id]); },
  },
  {
    name: "stepfunctions-machines",
    list: async () => arr<any>((await aws(["stepfunctions", "list-state-machines"]))?.stateMachines).filter((m) => ours(m.name)).map((m) => ({ id: m.stateMachineArn, label: m.name })),
    remove: async (i) => { await aws(["stepfunctions", "delete-state-machine", "--state-machine-arn", i.id]); },
  },
  {
    name: "stepfunctions-activities",
    list: async () => arr<any>((await aws(["stepfunctions", "list-activities"]))?.activities).filter((m) => ours(m.name)).map((m) => ({ id: m.activityArn, label: m.name })),
    remove: async (i) => { await aws(["stepfunctions", "delete-activity", "--activity-arn", i.id]); },
  },
  {
    name: "events-api-destinations",
    list: async () => arr<any>((await aws(["events", "list-api-destinations", "--name-prefix", PREFIX]))?.ApiDestinations).map((d) => ({ id: d.Name, label: d.Name })),
    remove: async (i) => { await aws(["events", "delete-api-destination", "--name", i.id]); },
  },
  {
    name: "events-rules",
    list: async () => {
      const buses = ["default", ...arr<any>((await aws(["events", "list-event-buses", "--name-prefix", PREFIX]))?.EventBuses).map((b) => b.Name as string)];
      const out: Item[] = [];
      for (const bus of buses) {
        const args = ["events", "list-rules", "--event-bus-name", bus];
        if (bus === "default") args.push("--name-prefix", PREFIX);
        for (const r of arr<any>((await aws(args))?.Rules)) if (ours(r.Name)) out.push({ id: `${bus}\t${r.Name}`, label: r.Name });
      }
      return out;
    },
    remove: async (i) => {
      const [bus, rule] = i.id.split("\t");
      const t = await aws(["events", "list-targets-by-rule", "--rule", rule, "--event-bus-name", bus]);
      const ids = arr<any>(t?.Targets).map((x) => x.Id as string);
      if (ids.length) await aws(["events", "remove-targets", "--rule", rule, "--event-bus-name", bus, "--ids", ...ids, "--force"]);
      await aws(["events", "delete-rule", "--name", rule, "--event-bus-name", bus, "--force"]);
    },
  },
  {
    name: "events-archives",
    list: async () => arr<any>((await aws(["events", "list-archives", "--name-prefix", PREFIX]))?.Archives).map((a) => ({ id: a.ArchiveName, label: a.ArchiveName })),
    remove: async (i) => { await aws(["events", "delete-archive", "--archive-name", i.id]); },
  },
  {
    name: "events-buses",
    list: async () => arr<any>((await aws(["events", "list-event-buses", "--name-prefix", PREFIX]))?.EventBuses).map((b) => ({ id: b.Name, label: b.Name })),
    remove: async (i) => { await aws(["events", "delete-event-bus", "--name", i.id]); },
  },
  {
    name: "events-connections",
    list: async () => arr<any>((await aws(["events", "list-connections", "--name-prefix", PREFIX]))?.Connections).map((c) => ({ id: c.Name, label: c.Name })),
    remove: async (i) => { await aws(["events", "delete-connection", "--name", i.id]); },
  },
  {
    name: "scheduler-schedules",
    list: async () => arr<any>((await aws(["scheduler", "list-schedules", "--name-prefix", PREFIX]))?.Schedules).map((s) => ({ id: `${s.GroupName}\t${s.Name}`, label: s.Name })),
    remove: async (i) => { const [g, n] = i.id.split("\t"); await aws(["scheduler", "delete-schedule", "--name", n, "--group-name", g]); },
  },
  {
    name: "scheduler-groups",
    list: async () => arr<any>((await aws(["scheduler", "list-schedule-groups", "--name-prefix", PREFIX]))?.ScheduleGroups).map((g) => ({ id: g.Name, label: g.Name })),
    remove: async (i) => { await aws(["scheduler", "delete-schedule-group", "--name", i.id]); },
  },
  {
    name: "lambda-functions",
    list: async () => arr<any>((await aws(["lambda", "list-functions"]))?.Functions).filter((f) => ours(f.FunctionName)).map((f) => ({ id: f.FunctionName, label: f.FunctionName })),
    remove: async (i) => { await aws(["lambda", "delete-function", "--function-name", i.id]); },
  },
  {
    name: "lambda-layers",
    list: async () => arr<any>((await aws(["lambda", "list-layers"]))?.Layers).filter((l) => ours(l.LayerName)).map((l) => ({ id: l.LayerName, label: l.LayerName })),
    remove: async (i) => {
      for (const v of arr<any>((await aws(["lambda", "list-layer-versions", "--layer-name", i.id]))?.LayerVersions))
        await aws(["lambda", "delete-layer-version", "--layer-name", i.id, "--version-number", String(v.Version)]);
    },
  },
  {
    name: "kinesis",
    list: async () => arr<string>((await aws(["kinesis", "list-streams"]))?.StreamNames).filter(ours).map((n) => ({ id: n, label: n })),
    remove: async (i) => {
      await attempt(() => aws(["kinesis", "wait", "stream-exists", "--stream-name", i.id]));
      await aws(["kinesis", "delete-stream", "--stream-name", i.id, "--enforce-consumer-deletion"]);
    },
  },
  {
    name: "dynamodb",
    list: async () => arr<string>((await aws(["dynamodb", "list-tables"]))?.TableNames).filter(ours).map((n) => ({ id: n, label: n })),
    remove: async (i) => {
      await attempt(() => aws(["dynamodb", "update-table", "--table-name", i.id, "--no-deletion-protection-enabled"]));
      await aws(["dynamodb", "delete-table", "--table-name", i.id]);
    },
  },
  {
    name: "sqs",
    list: async () => arr<string>((await aws(["sqs", "list-queues", "--queue-name-prefix", PREFIX]))?.QueueUrls).map((u) => ({ id: u, label: tail(u) })),
    remove: async (i) => { await aws(["sqs", "delete-queue", "--queue-url", i.id]); },
  },
  {
    name: "sns",
    list: async () => arr<any>((await aws(["sns", "list-topics"]))?.Topics).filter((t) => ours(tail(t.TopicArn))).map((t) => ({ id: t.TopicArn, label: tail(t.TopicArn) })),
    remove: async (i) => { await aws(["sns", "delete-topic", "--topic-arn", i.id]); },
  },
  {
    name: "ssm-parameters",
    list: async () => arr<any>((await aws(["ssm", "describe-parameters"]))?.Parameters).filter((p) => ours(p.Name)).map((p) => ({ id: p.Name, label: p.Name })),
    remove: async (i) => { await aws(["ssm", "delete-parameter", "--name", i.id]); },
  },
  {
    name: "secretsmanager",
    list: async () => arr<any>((await aws(["secretsmanager", "list-secrets"]))?.SecretList).filter((s) => ours(s.Name)).map((s) => ({ id: s.ARN, label: s.Name })),
    remove: async (i) => { await aws(["secretsmanager", "delete-secret", "--secret-id", i.id, "--force-delete-without-recovery"]); },
  },
  {
    name: "logs",
    list: async () => {
      const seen = new Map<string, Item>();
      for (const p of [PREFIX, `/${PREFIX}`, `/aws/lambda/${PREFIX}`])
        for (const g of arr<any>((await aws(["logs", "describe-log-groups", "--log-group-name-prefix", p]))?.logGroups)) seen.set(g.logGroupName, { id: g.logGroupName, label: g.logGroupName });
      return [...seen.values()];
    },
    remove: async (i) => { await aws(["logs", "delete-log-group", "--log-group-name", i.id]); },
  },
  {
    name: "cloudwatch-alarms",
    list: async () => {
      const r = await aws(["cloudwatch", "describe-alarms", "--alarm-name-prefix", PREFIX, "--alarm-types", "CompositeAlarm", "MetricAlarm"]);
      return [...arr<any>(r?.CompositeAlarms), ...arr<any>(r?.MetricAlarms)].map((a) => ({ id: a.AlarmName, label: a.AlarmName }));
    },
    remove: async (i) => { await aws(["cloudwatch", "delete-alarms", "--alarm-names", i.id]); },
  },
  {
    name: "cloudwatch-dashboards",
    list: async () => arr<any>((await aws(["cloudwatch", "list-dashboards", "--dashboard-name-prefix", PREFIX]))?.DashboardEntries).map((d) => ({ id: d.DashboardName, label: d.DashboardName })),
    remove: async (i) => { await aws(["cloudwatch", "delete-dashboards", "--dashboard-names", i.id]); },
  },
  {
    name: "s3",
    list: async () => arr<any>((await aws(["s3api", "list-buckets"]))?.Buckets).filter((b) => ours(b.Name)).map((b) => ({ id: b.Name, label: b.Name })),
    remove: async (i) => {
      for (const u of arr<any>((await aws(["s3api", "list-multipart-uploads", "--bucket", i.id]))?.Uploads))
        await aws(["s3api", "abort-multipart-upload", "--bucket", i.id, "--key", u.Key, "--upload-id", u.UploadId]);
      for (;;) {
        const v = await aws(["s3api", "list-object-versions", "--bucket", i.id, "--max-items", "500"]);
        const objs = [...arr<any>(v?.Versions), ...arr<any>(v?.DeleteMarkers)].map((o) => ({ Key: o.Key, VersionId: o.VersionId }));
        if (!objs.length) break;
        await aws(["s3api", "delete-objects", "--bucket", i.id, "--delete", JSON.stringify({ Objects: objs, Quiet: true })]);
      }
      await aws(["s3api", "delete-bucket", "--bucket", i.id]);
    },
  },
  {
    name: "kms-aliases",
    list: async () => arr<any>((await aws(["kms", "list-aliases"]))?.Aliases).filter((a) => String(a.AliasName).startsWith(`alias/${PREFIX}`)).map((a) => ({ id: a.AliasName, label: a.AliasName.slice(6) })),
    remove: async (i) => { await aws(["kms", "delete-alias", "--alias-name", i.id]); },
  },
  {
    // Keys have no names, so only keys carrying the tag doze-rec=true are ours. Deletion is scheduled for the 7-day minimum.
    name: "kms-keys",
    list: async () => {
      const out: Item[] = [];
      for (const k of arr<any>((await aws(["kms", "list-keys"]))?.Keys)) {
        const m = (await aws(["kms", "describe-key", "--key-id", k.KeyId]))?.KeyMetadata;
        if (!m || m.KeyManager !== "CUSTOMER" || m.KeyState === "PendingDeletion" || m.KeyState === "PendingReplicaDeletion") continue;
        const tags = arr<any>((await aws(["kms", "list-resource-tags", "--key-id", k.KeyId]))?.Tags);
        if (tags.some((t) => t.TagKey === "doze-rec" && t.TagValue === "true")) out.push({ id: k.KeyId, label: k.KeyId, tagged: true });
      }
      return out;
    },
    remove: async (i) => { await aws(["kms", "schedule-key-deletion", "--key-id", i.id, "--pending-window-in-days", "7"]); },
  },
  {
    name: "iam-users",
    list: async () => arr<any>((await aws(["iam", "list-users"]))?.Users).filter((u) => ours(u.UserName)).map((u) => ({ id: u.UserName, label: u.UserName })),
    remove: async (i) => {
      const u = ["--user-name", i.id];
      for (const k of arr<any>((await aws(["iam", "list-access-keys", ...u]))?.AccessKeyMetadata)) await aws(["iam", "delete-access-key", ...u, "--access-key-id", k.AccessKeyId]);
      for (const p of arr<any>((await aws(["iam", "list-attached-user-policies", ...u]))?.AttachedPolicies)) await aws(["iam", "detach-user-policy", ...u, "--policy-arn", p.PolicyArn]);
      for (const p of arr<string>((await aws(["iam", "list-user-policies", ...u]))?.PolicyNames)) await aws(["iam", "delete-user-policy", ...u, "--policy-name", p]);
      for (const g of arr<any>((await aws(["iam", "list-groups-for-user", ...u]))?.Groups)) await aws(["iam", "remove-user-from-group", ...u, "--group-name", g.GroupName]);
      await attempt(() => aws(["iam", "delete-login-profile", ...u]));
      await aws(["iam", "delete-user", ...u]);
    },
  },
  {
    name: "iam-groups",
    list: async () => arr<any>((await aws(["iam", "list-groups"]))?.Groups).filter((g) => ours(g.GroupName)).map((g) => ({ id: g.GroupName, label: g.GroupName })),
    remove: async (i) => {
      const g = ["--group-name", i.id];
      for (const p of arr<any>((await aws(["iam", "list-attached-group-policies", ...g]))?.AttachedPolicies)) await aws(["iam", "detach-group-policy", ...g, "--policy-arn", p.PolicyArn]);
      for (const p of arr<string>((await aws(["iam", "list-group-policies", ...g]))?.PolicyNames)) await aws(["iam", "delete-group-policy", ...g, "--policy-name", p]);
      for (const u of arr<any>((await aws(["iam", "get-group", ...g]))?.Users)) await aws(["iam", "remove-user-from-group", ...g, "--user-name", u.UserName]);
      await aws(["iam", "delete-group", ...g]);
    },
  },
  {
    name: "iam-roles",
    list: async () => arr<any>((await aws(["iam", "list-roles"]))?.Roles).filter((r) => ours(r.RoleName)).map((r) => ({ id: r.RoleName, label: r.RoleName })),
    remove: async (i) => {
      const r = ["--role-name", i.id];
      for (const p of arr<any>((await aws(["iam", "list-attached-role-policies", ...r]))?.AttachedPolicies)) await aws(["iam", "detach-role-policy", ...r, "--policy-arn", p.PolicyArn]);
      for (const p of arr<string>((await aws(["iam", "list-role-policies", ...r]))?.PolicyNames)) await aws(["iam", "delete-role-policy", ...r, "--policy-name", p]);
      for (const ip of arr<any>((await aws(["iam", "list-instance-profiles-for-role", ...r]))?.InstanceProfiles)) await aws(["iam", "remove-role-from-instance-profile", ...r, "--instance-profile-name", ip.InstanceProfileName]);
      await aws(["iam", "delete-role", ...r]);
    },
  },
  {
    name: "iam-instance-profiles",
    list: async () => arr<any>((await aws(["iam", "list-instance-profiles"]))?.InstanceProfiles).filter((p) => ours(p.InstanceProfileName)).map((p) => ({ id: p.InstanceProfileName, label: p.InstanceProfileName })),
    remove: async (i) => {
      const ip = (await aws(["iam", "get-instance-profile", "--instance-profile-name", i.id]))?.InstanceProfile;
      for (const r of arr<any>(ip?.Roles)) await aws(["iam", "remove-role-from-instance-profile", "--instance-profile-name", i.id, "--role-name", r.RoleName]);
      await aws(["iam", "delete-instance-profile", "--instance-profile-name", i.id]);
    },
  },
  {
    name: "iam-policies",
    list: async () => arr<any>((await aws(["iam", "list-policies", "--scope", "Local"]))?.Policies).filter((p) => ours(p.PolicyName)).map((p) => ({ id: p.Arn, label: p.PolicyName })),
    remove: async (i) => {
      const pa = ["--policy-arn", i.id];
      const e = await aws(["iam", "list-entities-for-policy", ...pa]);
      for (const r of arr<any>(e?.PolicyRoles)) await aws(["iam", "detach-role-policy", "--role-name", r.RoleName, ...pa]);
      for (const u of arr<any>(e?.PolicyUsers)) await aws(["iam", "detach-user-policy", "--user-name", u.UserName, ...pa]);
      for (const g of arr<any>(e?.PolicyGroups)) await aws(["iam", "detach-group-policy", "--group-name", g.GroupName, ...pa]);
      for (const v of arr<any>((await aws(["iam", "list-policy-versions", ...pa]))?.Versions)) if (!v.IsDefaultVersion) await aws(["iam", "delete-policy-version", ...pa, "--version-id", v.VersionId]);
      await aws(["iam", "delete-policy", ...pa]);
    },
  },
  {
    name: "iam-saml-providers",
    list: async () => arr<any>((await aws(["iam", "list-saml-providers"]))?.SAMLProviderList).filter((p) => ours(tail(p.Arn))).map((p) => ({ id: p.Arn, label: tail(p.Arn) })),
    remove: async (i) => { await aws(["iam", "delete-saml-provider", "--saml-provider-arn", i.id]); },
  },
];

const args = process.argv.slice(2);
if (args.includes("--kinds")) { console.log(kinds.map((k) => k.name).join("\n")); process.exit(0); }
const del = args.includes("--delete");
const onlyArg = args.indexOf("--only") >= 0 ? args[args.indexOf("--only") + 1] : "";
const only = onlyArg ? onlyArg.split(",") : null;
const chosen = kinds.filter((k) => !only || only.some((o) => k.name === o || k.name.startsWith(`${o}-`)));
if (only && !chosen.length) { console.error(`no kind matches --only ${onlyArg}`); process.exit(2); }

/** Last line of defence: nothing is removed unless it is named doze-rec-* or a doze-rec=true key. */
const safe = (i: Item) => i.tagged === true || ours(i.label) || ours(i.id) || ours(i.id.split("\t").pop() ?? "");

async function scan(label: string): Promise<{ found: Map<string, Item[]>; errors: string[] }> {
  const found = new Map<string, Item[]>();
  const errors: string[] = [];
  await Promise.all(chosen.map(async (k) => {
    try { const items = await k.list(); if (items.length) found.set(k.name, items); }
    catch (e) { errors.push(`${k.name}: cannot list (${(e as Error).message})`); }
  }));
  const total = [...found.values()].reduce((n, v) => n + v.length, 0);
  console.log(`\n${label}: ${total} item(s) in ${found.size} kind(s)`);
  for (const k of chosen) for (const i of found.get(k.name) ?? []) console.log(`  ${k.name.padEnd(26)} ${i.label}`);
  for (const e of errors) console.log(`  ! ${e}`);
  return { found, errors };
}

console.log(`recorder: ${await assertRecorder()}`);
console.log(del ? "mode: DELETE" : "mode: dry run (nothing is deleted; use --delete)");
const first = await scan("found");
if (first.errors.length) console.log("\nsome kinds could not be listed; the residue report below is not complete for them");
if (!del) process.exit(first.errors.length ? 1 : 0);

const failures: string[] = [];
for (let pass = 1; pass <= 2; pass++) {
  const work = pass === 1 ? first.found : (await scan(`retry pass ${pass}`)).found;
  failures.length = 0;
  for (const k of chosen) {
    for (const i of work.get(k.name) ?? []) {
      if (!safe(i)) { failures.push(`${k.name} ${i.label}: refused, not ours`); continue; }
      try { await k.remove(i); console.log(`  deleted  ${k.name.padEnd(26)} ${i.label}`); await k.afterEach?.(); }
      catch (e) { failures.push(`${k.name} ${i.label}: ${(e as Error).message}`); console.log(`  ${pass === 1 ? "deferred" : "FAILED  "} ${k.name.padEnd(26)} ${i.label}${pass === 1 ? "  (re-checked in the retry pass)" : ""}`); }
    }
  }
  if (!failures.length) break;
}
// Lists of these kinds lag behind deletes (SQS keeps a deleted queue in ListQueues for up to 60 s), so wait before calling it residue.
const EVENTUAL = new Set(["sqs", "stepfunctions-machines", "stepfunctions-activities", "cloudformation", "kinesis", "iam-roles", "iam-users", "iam-groups", "iam-policies", "s3"]);
let after = await scan("residue");
for (let waited = 0; waited < 90 && [...after.found.keys()].every((k) => EVENTUAL.has(k)) && after.found.size > 0; waited += 15) {
  console.log("  (only eventually-consistent lists still show items; waiting 15 s and checking again)");
  await sleep(15_000);
  after = await scan("residue, re-checked");
}
const left = [...after.found.values()].reduce((n, v) => n + v.length, 0);
const kmsPending = "KMS keys scheduled for deletion are not listed as residue; they disappear after the 7-day window.";
console.log(`\n${left === 0 && !after.errors.length && !failures.length ? "CLEAN: nothing of ours is left." : `NOT CLEAN: ${left} left, ${failures.length} failure(s), ${after.errors.length} unlisted kind(s).`}\n${kmsPending}`);
for (const f of failures) console.log(`  ! ${f}`);
process.exit(left === 0 && !after.errors.length ? 0 : 1);
