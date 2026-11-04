# Wechaty Puppet Neo

A standalone WeChat protocol Puppet for Wechaty. The first milestone exercises QR login, direct text replies, and clean session restart through Wechaty's MemoryCard. See the [v1 specification](docs/v1-spec.md) for the storage format and acceptance gates.

[Watch the 22-second Ding Dong demo](docs/media/ding-dong-demo.mp4). The terminal recording shows a fresh QR login, a plain-text `ding`, and the matching `dong` in the recipient's chat.

Build the local Linux x64 addon with Node 24, Rust 1.93.1, a C compiler, Make and Perl.

```sh
npm ci
npm run build:native
npm test
```

For controlled live acceptance, use a private directory for the card and supply a Base64-encoded random 32-byte key through `PUPPET_NEO_SESSION_KEY`. Keep the key separately so both runs use the same value. Supply the exact username of the confirmed test peer and a fresh challenge.

```sh
node examples/ding-dong.mjs /private/test-state/neo EXACT_TEST_PEER run-one
```

Scan the terminal QR. Send `ding run-one` from that peer and check for `dong run-one` on the recipient device. Stop with Ctrl+C and run the same command with `run-two`, the same card path and the same key. An accepted restore will go directly to ready.

The current artifact is a local development build. The test suite separates simulated Wechaty behavior, native addon checks and real-account acceptance.
