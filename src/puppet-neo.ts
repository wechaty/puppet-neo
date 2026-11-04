import { setTimeout as delay } from 'node:timers/promises'
import { Puppet, types, type payloads, type PuppetOptions } from 'wechaty-puppet'
import jsQR from 'jsqr'
import { PNG } from 'pngjs'
import { openNativeSession, type Session, type Snapshot } from './native.js'

const SESSION_KEY = 'puppet-neo.session'
const CACHE_LIMIT = 2048

function putRecent<T>(cache: Map<string, T>, id: string, value: T) {
  cache.delete(id)
  cache.set(id, value)
  if (cache.size > CACHE_LIMIT) {
    const oldest = cache.keys().next().value
    if (oldest !== undefined) cache.delete(oldest)
  }
}

function decodeQr(encoded: string) {
  const bytes = Buffer.from(encoded, 'base64')
  if (bytes.length < 24 || bytes.length > 1024 * 1024 ||
      bytes.readUInt32BE(16) > 1024 || bytes.readUInt32BE(20) > 1024) {
    throw new Error('Invalid Neo login QR image')
  }
  const png = PNG.sync.read(bytes)
  const pixels = new Uint8ClampedArray(png.data.buffer, png.data.byteOffset, png.data.byteLength)
  const qr = jsQR(pixels, png.width, png.height)
  if (!qr?.data) throw new Error('Could not decode the Neo login QR')
  return qr.data
}

export class PuppetNeo extends Puppet {
  static override readonly VERSION = '0.1.0'

  private run?: Promise<void>
  private abort?: AbortController
  private session?: Session
  private authenticated = false
  private online = false
  private pendingSends = new Set<Promise<unknown>>()
  private readonly contacts = new Map<string, payloads.Contact>()
  private readonly messages = new Map<string, payloads.Message>()
  private readonly unsupported = new Set<number>()

  constructor(options: PuppetOptions = {}) {
    super(options)
  }

  protected createSession(): Session {
    return openNativeSession()
  }

  protected scanCode(png: string): string {
    return decodeQr(png)
  }

  override async onStart(): Promise<void> {
    if (!this.memory.name) throw new Error('Configure a named Wechaty MemoryCard for Neo')
    const abort = new AbortController()
    const session = this.createSession()
    this.abort = abort
    this.session = session
    this.run = this.serve(session, abort.signal).catch((error: unknown) => {
      if (!abort.signal.aborted) this.emit('error', { data: error instanceof Error ? error.message : 'Neo session failed' })
    }).finally(() => {
      this.online = false
    })
  }

  override async onStop(): Promise<void> {
    const session = this.session
    this.abort?.abort()
    try {
      await this.run
      await Promise.allSettled(this.pendingSends)
      if (session && this.authenticated) await this.save(session)
    } finally {
      this.authenticated = false
      this.abort = undefined
      this.session = undefined
      this.run = undefined
      this.contacts.clear()
      this.messages.clear()
      this.unsupported.clear()
      if (this.isLoggedIn) await super.logout()
    }
  }

  override ding(data?: string): void {
    this.emit('dong', { data })
  }

  override async messageRawPayload(id: string): Promise<payloads.Message> {
    const message = this.messages.get(id)
    if (!message) throw new Error('Neo message payload is no longer cached')
    return message
  }

  override async messageRawPayloadParser(payload: payloads.Message): Promise<payloads.Message> {
    return payload
  }

  override async contactRawPayload(id: string): Promise<payloads.Contact> {
    const contact = this.contacts.get(id)
    if (!contact) throw new Error('Neo has no payload for this contact')
    return contact
  }

  override async contactRawPayloadParser(payload: payloads.Contact): Promise<payloads.Contact> {
    return payload
  }

