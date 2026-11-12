// Makes crates/kafka-auth/tests/crypto_vectors/vectors.txt: PBKDF2-HMAC-SHA-256/512 and base64 vectors computed by bun's
// node:crypto and Buffer, which share no code with the in-tree implementations they check (SEC-196).
//
//   bun tools/gen-crypto-vectors.ts        (from anywhere; the root is found from this file)
//
// Lines: `P <alg> <password hex> <salt hex> <iterations> <derived key hex>` and `B <raw hex> <base64>`, with `-` for empty.
// The file is committed: this only needs running to change it.
import { pbkdf2Sync, randomBytes, randomInt } from "node:crypto";
import { writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dir, "..");
const out = join(root, "crates/kafka-auth/tests/crypto_vectors/vectors.txt");
const lines: string[] = [];
// Password lengths on both sides of the hash block sizes (64 and 128) where HMAC pads or hashes the key.
const around = [0, 1, 2, 55, 56, 63, 64, 65, 111, 112, 127, 128, 129, 200];
for (let i = 0; i < 112; i++) {
  const length = around[i % around.length] + (i % 3 === 0 ? randomInt(0, 100) : 0);
  const password = randomBytes(length);
  const salt = randomBytes(randomInt(0, 40));
  const iterations = [1, 2, 3, 4096, 5000][i % 5];
  for (const [alg, size] of [["sha256", 32], ["sha512", 64]] as const) {
    const key = pbkdf2Sync(password, salt, iterations, size, alg).toString("hex");
    lines.push(`P ${alg} ${password.toString("hex") || "-"} ${salt.toString("hex") || "-"} ${iterations} ${key}`);
  }
}
for (let i = 0; i < 160; i++) {
  const raw = randomBytes(i % 70);
  lines.push(`B ${raw.toString("hex") || "-"} ${raw.toString("base64") || "-"}`);
}
writeFileSync(out, lines.join("\n") + "\n");
console.log(`${lines.length} vectors written to ${out}`);
