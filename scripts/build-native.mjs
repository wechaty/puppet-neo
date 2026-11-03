import { copyFileSync, readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { execFileSync, spawnSync } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
if (process.platform !== 'linux' || process.arch !== 'x64' || !process.report.getReport().header.glibcVersionRuntime) {
  throw new Error('Neo currently builds for Linux x64 with glibc')
}
const { status, error } = spawnSync('cargo', ['build', '--release', '--locked', '--manifest-path', 'native/Cargo.toml', '--target-dir', 'native/target'], {
  cwd: root,
  stdio: 'inherit',
})
if (error) throw error
if (status !== 0) process.exit(status ?? 1)
copyFileSync(new URL('../native/target/release/libpuppet_neo_native.so', import.meta.url), new URL('../native/neo.node', import.meta.url))

const metadata = JSON.parse(execFileSync('cargo', [
  'metadata', '--locked', '--format-version', '1', '--manifest-path', 'native/Cargo.toml',
  '--filter-platform', 'x86_64-unknown-linux-gnu',
], { cwd: root, encoding: 'utf8' }))
const notices = metadata.packages
  .filter(pkg => pkg.source)
  .sort((a, b) => a.name.localeCompare(b.name))
  .map(pkg => {
    const directory = dirname(pkg.manifest_path)
    const paths = new Set(readdirSync(directory, { withFileTypes: true })
      .filter(entry => entry.isFile() && /^(license|licence|notice|copying|copyright|unlicense)([._-].*)?$/i.test(entry.name))
      .map(entry => entry.name))
    if (pkg.license_file) paths.add(pkg.license_file)
    if (pkg.name === 'openssl-src') paths.add('openssl/LICENSE.txt')
    return [`${pkg.name} ${pkg.version} (${pkg.license ?? 'see license text'})`, pkg.repository,
      ...[...paths].sort().map(path => readFileSync(join(directory, path), 'utf8'))].filter(Boolean).join('\n\n')
  })
writeFileSync(new URL('../native/THIRD-PARTY.txt', import.meta.url), notices.join('\n\n---\n\n'))
