import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

const repo = new URL('../../', import.meta.url);
const script = fileURLToPath(new URL('./release-version.mjs', import.meta.url));
const files = ['package.json', 'package-lock.json', 'src-tauri/tauri.conf.json', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock'];
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'qingyu-release-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, 'src-tauri'));
  for (const file of files) writeFileSync(join(root, file), readFileSync(new URL(file, repo)));
  return root;
}
const run = (root, version) => spawnSync(process.execPath, [script], {
  cwd: root, env: { ...process.env, RELEASE_VERSION: version }, encoding: 'utf8',
});

for (const input of ['0.2.3', 'v0.2.3']) test(`synchronizes all version files for ${input}`, t => {
  const root = fixture(t);
  const before = Object.fromEntries(files.map(file => [file, readFileSync(join(root, file), 'utf8')]));
  const previous = JSON.parse(before['package.json']).version;
  const result = run(root, input);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), 'v0.2.3');
  for (const file of files) {
    const text = readFileSync(join(root, file), 'utf8');
    if (file.endsWith('.json')) {
      const data = JSON.parse(text);
      const original = JSON.parse(before[file]);
      assert.equal(data.version, '0.2.3');
      data.version = original.version;
      if (file === 'package-lock.json') {
        assert.equal(data.packages[''].version, '0.2.3');
        data.packages[''].version = original.packages[''].version;
      }
      assert.deepEqual(data, original, 'Other configuration and dependencies must stay unchanged');
    } else {
      assert.equal(text.replace('version = "0.2.3"', `version = "${previous}"`), before[file]);
    }
  }
});

test('rejects invalid versions without changing files', t => {
  const root = fixture(t);
  const before = files.map(file => readFileSync(join(root, file), 'utf8'));
  for (const input of ['', '01.2.3', '1.2', '1.2.3-beta', '1.2.3\nother=value', '$(touch injected)']) {
    assert.notEqual(run(root, input).status, 0, input);
    assert.deepEqual(files.map(file => readFileSync(join(root, file), 'utf8')), before);
  }
});

test('rejects inconsistent source versions before writing any file', t => {
  const root = fixture(t);
  const path = join(root, 'src-tauri/Cargo.lock');
  writeFileSync(path, readFileSync(path, 'utf8').replace(/(name = "qingyu"\nversion = ")[^"]+/, '$1inconsistent'));
  const before = files.map(file => readFileSync(join(root, file), 'utf8'));
  const result = run(root, '0.2.3');
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Source version files must agree/);
  assert.deepEqual(files.map(file => readFileSync(join(root, file), 'utf8')), before);
});

test('creates and reuses matching tags, preserves the source branch, and rejects different source', t => {
  const root = fixture(t);
  const git = (...args) => {
    const result = spawnSync('git', args, { cwd: root, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout.trim();
  };
  git('init', '-b', 'main');
  git('config', 'user.name', 'Release test');
  git('config', 'user.email', 'test@example.invalid');
  git('add', ...files);
  git('commit', '-m', 'source');
  const source = git('rev-parse', 'HEAD');
  git('init', '--bare', 'remote.git');
  git('remote', 'add', 'origin', join(root, 'remote.git'));
  git('push', 'origin', 'main');
  git('checkout', '--detach', source);
  assert.equal(run(root, '0.2.3').status, 0);
  const workflow = readFileSync(new URL('../workflows/release.yml', import.meta.url), 'utf8');
  const block = workflow.split('      - name: Create or verify release tag\n')[1]
    .split('      - name: Create or resume draft release\n')[0].split('        run: |\n')[1];
  const shell = block.split('\n').map(line => line.slice(10)).join('\n');
  const tag = () => spawnSync('bash', ['-e', '-o', 'pipefail', '-c', shell], {
    cwd: root, env: { ...process.env, RELEASE_TAG: 'v0.2.3', GITHUB_OUTPUT: join(root, 'output') }, encoding: 'utf8',
  });
  let result = tag();
  assert.equal(result.status, 0, result.stderr);
  const commit = git('rev-parse', 'v0.2.3^{commit}');
  assert.equal(git('ls-remote', 'origin', 'refs/tags/v0.2.3^{}').split(/\s/)[0], commit);
  assert.equal(git('ls-remote', 'origin', 'refs/heads/main').split(/\s/)[0], source);
  assert.equal(readFileSync(join(root, 'output'), 'utf8').trim(), `commit=${commit}`);
  git('reset', '--hard', source);
  assert.equal(run(root, '0.2.3').status, 0);
  result = tag();
  assert.equal(result.status, 0, result.stderr);
  writeFileSync(join(root, 'different-source.txt'), 'changed');
  git('add', 'different-source.txt');
  result = tag();
  assert.notEqual(result.status, 0);
  assert.match(result.stdout, /existing tag points to different source/);
  assert.equal(git('rev-parse', 'v0.2.3^{commit}'), commit);
});
