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
3. Check that the terminal prints `Logged in as` with the account's nickname, the incoming message and the reply, and that the peer receives exactly one `dong`.
4. Sign the Puppet Neo device out from the phone. Check that the terminal prints `Logged out: WeChat ended the session`.
5. Stop with Ctrl+C and run the command again. Verify that a fresh QR scan is required and repeat the exchange.
6. Install an npm tarball in a fresh project, copy its TypeScript example into that project, and repeat the test using the installed package.

The example listens for Puppet events, retrieves messages with `messagePayload()`, and replies with `messageSendText()`. It handles direct text from other accounts. Initial sync can deliver earlier messages, so inspect the test conversation before the exchange.

## Architecture

```text
ding-dong-bot.ts
  -> PuppetNeo (TypeScript, Puppet events and payload lookup)
  -> NeoSession (.node, Rust account credentials and protocol operations)
  -> wechat-protocol = 2.1.0 (published Cargo source)
  -> WeChat servers
```

Neo builds the addon from pinned crate sources. Cargo.lock and package-lock.json pin build inputs. Native CI builds and tests Linux x64 and ARM64 with glibc, macOS ARM64 on `macos-latest`, and Windows x64 on `windows-latest`. The combined npm tarball contains all four native builds and selects one using Node's platform and architecture. The CI-built Linux addons require glibc 2.38 or newer.

Blocking native calls run off the Node event loop, and a Rust lock serializes session access. TypeScript schedules login polling, sync and heartbeat, then maps incoming text to Puppet payloads and events. After authentication, Neo opens the account's incoming connection, drains initial sync, and requests NewSync when that connection or a heartbeat signals message work.

For protocol contributors, follow `messageSendText()` in [the Puppet](../src/puppet-neo.ts) into the [native session](../native/src/lib.rs), then see `AccountRequest::send_text` and `wire::text_send_request` in the [published Rust source](https://docs.rs/crate/wechat-protocol/2.1.0/source/src/). After login, the session moves the accepted credentials into one `NativeClient` account owner; the push connection and sends share it. Incoming text follows the reverse direction through NewSync and AddMsg decoding.

## State and errors

Rust holds the QR challenge, device identity, authentication credentials and sync cursor in memory. TypeScript keeps the most recent 2,048 message and contact payloads for Puppet lookups and duplicate suppression. A fresh start creates a new session and QR challenge. Stopping ends session work and clears the payload caches.

Session state lasts for the running process. Neo neither exports nor restores authentication snapshots. Existing MemoryCard files from earlier development builds are ignored by Neo's authentication code. Wechaty Puppet's inherited MemoryCard abstraction remains part of its base API.

Unhandled sync commands are silently discarded while the received cursor advances. Neo emits direct text messages with a known server message ID and keeps IDs as strings. It caches each payload before emitting its ID so callers can retrieve it.

An accepted send resolves with no Message object. An uncertain send is reported to the caller after a single submission.

## Login and logout

When the phone confirms the QR login, Neo emits `login` with the account's username at once. It also starts a background lookup of the account's own profile. `contactPayload(currentUserId)` waits for that lookup and returns the nickname as `name` and the WeChat ID, when set, as `weixin`. If the lookup fails, `name` falls back to the WeChat ID, then to the username. Login never waits for or fails because of the profile.

Each session emits `logout` once:

| Cause | `logout` reason |
| --- | --- |
| The application calls `stop()` | `puppet stop()` |
| The application calls `logout(reason)`. Neo first submits WeChat's logout request once | the given reason |
| WeChat rejects heartbeat or sync credentials (`-13`) and also rejects one automatic renewal, for example after the device is signed out from the phone | `WeChat ended the session` |
| Any other failure after login stops the session loop. An `error` event comes first | `Neo session failed` |

If the notification socket closes, Neo reconnects once and lets a heartbeat decide whether the session is still valid. Sends are never repeated after a renewal. After a logout the puppet stays started; stop and start it to log in with a new QR. The base Puppet clears `currentUserId` one event-loop turn after emitting `logout`, so listeners can still read it.

## Validation and release gates

| Gate | Evidence to collect |
| --- | --- |
| Rust behavior | Unhandled sync commands skipped and lost send acknowledgment handled once |
| Puppet API | Scripted session drives QR login, text payload lookup, reply, clean stop and a fresh login |
| Login and logout | Login before the profile, nickname and fallback names, and one logout for stop, `logout()`, a session ended by WeChat and an unexpected failure |
| Example | The actual TypeScript example runs against a scripted session and replies through PuppetNeo |
| Native boundary | Load the addon without a storage key and reject unauthenticated operations |
| Package | Install a tarball in a fresh project and run the Puppet, native and example tests |
| Live acceptance | Fresh QR login showing the nickname, Ding Dong with an isolated test account, `logout` after signing out from the phone, then repeat after restarting |

Start with an npm development release on `next` after acceptance. Follow Wechaty's odd-minor development and even-minor stable convention. Use the combined tarball after its four native package checks pass and report live acceptance separately. Review the repository, installed example and release workflow with the Wechaty maintainers before expanding scope or promoting a stable release. Automatic publication follows that review.
