import { describe, expect, test } from "bun:test";
import { indexSearch, groupId } from "../pages/resources/search";
import { groupTo, resourceCrumbs, serviceTrail, withCurrent, withGroup } from "./crumbs";

const queue = { name: "larder-app", group: "Messaging and events", service: "sqs", service_title: "SQS", kind: "queues", kind_title: "Queues" };

describe("a page's breadcrumb ends on the page itself", () => {
  test("a resource's page ends on the resource, with the kind before it as a link", () => {
    const c = resourceCrumbs(queue);
    expect(c.map((x) => x.label)).toEqual(["Resources", "SQS", "Queues", "larder-app"]);
    expect(c.at(-2)?.to).toBe("/resources/sqs/queues");
  });

  test("what follows the resource comes after it, the resource staying a link to its page", () => {
    const c = resourceCrumbs(queue, { label: "Aliases", to: "" });
    expect(c.map((x) => x.label)).toEqual(["Resources", "SQS", "Queues", "larder-app", "Aliases"]);
    expect(c.at(-2)?.to).toBe("/resources/sqs/queues/larder-app");
  });

  test("a trail that stops on a parent gets the page added", () => {
    const c = withCurrent(resourceCrumbs(queue), "Edit larder-app", "Edit");
    expect(c.at(-1)).toEqual({ label: "Edit", to: "" });
    expect(withCurrent([{ label: "Queues", to: "/resources/sqs/queues" }], "Create queue").map((x) => x.label)).toEqual(["Queues", "Create queue"]);
  });

  test("a trail that already ends on the page is left alone", () => {
    const same = [{ label: "Queues", to: "/q" }, { label: "Create queue", to: "/q?create" }];
    expect(withCurrent(same, "Create queue")).toBe(same);
    const longer = [{ label: "Data streams", to: "/s" }, { label: "Create data stream: Provisioned", to: "/s?create" }];
    expect(withCurrent(longer, "Create data stream")).toBe(longer);
    const goesNowhere = [{ label: "Keys", to: "/k" }, { label: "Edit key policy", to: "" }];
    expect(withCurrent(goesNowhere, "larder-app")).toBe(goesNowhere);
  });
});

describe("IAM and STS begin with Access, not Resources", () => {
  const user = { name: "dev", group: "Identity and access", service: "iam", service_title: "IAM", kind: "users", kind_title: "Users" };

  test("a resource of IAM", () => {
    const c = resourceCrumbs(user);
    expect(c.map((x) => x.label)).toEqual(["Access", "IAM", "Users", "dev"]);
    expect(c[0]?.to).toBe("/access");
    expect(c[1]?.to).toBe("/resources/iam");
  });

  test("a service's own trail, and no group crumb for any service", () => {
    expect(serviceTrail("sts", "STS").map((x) => x.label)).toEqual(["Access", "STS"]);
    expect(serviceTrail("sqs", "SQS").map((x) => x.label)).toEqual(["Resources", "SQS"]);
    expect(serviceTrail("sqs").map((x) => x.label)).toEqual(["Resources"]);
  });
});

describe("a service's group is a crumb between Resources and the service", () => {
  const groups: Record<string, string> = { sqs: "Messaging and events", cloudwatch: "Observability" };
  const of = (s: string) => groups[s];
  const labels = (c: { label: string }[]) => c.map((x) => x.label);

  test("a resource's page gets the group after Resources, linking to that heading of the index", () => {
    const c = withGroup(resourceCrumbs(queue), of);
    expect(labels(c)).toEqual(["Resources", "Messaging and events", "SQS", "Queues", "larder-app"]);
    expect(c[1]?.to).toBe("/resources?group=Messaging+and+events");
    expect(c[2]?.to).toBe("/resources/sqs");
  });

  test("a page that names its service by a kind's address still finds it", () => {
    const c = withGroup([{ label: "Resources", to: "/resources" }, { label: "CloudWatch", to: "/resources/cloudwatch/alarms" }], of);
    expect(labels(c)).toEqual(["Resources", "Observability", "CloudWatch"]);
  });

  test("it is added once", () => {
    const once = withGroup(resourceCrumbs(queue), of);
    expect(withGroup(once, of)).toBe(once);
  });

  test("nothing is added for Access, for a trail that has no service, or for a service the catalogue has not given", () => {
    const access = resourceCrumbs({ name: "dev", group: "Identity and access", service: "iam", service_title: "IAM", kind: "users", kind_title: "Users" });
    expect(withGroup(access, of)).toBe(access);
    const bare = [{ label: "Resources", to: "/resources" }];
    expect(withGroup(bare, of)).toBe(bare);
    const unknown = resourceCrumbs({ ...queue, service: "ssm" });
    expect(withGroup(unknown, of)).toBe(unknown);
    expect(withGroup([{ label: "Home", to: "/" }, { label: "Connect", to: "/connect" }], of)).toHaveLength(2);
  });

  test("the group's link and its heading's id agree", () => {
    expect(groupTo("Config and secrets")).toBe("/resources?group=Config+and+secrets");
    expect(groupId("Config and secrets")).toBe("group-config-and-secrets");
    expect(indexSearch({ group: "Config and secrets" })).toEqual({ group: "Config and secrets" });
    expect(indexSearch({ group: 7 })).toEqual({});
  });
});
