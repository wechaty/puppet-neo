import { createRequire } from 'node:module'

export interface Snapshot {
  version: number
  nonce: string
  ciphertext: string
}

export interface IncomingText {
  id: string
  from: string
  to: string
  text: string
  timestamp: number
}

export interface Session {
  startLogin(): Promise<{ png: string }>
  pollLogin(): Promise<
    | { status: 'pending' | 'scanned' | 'retry' | 'expired' | 'cancelled' | 'rejected' }
    | { status: 'completed', username: string }
  >
  restore(snapshot: Snapshot): Promise<{ username: string }>
  snapshot(): Promise<Snapshot>
  heartbeat(): Promise<{ sync: boolean, intervalMs: number }>
  sync(initial: boolean): Promise<{ messages: IncomingText[], unsupported: number[], continuation: boolean }>
  finishPage(): Promise<null>
  sendText(destination: string, text: string): Promise<{ status: 'accepted' | 'rejected' | 'uncertain' }>
}

type Addon = { [K in keyof Session]: (...args: Parameters<Session[K]>) => Promise<string> }

export function openNativeSession(): Session {
  if (process.platform !== 'linux' || process.arch !== 'x64' || !(process.report.getReport() as { header: { glibcVersionRuntime?: string } }).header.glibcVersionRuntime) {
    throw new Error('This Neo build supports Linux x64 with glibc')
  }
  const require = createRequire(import.meta.url)
  const { NeoSession } = require('../native/neo.node') as { NeoSession: new () => Addon }
  const addon = new NeoSession()

  return {
    startLogin: async () => JSON.parse(await addon.startLogin()),
    pollLogin: async () => JSON.parse(await addon.pollLogin()),
    restore: async (snapshot) => JSON.parse(await addon.restore(snapshot)),
    snapshot: async () => JSON.parse(await addon.snapshot()),
    heartbeat: async () => JSON.parse(await addon.heartbeat()),
    sync: async (initial) => JSON.parse(await addon.sync(initial)),
    finishPage: async () => JSON.parse(await addon.finishPage()),
    sendText: async (destination, text) => JSON.parse(await addon.sendText(destination, text)),
  }
}
