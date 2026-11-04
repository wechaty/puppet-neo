# Wechaty Puppet Neo

A standalone WeChat protocol Puppet for Wechaty. The first milestone exercises QR login, direct text replies, and clean session restart through Wechaty's MemoryCard. See the [v1 specification](docs/v1-spec.md) for the storage format and acceptance gates.

The 22-second Ding Dong demo shows a fresh QR login, a plain-text `ding`, and the matching `dong` in the recipient's chat.

https://github.com/user-attachments/assets/2087ba0d-6613-492c-8629-481303bad700

Neo's native build targets Linux with glibc, macOS, and Windows, each on x64 and ARM64. CI tests an npm install on each target and assembles one tarball with all six addons. Neo loads the addon for the architecture of the running Node process.

To build from source, use Node 24 and Rust 1.93.1 with the matching Rust target.

| Platform | Build prerequisites |
| --- | --- |
| Linux | C compiler, Make, Perl |
| macOS | Xcode Command Line Tools, Make, Perl |
| Windows | MSVC C++ tools for the target architecture, Windows SDK, Perl. Windows ARM64 also needs Python to build Wechaty 1.20.2's LevelDOWN dependency. |

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

The demo records a real-account test on Linux x64. CI on the other targets covers addon loading, npm installation, and simulated Wechaty behavior.
