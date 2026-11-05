import assert from 'node:assert/strict'
import { once } from 'node:events'
import { setTimeout as delay } from 'node:timers/promises'
import { test } from 'node:test'
import { types, type payloads } from 'wechaty-puppet'
import qrTerminal from 'qrcode-terminal'
import PuppetNeo from 'wechaty-puppet-neo'
import type { Session } from '../src/native.js'

const username = 'fixture-self'
const sender = 'fixture-peer'

function scriptedSession(trace: unknown[], result: 'accepted' | 'uncertain' = 'accepted'): Session {
  let notified = false
  return {
    async startLogin() {
      trace.push('start-login')
      return { png: 'fixture-image' }
    },
    async pollLogin() {
      return { status: 'completed', username }
    },
    async heartbeat() {
      return { sync: false, intervalMs: 5000 }
    },
    async pollIncoming() {
      if (notified) {
        await delay(15)
        return false
      }
      notified = true
      return true
    },
    async sync(scene) {
      trace.push(['sync', scene])
      if (scene === 6) return { messages: [], continuation: false }
      assert.equal(scene, 1)
      return {
        messages: [{ id: 'fixture-message', from: sender, to: username, text: 'ding', timestamp: 1700000000 }],
        continuation: false,
      }
    },
    async sendText(destination, text) {
      trace.push(['send', destination, text])
      return { status: result }
    },
  }
}

test('the raw Puppet API delivers text and scans again after stopping', async () => {
  const trace: unknown[] = []
  const scans: payloads.EventScan[] = []
  const errors: payloads.EventError[] = []
  let result: 'accepted' | 'uncertain' = 'accepted'
  const puppet = new class extends PuppetNeo {
    protected override createSession() { return scriptedSession(trace, result) }
    protected override scanCode() { return 'fixture-qr' }
  }()
  puppet.on('scan', payload => { scans.push(payload) })
  puppet.on('error', payload => { errors.push(payload) })
  for (const attempt of [1, 2]) {
    trace.length = 0
    scans.length = 0
    const delivery = once(puppet, 'message', { signal: AbortSignal.timeout(5000) })
    await puppet.start()
    try {
      const [{ messageId }] = await delivery
      const message = await puppet.messagePayload(messageId)
      assert.equal(puppet.currentUserId, username)
      assert.equal(message.text, 'ding')
      assert.equal(message.talkerId, sender)
      if (attempt === 1) await puppet.messageSendText(sender, 'dong')
      else await assert.rejects(puppet.messageSendText(sender, 'dong'), /outcome is uncertain/)
      assert.deepEqual(trace, ['start-login', ['sync', 6], ['sync', 1], ['send', sender, 'dong']])
      assert(scans.some(scan => scan.status === types.ScanStatus.Waiting && scan.qrcode === 'fixture-qr'))
      assert.equal(puppet.memory.name, undefined)
      assert.equal(await puppet.memory.size, 0)
      assert.deepEqual(errors, [])
    } finally {
      await puppet.stop()
    }
    await assert.rejects(puppet.messageSendText(sender, 'dong'), /offline/)
    result = 'uncertain'
  }
})

test('the TypeScript example replies through PuppetNeo directly', async t => {
  const trace: unknown[] = []
  const originalCreateSession = Reflect.get(PuppetNeo.prototype, 'createSession')
  const originalScanCode = Reflect.get(PuppetNeo.prototype, 'scanCode')
  let puppet: PuppetNeo | undefined
  Reflect.set(PuppetNeo.prototype, 'createSession', function (this: PuppetNeo) {
    puppet = this
    return scriptedSession(trace)
  })
  Reflect.set(PuppetNeo.prototype, 'scanCode', () => 'fixture-qr')
  const qr = t.mock.method(qrTerminal, 'generate', () => {})
  const output = t.mock.method(console, 'info', () => {})
  const errors = t.mock.method(console, 'error', () => {})
  t.after(async () => {
    await puppet?.stop()
    Reflect.set(PuppetNeo.prototype, 'createSession', originalCreateSession)
    Reflect.set(PuppetNeo.prototype, 'scanCode', originalScanCode)
  })

  await import('../examples/ding-dong-bot.ts')
  assert(puppet)
  const deadline = Date.now() + 5000
  while (!output.mock.calls.some(call => call.arguments[0] === 'Sent: dong')) {
    assert(Date.now() < deadline, 'Example did not finish its reply')
    await delay(15)
  }
  assert.deepEqual(qr.mock.calls[0]?.arguments, ['fixture-qr', { small: true }])
  assert.deepEqual(trace, ['start-login', ['sync', 6], ['sync', 1], ['send', sender, 'dong']])
  assert.equal(errors.mock.callCount(), 0)
  assert(output.mock.calls.every(call => !String(call.arguments[0]).includes(sender) && !String(call.arguments[0]).includes(username)))
})
