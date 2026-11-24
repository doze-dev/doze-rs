import { describe, expect, test } from "bun:test";
import { serviceOfPath } from "./rail-services";

describe("serviceOfPath", () => {
  test("is the service whose pages are open", () => {
    expect(serviceOfPath("/resources/sqs")).toBe("sqs");
    expect(serviceOfPath("/resources/sqs/queues")).toBe("sqs");
    expect(serviceOfPath("/resources/sqs/queues/orders")).toBe("sqs");
    expect(serviceOfPath("/resources/s3/buckets/a%2Fb")).toBe("s3");
  });
  test("is none off the Resources space, and on its index", () => {
    expect(serviceOfPath("/")).toBeNull();
    expect(serviceOfPath("/live")).toBeNull();
    expect(serviceOfPath("/resources")).toBeNull();
    expect(serviceOfPath("/access/users")).toBeNull();
  });
});
