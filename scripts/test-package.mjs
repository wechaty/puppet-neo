import { spawnSync } from 'node:child_process'
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import platforms from '../src/native-platforms.json' with { type: 'json' }

const root = fileURLToPath(new URL('../', import.meta.url))
const npmCli = process.env.npm_execpath
if (!npmCli) throw new Error('Run this check with npm run test:package')
const destination = process.argv[2] ? resolve(process.argv[2]) : undefined
const platform = `${process.platform}-${process.arch}`
const requiredPlatforms = destination ? Object.keys(platforms) : [platform]
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
  const [pack] = JSON.parse(run(process.execPath, [npmCli, 'pack', '--json', '--pack-destination', scratch], root, true))
  const expectedFiles = [
    'dist/index.js', 'dist/native-platforms.json', 'docs/v1-spec.md',
    ...requiredPlatforms.flatMap(key => [`native/${key}/neo.node`, `native/${key}/THIRD-PARTY.txt`]),
  ]
  for (const expected of expectedFiles) {
    if (!pack.files.some(file => file.path === expected)) throw new Error(`Packed artifact is missing ${expected}`)
  }
  writeFileSync(join(scratch, 'package.json'), '{"name":"neo-package-test","private":true}')
  // Wechaty 1.20.2's LevelDOWN dependency has no Windows ARM64 prebuild.
  const installFlags = platform === 'win32-arm64' ? [] : ['--ignore-scripts']
  run(process.execPath, [npmCli, 'install', ...installFlags, '--no-audit', join(scratch, pack.filename), 'wechaty@1.20.2'], scratch)
  for (const test of ['ding-dong.test.mjs', 'native.test.mjs']) copyFileSync(join(root, 'test', test), join(scratch, test))
  run(process.execPath, ['--test', 'ding-dong.test.mjs', 'native.test.mjs'], scratch)
  if (destination) {
    mkdirSync(destination, { recursive: true })
    copyFileSync(join(scratch, pack.filename), join(destination, pack.filename))
  }
} finally {
  rmSync(scratch, { recursive: true, force: true })
}
