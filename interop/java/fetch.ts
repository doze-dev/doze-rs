// Downloads the jars the Java interop client needs from Maven Central into a cache directory, checks each against the
// SHA-256 pinned here, and prints the classpath (colon-separated) for `java -cp`.
//
//   bun interop/java/fetch.ts 4      kafka-clients 4.4.0 and its runtime dependencies
//   bun interop/java/fetch.ts 2.8    kafka-clients 2.8.2 and its runtime dependencies
//   bun interop/java/fetch.ts streams   the 4.4.0 set and kafka-streams 4.4.0 (Streams.java)
//
// The cache is $DOZE_INTEROP_CACHE/java, else ~/.cache/doze-interop/java. A jar already there is checked again, not
// trusted; one whose bytes do not match is fetched again, and a download that does not match is an error.
//
// The checksums were taken on 2026-11-15: each jar was downloaded and its SHA-1 checked against Maven Central's
// .sha1 file (all matched); kafka-clients 4.4.0 and lz4-java 1.11.2 also have a .sha256 there, which matched the
// SHA-256 below. The dependency versions are the ones each kafka-clients POM names at runtime scope, with two
// exceptions, both for Apple Silicon (darwin aarch64), on which the run is made:
//   - snappy-java: 2.8.2's POM names 1.1.8.1, which has no native library for darwin aarch64 and no fallback, so
//     snappy would fail to load; 1.1.10.7 (the 4.4.0 pin) is used for both, as a 2.8 user on a Mac would do.
//   - lz4-java 1.7.1 (2.8.2's) has no darwin aarch64 library either, but falls back to its pure-Java code by itself,
//     so it is kept as named.
// slf4j-simple is the logging binding (kafka-clients needs one, or it warns that there is none); 1.7.36 for both.
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

type Jar = { group: string; artifact: string; version: string; sha256: string };

const jar = (coords: string, sha256: string): Jar => {
  const [group, artifact, version] = coords.split(":");
  return { group, artifact, version, sha256 };
};

const slf4j = [
  jar("org.slf4j:slf4j-api:1.7.36", "d3ef575e3e4979678dc01bf1dcce51021493b4d11fb7f1be8ad982877c16a1c0"),
  jar("org.slf4j:slf4j-simple:1.7.36", "2f39bed943d624dfa8f4102d0571283a10870b6aa36f197a8a506f147010c10f"),
];
const snappy = jar("org.xerial.snappy:snappy-java:1.1.10.7", "4c766cb3f855415ee734b2392949a0b6f12a60879334a74518deaf6270d32e36");

const sets: Record<string, Jar[]> = {
  "4": [
    jar("org.apache.kafka:kafka-clients:4.4.0", "1ab07317eb6633241392449d329dbdc5d9251e6ffc4926860f42dd966e88bdf9"),
    jar("com.github.luben:zstd-jni:1.5.7-17", "c9043e2da8a16c13b6cd7661a5cb56685a573c3546ef4bd0ead67b0b67c2e107"),
    jar("at.yawk.lz4:lz4-java:1.11.2", "8a7458657938e9b692224db83aac458c13abd7a59ab75c3d3b08bd2233a646a2"),
    snappy,
    ...slf4j,
  ],
  "2.8": [
    jar("org.apache.kafka:kafka-clients:2.8.2", "0351de6b808f551e4c6cf230a83984d653e14776cc0f4621e7a3eab377e2f621"),
    jar("com.github.luben:zstd-jni:1.4.9-1", "16bb6f70b6577db165bda521a30349fcdc698055893833a993c4553eba71a61b"),
    jar("org.lz4:lz4-java:1.7.1", "f1167a45d4b8002053670ef6991ca66d1bab9dcc03e4ef00183674d2f3fb9cac"),
    snappy,
    ...slf4j,
  ],
};

// kafka-clients 3.9.1 for tools/old-profiles-oracle/Probe.java (debt K2, 2026-11-19): the last client that writes every version
// Kafka 1.1, 2.8 and 3.9 serve, v0 included, and builds message sets of format v0 and v1. Its SHA-1 matched Maven Central's
// .sha1 (86ca0799...). The compression libraries are the 4.4.0 set's: the same packages, and the ones with a darwin aarch64 library.
sets["3.9"] = [
  jar("org.apache.kafka:kafka-clients:3.9.1", "7568b998572d256f0b7bc0afdc1b7a2588b8b08415c62ce314c864a6851ae9d9"),
  ...sets["4"].slice(1),
];

// Kafka Streams 4.4.0 for Streams.java, checked against Maven Central's .sha1 and .sha256 on 2026-11-18. Its POM also names
// rocksdbjni (72 MB) and Jackson; the word count keeps its state in memory and loads neither, so they are not fetched.
sets["streams"] = [
  ...sets["4"],
  jar("org.apache.kafka:kafka-streams:4.4.0", "6e06548a119adf213121ad2e4fefff6ab4263875b4f761425ace8ea4f5ba04a6"),
];

const which = process.argv[2];
const set = sets[which];
if (!set) {
  console.error(`usage: bun interop/java/fetch.ts <${Object.keys(sets).join("|")}>`);
  process.exit(2);
}
const cache = join(process.env.DOZE_INTEROP_CACHE ?? join(homedir(), ".cache", "doze-interop"), "java");
mkdirSync(cache, { recursive: true });

const sha256 = (bytes: Uint8Array) => createHash("sha256").update(bytes).digest("hex");

const paths: string[] = [];
for (const j of set) {
  const file = `${j.artifact}-${j.version}.jar`;
  // The group goes in the name too: two lz4-java artifacts of different groups would otherwise share a file name.
  const path = join(cache, `${j.group}_${file}`);
  if (!existsSync(path) || sha256(readFileSync(path)) !== j.sha256) {
    const url = `https://repo1.maven.org/maven2/${j.group.replaceAll(".", "/")}/${j.artifact}/${j.version}/${file}`;
    const res = await fetch(url);
    if (!res.ok) {
      console.error(`${url}: HTTP ${res.status}`);
      process.exit(1);
    }
    const bytes = new Uint8Array(await res.arrayBuffer());
    const got = sha256(bytes);
    if (got !== j.sha256) {
      console.error(`${url}: SHA-256 ${got}, pinned ${j.sha256}`);
      process.exit(1);
    }
    writeFileSync(`${path}.part`, bytes);
    renameSync(`${path}.part`, path);
  }
  paths.push(path);
}
console.log(paths.join(":"));
