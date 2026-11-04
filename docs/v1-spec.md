# Puppet Neo v1 specification

Puppet Neo is a local WeChat protocol Puppet for Wechaty. Its long-term goal is comprehensive protocol coverage through open source. The first development milestone covers QR login, direct text, a clean stop, and session resume in a new Node process.

## First acceptance gate

Start with one Linux x64 Wechaty bot. Use an isolated test account and confirm the separate test peer before sending. Install the Neo tarball in a fresh Node project, run the Ding Dong example, scan the QR code, and send the unique challenge from that peer. Confirm the official recipient client received exactly one matching reply. Stop the bot and run it in a fresh process with the same MemoryCard and key. Confirm the same account resumes without a scan and repeat the exchange with a different challenge.

Follow this with the offline tests for corrupted or incorrectly keyed snapshots and lost send responses. A clean restart is the milestone's recovery promise. A process crash can leave an older checkpoint, and Wechaty event delivery has no acknowledgment for the bot's business logic.

## Architecture

```text
Wechaty bot
  -> PuppetNeo (TypeScript, Wechaty events and payload lookup)
  -> NeoSession (.node, one Rust owner for account credentials and protocol operations)
  -> wechat-protocol = 1.4.0 (published Cargo source)
  -> WeChat servers
```

Neo builds the addon from pinned crate sources. Cargo.lock and package-lock.json pin build inputs. Blocking native calls run off the Node event loop and a Rust lock serializes session access. TypeScript schedules login polling, sync and heartbeat, maps incoming text, and saves sealed snapshots using the supplied Wechaty MemoryCard. After authentication, Neo associates a persistent incoming connection with the account. It drains initial catch-up, then requests NewSync when that connection signals message work. A heartbeat can also signal pending work.

For protocol contributors, follow `messageSendText()` in [the Puppet](../src/puppet-neo.ts) into the [native session](../native/src/lib.rs), then see `send_text` and `text_send_request` in the [published Rust source](https://docs.rs/crate/wechat-protocol/1.4.0/source/src/). Incoming text follows the reverse direction through NewSync and AddMsg decoding. Neo publishes only direct text events with a known server message ID. Keep message IDs as strings. Store the payload in the bounded memory cache before emitting its ID so Wechaty can hydrate its Message object.

QR challenge state, current credentials, received payloads, and send attempts stay in memory. An accepted text send resolves with no Message object. An uncertain send is surfaced to the caller and submits only once. The example limits sends to one explicitly selected peer and one fresh challenge per process. It ignores self messages and rooms. Session work ends if polling, heartbeat or sync fails. The caller can stop and restart after resolving the error.

## MemoryCard format

Use a named MemoryCard and one bot process per card. Neo uses the logical key `puppet-neo.session` in Wechaty's Puppet namespace. Its value is:

```json
{
  "version": 1,
  "nonce": "<Base64 12-byte nonce>",
  "ciphertext": "<Base64 ciphertext followed by the 16-byte tag>"
}
```

Rust encrypts the record with AES-256-GCM, a fresh random nonce per save, and a versioned Neo context. The example supplies the same random 32-byte Base64 key on both runs through `PUPPET_NEO_SESSION_KEY`. Store that secret separately from the MemoryCard file. The file backend stores JSON.

| Encrypted field | Value |
| --- | --- |
| `protocolCrateVersion` | Exact crate version that wrote the snapshot |
| `account` | WeChat UIN as a decimal string, and authenticated username |
| `device` | Crate DeviceProfile with device ID, device type and client version |
| `credentials` | Cookie, session/request/response keys, AutoAuth key and key information as Base64 |
| `routing` | Accepted long and short connection hosts |
| `sync.cursor` | Last usable checkpoint as Base64, or null before the first checkpoint |

The snapshot deliberately excludes the official database password, raw auth sections and message contents. Restore validates the envelope and protocol state before renewing with WeChat. Neo verifies that the resulting account matches the snapshot, then saves the accepted result. An invalid snapshot or unsuccessful renewal reports an error and keeps the old file for investigation. To scan anew, use a new card name or deliberately move the old card aside.

Save after QR login or verified restore, and again on clean stop. The received network cursor may run ahead in memory. Neo retains unhandled commands in a bounded in-memory buffer and freezes the saved checkpoint behind them. A restart can replay from that earlier point. Hitting the retention limit stops sync with an error. A new run of the Ding Dong example uses a new challenge so replayed traffic cannot trigger its reply.

## Validation and release gates

| Gate | Evidence to collect |
| --- | --- |
| Rust storage and send | Seal/open round trip, wrong-key and tamper rejection, unknown sync command retention, lost acknowledgment handled once |
| Wechaty adapter | Real Wechaty with a scripted session drives login, direct text hydration, one reply attempt and named MemoryCard restore |
| Native boundary | Build the locked crate, load the addon in Node and reject invalid saved state without contacting WeChat |
| Package | Install an npm tarball in a fresh project and run the adapter and native smoke tests without a compiler |
| Live acceptance | First login and Ding Dong, then a fresh-process restore and second Ding Dong with an isolated test account |

Start with an npm development release on `next` after acceptance. Follow Wechaty's odd-minor development and even-minor stable convention. Publish only the targets that have passed their package tests and list live acceptance separately. Review the repository, installed example, storage behavior and release workflow with the Wechaty maintainers before expanding scope or promoting a stable release. Platform artifacts, automatic publication and any changes to the upstream crate follow the evidence from this milestone.