  override async messageSendText(destination: string, text: string, mentions?: string[]): Promise<void> {
    if (!this.session || !this.online || this.abort?.signal.aborted) throw new Error('Neo session is offline')
    if (mentions?.length || destination.endsWith('@chatroom') || !this.contacts.has(destination)) {
      throw new Error('Neo supports direct text to an observed peer')
    }
    const send = this.session.sendText(destination, text)
    this.pendingSends.add(send)
    try {
      const result = await send
      if (result.status === 'uncertain') throw new Error('WeChat text send outcome is uncertain')
      if (result.status === 'rejected') throw new Error('WeChat rejected the text send')
    } finally {
      this.pendingSends.delete(send)
    }
  }

  private rememberContact(id: string): void {
    putRecent(this.contacts, id, {
      id, name: id, avatar: '', phone: [], type: types.Contact.Unknown, gender: types.ContactGender.Unknown,
    })
  }

  private async save(session: Session): Promise<void> {
    await this.memory.set(SESSION_KEY, await session.snapshot())
    await this.memory.save()
  }

  private async loginWithQr(session: Session, signal: AbortSignal): Promise<string> {
    const qr = this.scanCode((await session.startLogin()).png)
    signal.throwIfAborted()
    this.emit('scan', { qrcode: qr, status: types.ScanStatus.Waiting })
    let scanned = false
    for (let attempt = 0; attempt < 120; attempt++) {
      await delay(1000, undefined, { signal })
      const result = await session.pollLogin()
      signal.throwIfAborted()
      if (result.status === 'completed') {
        this.emit('scan', { status: types.ScanStatus.Confirmed })
        return result.username
      }
      if (result.status === 'scanned' && !scanned) {
        scanned = true
        this.emit('scan', { status: types.ScanStatus.Scanned })
      }
      if (result.status === 'expired' || result.status === 'cancelled' || result.status === 'rejected') {
        this.emit('scan', { status: result.status === 'expired' ? types.ScanStatus.Timeout : types.ScanStatus.Cancel })
        throw new Error(`Neo QR login ended: ${result.status}`)
      }
    }
    this.emit('scan', { status: types.ScanStatus.Timeout })
    throw new Error('Neo QR login timed out')
  }

  private async serve(session: Session, signal: AbortSignal): Promise<void> {
    await delay(0, undefined, { signal })
    const stored = await this.memory.get<Snapshot>(SESSION_KEY)
    const username = stored == null
      ? await this.loginWithQr(session, signal)
      : (await session.restore(stored)).username
    signal.throwIfAborted()
    await this.save(session)
    signal.throwIfAborted()
    this.authenticated = true
    this.online = true
    this.rememberContact(username)
    await this.login(username)
    const associated = await session.heartbeat()
    signal.throwIfAborted()
    let nextHeartbeat = performance.now() + associated.intervalMs
    this.emit('ready', {})
    await this.drain(session, 6, signal)
    while (!signal.aborted) {
      if (performance.now() >= nextHeartbeat) {
        const beat = await session.heartbeat()
        signal.throwIfAborted()
        nextHeartbeat = performance.now() + beat.intervalMs
        if (beat.sync) await this.drain(session, 7, signal)
      }
      if (await session.pollIncoming()) await this.drain(session, 1, signal)
    }
  }

  private async drain(session: Session, scene: 1 | 6 | 7, signal: AbortSignal): Promise<void> {
    let more: boolean
    do {
      signal.throwIfAborted()
      const page = await session.sync(scene)
      signal.throwIfAborted()
      for (const code of page.unsupported) {
        if (this.unsupported.has(code)) continue
        this.unsupported.add(code)
        this.emit('error', { data: `Neo retained sync command ${code}; the saved checkpoint may lag` })
      }
      for (const item of page.messages) {
        if (this.messages.has(item.id)) continue
        this.rememberContact(item.from)
        this.rememberContact(item.to)
        putRecent(this.messages, item.id, {
          id: item.id, talkerId: item.from, listenerId: item.to,
          text: item.text, timestamp: item.timestamp, type: types.Message.Text,
        })
        this.emit('message', { messageId: item.id })
      }
      await session.finishPage()
      more = page.continuation
    } while (more)
  }
}
