import assert from 'node:assert/strict'
import { test } from 'node:test'
import PuppetNeo from 'wechaty-puppet-neo'

test('the compiled addon starts without a storage key and rejects unauthenticated operations', async () => {
  const puppet = new class extends PuppetNeo {
    openSession() { return this.createSession() }
  }()
  const session = puppet.openSession()
  await assert.rejects(session.heartbeat(), /check session heartbeat/)
  await assert.rejects(session.sendText('fixture-peer', 'ding'), /prepare a text send/)
})
