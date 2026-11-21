import { describe, expect, test } from "bun:test";
import { buildZip, crc32, isText, readZip, toBase64 } from "./zip";

const enc = new TextEncoder();

describe("the archive the page writes", () => {
  test("is read back whole, with each file's mode", async () => {
    const zip = buildZip([
      { path: "lambda_function.py", data: enc.encode("print('hi')\n"), mode: 0o644 },
      { path: "bin/run.sh", data: enc.encode("#!/bin/sh\n"), mode: 0o755 },
    ]);
    const back = await readZip(zip);
    expect(back.map((f) => f.path)).toEqual(["lambda_function.py", "bin/run.sh"]);
    expect(new TextDecoder().decode(back[0]?.data)).toBe("print('hi')\n");
    expect(back.map((f) => f.mode)).toEqual([0o644, 0o755]);
  });

  test("starts with the local header and names the standard CRC", () => {
    const zip = buildZip([{ path: "a", data: enc.encode("123456789"), mode: 0o644 }]);
    expect(Array.from(zip.subarray(0, 4))).toEqual([0x50, 0x4b, 0x03, 0x04]);
    // The check value every CRC-32 implementation is tried against.
    expect(crc32(enc.encode("123456789"))).toBe(0xcbf43926);
  });

  test("an empty archive has no files, and bytes that are not an archive are refused in words", async () => {
    expect(await readZip(buildZip([]))).toEqual([]);
    await expect(readZip(enc.encode("not a zip at all, but long enough to scan for an end record"))).rejects.toThrow("not a .zip");
  });

  test("a deflated entry (what the zip program writes) is inflated", async () => {
    const dir = (await import("node:fs")).mkdtempSync("/tmp/zip-test-");
    const fs = await import("node:fs");
    fs.writeFileSync(`${dir}/hello.txt`, "hello hello hello hello hello hello hello\n");
    const run = Bun.spawnSync(["zip", "-qj", `${dir}/h.zip`, `${dir}/hello.txt`]);
    if (run.exitCode !== 0) return;
    const files = await readZip(new Uint8Array(fs.readFileSync(`${dir}/h.zip`)));
    expect(new TextDecoder().decode(files[0]?.data)).toContain("hello hello");
    fs.rmSync(dir, { recursive: true });
  });

  test("base64 and the text check", () => {
    expect(toBase64(enc.encode("abc"))).toBe("YWJj");
    expect(isText(enc.encode("héllo"))).toBe(true);
    expect(isText(new Uint8Array([0, 1, 2]))).toBe(false);
    expect(isText(new Uint8Array([0xff, 0xfe, 0x41]))).toBe(false);
  });
});
