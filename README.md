# Wechaty Puppet Neo

A standalone WeChat protocol Puppet for Wechaty. The TypeScript example uses PuppetNeo directly to log in with a QR code and reply to direct text messages. Authentication stays in memory, so each run needs a fresh QR scan. See the [v1 specification](docs/v1-spec.md) for the architecture and acceptance gates.

The 30-second live terminal demo shows QR login and the standalone TypeScript example exchanging `ding` and `dong` with a second account. It was recorded on Linux x64 with protocol crate 1.4.0. Idle waits are cut.

https://github.com/user-attachments/assets/e7e40c19-ad49-4061-be53-cef1e1c4c978

Neo builds native addons for Linux x64 and ARM64, macOS on Apple Silicon, and Windows x64. CI tests an npm install on each target and assembles one tarball with all four addons. The prebuilt Linux addons require glibc 2.38 or newer. Neo loads the addon for the architecture of the running Node process. Use ARM64 Node.js on macOS.

On Windows 11 on ARM, install **x64 Node.js** to use the x64 addon under Windows emulation. Run `node -p process.arch` to check that it prints `x64`. [Microsoft's emulation documentation](https://learn.microsoft.com/en-us/windows/arm/apps-on-arm-x86-emulation)

To build from source, use Node.js 26 and Rust 1.93.1 with the matching Rust target.

| Platform | Build prerequisites |
| --- | --- |
| Linux | C compiler, Make, Perl |
| macOS on Apple Silicon | Xcode Command Line Tools, Make, Perl |
| Windows x64 | MSVC x64 C++ tools, Windows SDK, Perl |

```sh
npm ci
npm run build:native
npm run build
node examples/ding-dong-bot.ts
```

Scan the terminal QR and confirm the login on your phone. After `Ready` appears, send `ding` from another WeChat account. The example replies with `dong` and prints the received message and reply. Stop with Ctrl+C. Starting it again displays a new QR.

Node runs the TypeScript example directly. The commands above build the package and native addon from source. To use a prebuilt tarball in another project, install the tarball and copy its `examples/ding-dong-bot.ts` into that project before running it.

Run `npm test` for type checking and the offline tests, and `npm run test:package` to check a clean tarball installation.

### Development container

A dev container is a reproducible development workspace described in the repository. VS Code's Dev Containers extension can open the project inside that environment so contributors use the same OS and toolchain without installing Node.js or Rust on the host.

The configuration in [`.devcontainer/devcontainer.json`](.devcontainer/devcontainer.json) is based on the [Ship.Fail devcontainer template](https://github.com/ShipFail/devcontainer). It uses Ubuntu 24.04, Node.js 26, Rust 1.93.1 with `rustfmt` and `clippy`, and the C compiler, Make, and Perl needed for the native addon. It runs `npm ci` after container creation. It also retains the template's supporting CLI tools and mounts host Claude, Codex, Gemini, and Git configuration into the container; the GitHub CLI helper can use the host Git credential helper. Only use these host mounts when you trust the repository and its development tooling.

Windows contributors must use WSL to open the repository and run the dev container; native Windows host initialization is unsupported. Install the VS Code Dev Containers extension, then run **Dev Containers: Reopen in Container** from the Command Palette.

For live acceptance, use an isolated test account and a confirmed peer. The example replies to direct `ding` messages, including messages received during initial sync. Verify `dong` on the peer's device. Real-account testing has covered Linux x64. CI on the other targets covers addon loading, npm installation, and scripted Puppet behavior.
