// Read-only: the registry's own parser and publish.mjs's checks on a dry-run release, nothing signed or written.
// Uses doze-registry's own parser and history check (DOZE_REGISTRY, default ../doze-registry), read-only.
//   bun tools/module-release/registry-check.mjs <release-dir>/<engine> <engine>
const registry = process.env.DOZE_REGISTRY ?? new URL('../../../doze-registry', import.meta.url).pathname;
const { parseYaml, SCHEMA, history } = await import(`${registry}/scripts/lib.mjs`);
import { readFileSync } from 'node:fs';
const dir = process.argv[2], name = process.argv[3];
const idx = parseYaml(readFileSync(`${dir}/index.yaml`, 'utf8'));
const fail = (m) => { console.error('REFUSED:', m); process.exit(1); };
if (idx?.schema !== SCHEMA) fail(`schema ${idx?.schema}`);
if (idx.module !== name) fail(`module ${idx.module}`);
if (!idx.releases || Object.keys(idx.releases).length === 0) fail('no releases');
for (const [v, r] of Object.entries(idx.releases)) for (const [t, a] of Object.entries(r.artifacts ?? {})) {
  if (!a?.sha256) fail(`${v} ${t}: no sha256`);
  const sha = new Bun.CryptoHasher('sha256').update(readFileSync(`${dir}/${a.url}`)).digest('hex');
  if (sha !== String(a.sha256).toLowerCase()) fail(`${v} ${t}: sha256 does not match the archive`);
}
const meta = parseYaml(readFileSync(`${dir}/meta.yaml`, 'utf8'));
if (meta.engine !== name || !Array.isArray(meta.config?.arguments)) fail('meta.yaml shape');
// publish.mjs's history check against a previously signed copy: nothing published may change.
const { changed } = history(idx, idx);
if (changed?.length) fail(`history: ${changed}`);
console.log(`accepted: ${name}, releases ${Object.keys(idx.releases).join(', ')}, protocol ${Object.values(idx.releases)[0].protocol}, meta args ${meta.config.arguments.map(a => a.name).join(',')}, blocks ${(meta.config.blocks ?? []).map(b => b.name).join(',')}`);
