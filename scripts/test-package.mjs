import { spawnSync } from 'node:child_process'
import { copyFileSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const scratch = mkdtempSync(join(tmpdir(), 'neo-pack-'))
function run(command, args, cwd, capture = false) {
  const result = spawnSync(command, args, {
    cwd,
    stdio: capture ? ['ignore', 'pipe', 'inherit'] : 'inherit',
    encoding: 'utf8',
  })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`${command} ${args[0]} failed`)
  return result.stdout
}
try {
  const [pack] = JSON.parse(run('npm', ['pack', '--json', '--pack-destination', scratch], root, true))
  for (const expected of ['dist/index.js', 'native/neo.node', 'native/THIRD-PARTY.txt', 'docs/v1-spec.md']) {
    if (!pack.files.some(file => file.path === expected)) throw new Error(`Packed artifact is missing ${expected}`)
  }
  writeFileSync(join(scratch, 'package.json'), '{"name":"neo-package-test","private":true}')
  run('npm', ['install', '--ignore-scripts', '--no-audit', join(scratch, pack.filename), 'wechaty@1.20.2'], scratch)
  for (const test of ['ding-dong.test.mjs', 'native.test.mjs']) copyFileSync(join(root, 'test', test), join(scratch, test))
  run(process.execPath, ['--test', 'ding-dong.test.mjs', 'native.test.mjs'], scratch)
} finally {
  rmSync(scratch, { recursive: true, force: true })
}
