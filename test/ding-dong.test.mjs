import assert from 'node:assert/strict'
import { once } from 'node:events'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { WechatyBuilder, ScanStatus } from 'wechaty'
import PuppetNeo from 'wechaty-puppet-neo'

const syntheticEnvelope = { version: 1, nonce: 'synthetic-nonce', ciphertext: 'synthetic-sealed-record' }
const username = 'fixture-self'
const sender = 'fixture-peer'

function scriptedPuppet(id, result, trace) {
  const session = {
    async startLogin() {
      trace.push('start-login')
      return { png: 'fixture-image' }
    },
    async pollLogin() {
      return { status: 'completed', username }
    },
    async restore(snapshot) {
      trace.push('restore')
      assert.deepEqual(snapshot, syntheticEnvelope)
      return { username }
    },
    async snapshot() {
      return syntheticEnvelope
    },
    async heartbeat() {
      return { sync: false, intervalMs: 5000 }
    },
    async sync() {
      return {
        messages: [{ id, from: sender, to: username, text: 'ding', timestamp: 1700000000 }],
        unsupported: [],
        continuation: false,
      }
    },
    async finishPage() {},
    async sendText(destination, text) {
      trace.push(['send', destination, text])
      return { status: result }
    },
  }
  return new class extends PuppetNeo {
    createSession() { return session }
    scanCode() { return 'fixture-qr' }
  }()
}

test('real Wechaty delivers text, replies once, and restores a named MemoryCard', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'neo-test-'))
  const card = join(dir, 'bot')
  try {
    for (const run of [1, 2]) {
      const trace = []
      const scans = []
      const errors = []
      const result = run === 1 ? 'accepted' : 'uncertain'
      const bot = WechatyBuilder.build({ name: card, puppet: scriptedPuppet(String(run), result, trace) })
      bot.on('scan', (_qr, status) => scans.push(status))
      bot.on('error', error => errors.push(error))
      const delivery = once(bot, 'message', { signal: AbortSignal.timeout(5000) })
      await bot.start()
      try {
        const [message] = await delivery
        assert.equal(message.text(), 'ding')
        assert.equal(message.talker().id, sender)
        if (result === 'accepted') assert.equal(await message.say('dong'), undefined)
        else await assert.rejects(message.say('dong'), /outcome is uncertain/)
        assert.deepEqual(trace, [run === 1 ? 'start-login' : 'restore', ['send', sender, 'dong']])
        assert.equal(scans.includes(ScanStatus.Waiting), run === 1)
        assert.deepEqual(errors, [])
      } finally {
        await bot.stop()
      }
    }
    const persisted = JSON.parse(await readFile(`${card}.memory-card.json`, 'utf8'))
    assert.deepEqual(Object.values(persisted), [syntheticEnvelope])
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
})
