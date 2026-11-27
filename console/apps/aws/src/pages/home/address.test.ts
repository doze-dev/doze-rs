import { describe, expect, it } from "bun:test";
import { addressLabel } from "./address";

describe("addressLabel", () => {
  it("says IP and port for an address of numbers, an IPv6 one or localhost", () => {
    expect(addressLabel("http://127.0.0.1:4566")).toBe("IP and port");
    expect(addressLabel("http://[::1]:4566")).toBe("IP and port");
    expect(addressLabel("http://localhost:4566")).toBe("IP and port");
    expect(addressLabel(undefined)).toBe("IP and port");
  });
  it("says .doze name for a name, as the doze app gives under names", () => {
    expect(addressLabel("http://aws.larder.doze:4566")).toBe(".doze name");
  });
});
