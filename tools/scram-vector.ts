import { createHmac, createHash, pbkdf2Sync } from "node:crypto";
function run(alg: "sha256" | "sha512", len: number) {
  const user = "user", pw = "pencil", cn = "rOprNGfwEbeRWgbNEkqO", sn = "%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0";
  const salt = Buffer.from("W22ZaJ0SNY7soEsUEjb6gQ==", "base64");
  const bare = `n=${user},r=${cn}`;
  const serverFirst = `r=${cn}${sn},s=${salt.toString("base64")},i=4096`;
  const withoutProof = `c=biws,r=${cn}${sn}`;
  const auth = `${bare},${serverFirst},${withoutProof}`;
  const salted = pbkdf2Sync(pw, salt, 4096, len, alg);
  const hmac = (k: Buffer, m: string | Buffer) => createHmac(alg, k).update(m).digest();
  const ck = hmac(salted, "Client Key");
  const sk = createHash(alg).update(ck).digest();
  const sig = hmac(sk, auth);
  const proof = Buffer.from(ck.map((b, i) => b ^ sig[i]));
  const ssig = hmac(hmac(salted, "Server Key"), auth);
  console.log(alg, "\n salted", salted.toString("hex"), "\n stored", sk.toString("hex"), "\n server", hmac(salted, "Server Key").toString("hex"));
  console.log(" final:", `${withoutProof},p=${proof.toString("base64")}`);
  console.log(" server-final:", `v=${ssig.toString("base64")}`);
}
run("sha256", 32); run("sha512", 64);
