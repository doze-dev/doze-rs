#!/usr/bin/env bun
// Prints what AWS answered in a recorded scenario: bun tools/conformance/snap.ts sqs test_batch_refusals [more tests]
const [module_, ...tests] = process.argv.slice(2);
const doc = JSON.parse(await Bun.file(`${import.meta.dir}/../../conformance/snapshots/test_${module_}.json`).text());
for (const test of tests) {
  console.log(`== ${test}`);
  for (const [label, v] of Object.entries<any>(doc[test] ?? {})) {
    if (label === "__meta") continue;
    const e = v?.Error;
    const status = v?.ResponseMetadata?.HTTPStatusCode;
    console.log(`  ${label.padEnd(36)} ${e ? `${e.Code} ${status} | ${e.Message}` : JSON.stringify(v).slice(0, 230)}`);
  }
}
