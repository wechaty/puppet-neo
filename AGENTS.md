# AGENTS.md — Puppet Neo

## Mission
- Build a reliable, open-source, cross-platform WeChat Puppet for the Wechaty ecosystem.
- Run locally without the official WeChat client or external daemons.
- Prioritize simplicity, maintainability, and developer experience.
- Follow the [Wechaty Puppet API](https://github.com/wechaty/puppet) and community conventions.

## Architecture
- Keep the Puppet adapter small, readable, and TypeScript-first.
- Use the existing Rust protocol implementation for v1.
- Prefer pure ECMAScript implementations in the long term when practical.
- Pin native dependencies to exact, reproducible source versions.
- Keep protocol details isolated behind the Puppet abstraction.
- Pass configuration through Puppet constructor `options`.

## Development
- Support the latest stable Node.js LTS release.
- Write TypeScript (`.ts`) for all new source files and examples.
- Run TypeScript directly with Node.js native type stripping.
- Avoid unnecessary transpilers, runtimes, and build steps.
- Keep examples minimal and easy to understand.
- Follow [Puppet Mock](https://github.com/wechaty/puppet-mock) conventions.
- Never depend on the higher-level Wechaty framework in Puppet examples or unit tests.
- Use raw Puppet events, payloads, and APIs for testing.

## First Milestone
- Implement QR login and text-message Ding Dong only.
- Display the logged-in user's name or available account identifier after login.
- Receive `ding` and reply `dong` through the Puppet API.
- Provide a directly runnable `examples/ding-dong-bot.ts`.
- Keep authentication and session handling simple and reliable.
- Defer additional messaging features until the foundation is approved.

## Quality and Releases
- Run automated tests and type checking in CI.
- Test clean installation, startup, login, shutdown, and restart.
- Build and test native packages for supported platforms.
- Publish prebuilt binaries without requiring users to install Rust.
- Keep builds reproducible and release artifacts verifiable.
- Follow [Wechaty versioning conventions](https://github.com/wechaty/wechaty/issues/905).
- Automate npm publishing after required checks pass.
- Document architecture, installation, and troubleshooting clearly.

## Collaboration
- Review architecture, dependencies, CI/CD, and packaging before the first stable release.
- Require mutual agreement between the project maintainer and Wechaty lead before expanding scope.
- Keep changes focused, incremental, and easy to review.
- Preserve compatibility with the Wechaty Puppet abstraction.
- Defer VoIP, video calls, and advanced protocol features until text messaging is stable.
- Favor long-term community maintainability over short-term feature count.