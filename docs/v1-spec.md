# Puppet Neo v1 specification

Puppet Neo is a local WeChat protocol Puppet for Wechaty. Its long-term goal is comprehensive protocol coverage through open source. The first milestone covers QR login and direct text Ding Dong through the Puppet API.

## First acceptance gate

Use an isolated Linux x64 test account and a confirmed separate peer. Build the addon and package, then run the repository example.

```sh
npm ci
npm run build:native
npm run build
node examples/ding-dong-bot.ts
```

1. Scan the QR displayed in the terminal and confirm the login on the phone.
2. Wait for `Ready`, then send one new `ding` from the peer.
3. Check that the terminal prints the incoming message and reply, and that the peer receives exactly one `dong`.
4. Stop with Ctrl+C and run the command again. Verify that a fresh QR scan is required and repeat the exchange.
5. Install an npm tarball in a fresh project, copy its TypeScript example into that project, and repeat the test using the installed package.

The example listens for Puppet events, retrieves messages with `messagePayload()`, and replies with `messageSendText()`. It handles direct text from other accounts. Initial sync can deliver earlier messages, so inspect the test conversation before the exchange.

## Architecture

```text
ding-dong-bot.ts
  -> PuppetNeo (TypeScript, Puppet events and payload lookup)
  -> NeoSession (.node, Rust account credentials and protocol operations)
  -> wechat-protocol = 1.5.1 (published Cargo source)
  -> WeChat servers
```

Neo builds the addon from pinned crate sources. Cargo.lock and package-lock.json pin build inputs. Native CI builds and tests x64 and ARM64 addons on Linux with glibc and macOS, plus Windows x64. The combined npm tarball contains all five builds and selects one using Node's platform and architecture. The CI-built Linux addons require glibc 2.38 or newer.

Blocking native calls run off the Node event loop, and a Rust lock serializes session access. TypeScript schedules login polling, sync and heartbeat, then maps incoming text to Puppet payloads and events. After authentication, Neo opens the account's incoming connection, drains initial sync, and requests NewSync when that connection or a heartbeat signals message work.

For protocol contributors, follow `messageSendText()` in [the Puppet](../src/puppet-neo.ts) into the [native session](../native/src/lib.rs), then see `send_text` and `text_send_request` in the [published Rust source](https://docs.rs/crate/wechat-protocol/1.5.1/source/src/). Incoming text follows the reverse direction through NewSync and AddMsg decoding.

## State and errors

Rust holds the QR challenge, device identity, authentication credentials and sync cursor in memory. TypeScript keeps the most recent 2,048 message and contact payloads for Puppet lookups and duplicate suppression. A fresh start creates a new session and QR challenge. Stopping ends session work and clears the payload caches.

Session state lasts for the running process. Neo neither exports nor restores authentication snapshots. Existing MemoryCard files from earlier development builds are ignored by Neo's authentication code. Wechaty Puppet's inherited MemoryCard abstraction remains part of its base API.

Unhandled sync commands are silently discarded while the received cursor advances. Neo emits direct text messages with a known server message ID and keeps IDs as strings. It caches each payload before emitting its ID so callers can retrieve it.

An accepted send resolves with no Message object. An uncertain send is reported to the caller after a single submission. Polling, heartbeat or sync failures stop the session loop and emit an error. The caller can stop and start again to log in afresh.

## Validation and release gates

| Gate | Evidence to collect |
| --- | --- |
| Rust behavior | Unhandled sync commands skipped and lost send acknowledgment handled once |
| Puppet API | Scripted session drives QR login, text payload lookup, reply, clean stop and a fresh login |
| Example | The actual TypeScript example runs against a scripted session and replies through PuppetNeo |
| Native boundary | Load the addon without a storage key and reject unauthenticated operations |
| Package | Install a tarball in a fresh project and run the Puppet, native and example tests |
| Live acceptance | Fresh QR login and Ding Dong with an isolated test account, then repeat after restarting |

Start with an npm development release on `next` after acceptance. Follow Wechaty's odd-minor development and even-minor stable convention. Use the combined tarball after its five native package checks pass and report live acceptance separately. Review the repository, installed example and release workflow with the Wechaty maintainers before expanding scope or promoting a stable release. Automatic publication follows that review.
