import assert from 'node:assert/strict'
import { randomBytes } from 'node:crypto'
import { test } from 'node:test'
import PuppetNeo from 'wechaty-puppet-neo'

test('the compiled addon rejects invalid MemoryCard state before network I/O', async () => {
  process.env.PUPPET_NEO_SESSION_KEY = randomBytes(32).toString('base64')
  const session = new PuppetNeo().createSession()
  await assert.rejects(session.restore({
    version: 1,
    nonce: randomBytes(12).toString('base64'),
    ciphertext: randomBytes(32).toString('base64'),
  }), /restore the saved session/)
  await assert.rejects(session.snapshot(), /seal the session snapshot/)
})
