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
const env = { ...process.env }
// npm rejects a project install with the parent npm script's allow-scripts flag.
delete env.npm_config_allow_scripts
function run(command, args, cwd, capture = false) {
  const result = spawnSync(command, args, {
    cwd,
    env,
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
    'dist/index.js', 'dist/native-platforms.json', 'docs/v1-spec.md', 'examples/ding-dong-bot.ts',
    ...requiredPlatforms.flatMap(key => [`native/${key}/neo.node`, `native/${key}/THIRD-PARTY.txt`]),
  ]
  for (const expected of expectedFiles) {
    if (!pack.files.some(file => file.path === expected)) throw new Error(`Packed artifact is missing ${expected}`)
  }
  writeFileSync(join(scratch, 'package.json'), '{"name":"neo-package-test","private":true,"type":"module"}')
  run(process.execPath, [npmCli, 'install', '--ignore-scripts', '--no-audit', join(scratch, pack.filename)], scratch)
  mkdirSync(join(scratch, 'test'))
  mkdirSync(join(scratch, 'examples'))
  const tests = ['ding-dong.test.ts', 'native.test.ts']
  for (const test of tests) copyFileSync(join(root, 'test', test), join(scratch, 'test', test))
  copyFileSync(join(scratch, 'node_modules', 'wechaty-puppet-neo', 'examples', 'ding-dong-bot.ts'), join(scratch, 'examples', 'ding-dong-bot.ts'))
  run(process.execPath, ['--test', ...tests.map(test => join('test', test))], scratch)
  if (destination) {
    mkdirSync(destination, { recursive: true })
    copyFileSync(join(scratch, pack.filename), join(destination, pack.filename))
  }
} finally {
  rmSync(scratch, { recursive: true, force: true })
}
