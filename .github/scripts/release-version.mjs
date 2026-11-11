import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';

const input = process.env.RELEASE_VERSION ?? '';
assert.match(input, /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/, 'Enter a stable version such as 0.1.1 or v0.1.1');
const version = input.replace(/^v/, '');
const files = ['package.json', 'package-lock.json', 'src-tauri/tauri.conf.json', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock'];
const source = Object.fromEntries(files.map(file => [file, readFileSync(file, 'utf8')]));
const pkg = JSON.parse(source['package.json']);
const lock = JSON.parse(source['package-lock.json']);
const config = JSON.parse(source['src-tauri/tauri.conf.json']);
const cargoVersion = /^version = "([^"]+)"$/m;
const cargoLockVersion = /^(name = "qingyu"\nversion = ")([^"]+)(")$/m;
for (const current of [lock.version, lock.packages[''].version, config.version,
  source['src-tauri/Cargo.toml'].match(cargoVersion)?.[1],
  source['src-tauri/Cargo.lock'].match(cargoLockVersion)?.[2]]) {
  assert.equal(current, pkg.version, 'Source version files must agree before release');
}
pkg.version = lock.version = lock.packages[''].version = version;
const updated = {
  'package.json': JSON.stringify(pkg, null, 2) + '\n',
  'package-lock.json': JSON.stringify(lock, null, 2) + '\n',
  'src-tauri/tauri.conf.json': source['src-tauri/tauri.conf.json'].replace(/("version"\s*:\s*")[^"]+(")/, (_, prefix, suffix) => prefix + version + suffix),
  'src-tauri/Cargo.toml': source['src-tauri/Cargo.toml'].replace(cargoVersion, `version = "${version}"`),
  'src-tauri/Cargo.lock': source['src-tauri/Cargo.lock'].replace(cargoLockVersion, (_, prefix, old, suffix) => prefix + version + suffix),
};
for (const file of files) writeFileSync(file, updated[file]);
console.log(`v${version}`);
