import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFile, writeFile, unlink, mkdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const project = path.resolve(here, '../..');
const upstream = path.resolve(process.argv[2] ?? path.join(project, '../mdsync-validation'));
const pinned = '6f0190efd517b709ff21c4d77bcc0b221bce52b6';
const head = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: upstream, encoding: 'utf8' });
assert.equal(head.status, 0, 'Clone MDSync and install its pinned dependencies first; see README.md');
assert.equal(head.stdout.trim(), pinned, 'Use the reviewed MDSync commit');
const status = spawnSync('git', ['status', '--porcelain', '--untracked-files=no'], { cwd: upstream, encoding: 'utf8' });
assert.equal(status.status, 0);
assert.equal(status.stdout.trim(), '', 'Preserve local changes: use a clean MDSync checkout');
const plugin = path.join(upstream, 'packages/plugin');
const target = path.join(plugin, 'tests/features/sync/miaoyan-spike.test.ts');
const fixture = path.join(plugin, 'tests/features/sync/s3-loopback.fixture.ts');
const output = path.join(project, 'artifacts/mdsync-spike');
await mkdir(output, { recursive: true });
// Use the upstream HTTP fixture on an ephemeral loopback port; it does not verify S3 signatures.
const server = await readFile(path.join(upstream, 'tests/e2e/s3.ts'), 'utf8');
assert(server.includes('server.listen(port, resolve)'));
assert(server.includes('url: `http://127.0.0.1:${port}`'));
const loopback = server
  .replace('server.listen(port, resolve)', 'server.listen(port, "127.0.0.1", resolve)')
  .replace('url: `http://127.0.0.1:${port}`', 'url: `http://127.0.0.1:${(server.address() as import("node:net").AddressInfo).port}`');
const cases = (await readFile(path.join(here, 'spike-cases.ts'), 'utf8'))
  .replace("'__MIAOYAN_NOTEBOOK__'", JSON.stringify(path.join(project, 'src/notebook.ts').replaceAll('\\', '/')));
let wroteFixture = false, wroteCases = false;
try {
  await writeFile(fixture, loopback, { flag: 'wx' });
  wroteFixture = true;
  await writeFile(target, cases, { flag: 'wx' });
  wroteCases = true;
  const result = spawnSync('pnpm', ['--filter', 'mdsync', 'test', 'tests/features/sync/miaoyan-spike.test.ts',
    '--reporter=default', '--reporter=json', `--outputFile=${path.join(output, 'probe-results.json')}`],
  { cwd: upstream, stdio: 'inherit' });
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
} finally {
  if (wroteCases) await unlink(target);
  if (wroteFixture) await unlink(fixture);
}
