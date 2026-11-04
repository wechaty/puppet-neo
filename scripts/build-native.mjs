import { copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { execFileSync, spawnSync } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import platforms from '../src/native-platforms.json' with { type: 'json' }

const root = fileURLToPath(new URL('../', import.meta.url))
const platform = `${process.platform}-${process.arch}`
const config = platforms[platform]
if (!config) throw new Error(`Neo has no native build for ${platform}`)
if (process.platform === 'linux' && !process.report.getReport().header.glibcVersionRuntime) {
  throw new Error('Neo requires glibc on Linux')
}
const { status, error } = spawnSync('cargo', [
  'build', '--release', '--locked', '--manifest-path', 'native/Cargo.toml',
  '--target-dir', 'native/target', '--target', config.target,
], { cwd: root, stdio: 'inherit' })
if (error) throw error
if (status !== 0) process.exit(status ?? 1)

const metadata = JSON.parse(execFileSync('cargo', [
  'metadata', '--locked', '--format-version', '1', '--manifest-path', 'native/Cargo.toml',
  '--filter-platform', config.target,
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
const output = join(root, 'native', platform)
mkdirSync(output, { recursive: true })
copyFileSync(join(root, 'native', 'target', config.target, 'release', config.binary), join(output, 'neo.node'))
writeFileSync(join(output, 'THIRD-PARTY.txt'), notices.join('\n\n---\n\n'))
