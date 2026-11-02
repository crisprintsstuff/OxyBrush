# RFC 0002: Opt-in update checks

- **Status:** accepted by the maintainer (2026-11-02), implemented in the same change
- **Changes:** AGENTS.md hard rules "No telemetry or network calls" and "Pure Rust"

## Problem

Testers install OxyBrush from GitHub releases and have no way to learn that
a new alpha is out, short of watching the repository. Asking them to check
by hand means most run old builds and report bugs that are already fixed.

## Decision

An **opt-in** updater. It is **off by default**: a fresh install never
connects anywhere. Settings > Updates offers:

| Mode | Behavior |
|---|---|
| Off (default) | No network access. Help > Check for updates still works, because the user asked. |
| Tell me about new versions | Check about once a day; show a notice in the status bar. |
| Download new versions automatically | Also download the file in the background. |

**Installing always waits for the user.** On Windows (installer builds), a
click on *Install and restart* starts the installer and closes OxyBrush
(after the usual save prompt). For the portable zip, the macOS DMG and the
Linux tarball, OxyBrush shows the verified file in its folder with
instructions; it does not replace its own files. **Flatpak** installs
never use the updater: they say "run `flatpak update`".

### What is sent

- `GET https://api.github.com/repos/crisprintsstuff/OxyBrush/releases`
  and, when downloading, the release file and `SHA256SUMS.txt`.
- A `User-Agent: OxyBrush/<version>` header. No identifiers, no settings,
  no usage data, no crash data. GitHub sees the request like any browser
  visit (IP address, time).
- HTTPS only, including redirects.

### Which release

The newest non-draft release newer than the running version. Pre-release
users are offered pre-releases; stable users only stable releases. The file
matches the install (Windows installer vs portable is detected from the
installer's uninstaller next to the executable). The user can skip a
version.

### Verification

1. Every release has `SHA256SUMS.txt` (release workflow). A download is
   written to `<name>.part`, checked against it, and only then renamed;
   failures delete the partial file.
2. **Signatures (when set up):** the release workflow signs
   `SHA256SUMS.txt` with an ed25519 key from the repository secret
   `UPDATE_SIGNING_KEY`, publishing `SHA256SUMS.txt.sig`. When
   `oxy_update::SIGNING_PUBLIC_KEY` is set in the source, the app refuses
   any download whose checksum list is not validly signed. Until the
   maintainer creates a key, checksums are trusted because they come from
   GitHub over HTTPS (protects against corrupted downloads, not against a
   compromised GitHub account).

Maintainer setup for signatures:

```bash
cargo run -p oxy-update --bin oxy-update-sign -- keygen
```

Store the printed secret as the repository secret `UPDATE_SIGNING_KEY`,
put the public key in `crates/oxy-update/src/lib.rs` (`SIGNING_PUBLIC_KEY`),
and release. From then on every release is signed and every updater
requires it.

### Dependencies (exception to "pure Rust")

HTTPS needs TLS. `ureq` 3 with `rustls` uses the `ring` crypto library,
which compiles a small amount of C and assembly (MSVC, clang or gcc, which
every supported platform's Rust setup already has). There is no
production-ready pure-Rust TLS backend. All new crates are permissively
licensed (MIT/Apache-2.0, ISC, Unicode-3.0, CDLA-Permissive-2.0 for the
Mozilla root certificates).

The network code sits behind the `updater` feature of `oxy-app` (on by
default). `cargo build -p oxy-app --no-default-features` produces a build
with no network code and no TLS dependencies, for distributions that want
that.

## Alternatives considered

- **Open the releases page only:** no dependencies, but users must still
  remember to look; rejected by the maintainer in favour of opt-in checks.
- **Self-replacing binaries:** fragile on Windows (running executables are
  locked) and unsafe on macOS (unsigned apps); the installer and manual
  replacement are clearer.
- **On by default:** conflicts with the project's no-telemetry stance.

## Follow-ups

- Signing key set up by the maintainer (above).
- Notarized macOS builds would allow a smoother macOS install later.
