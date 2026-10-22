# Platforms

## Windows 10 1809 and newer — supported

That is the floor, not a preference: the Evergreen WebView2 bootstrapper and the WebView2 runtime
itself both require 1809 (`10.0.17763`). The installer uses Tauri's `downloadBootstrapper` mode,
which fetches the runtime only when the machine does not already have it, then runs it silently.

What a Windows build includes:

- an MSI (WiX) and an NSIS installer, both per-user by default (`currentUser` for NSIS; the MSI
  uses the default install scope);
- the WebView2 bootstrapper, with a silent install;
- no Node.js, no Python, no Rust, and no package manager requirement for the app to start. The
  shelf is a local SQLite file, and the agent runs in-process. A *skill* may need a tool; that is
  what `rigel tools` is for, and the install is shown before it runs.

### Building an installer

```powershell
cargo install tauri-cli --version "^2" --locked
cargo tauri build
```

The output lands in `target/release/bundle/msi` and `target/release/bundle/nsis`. The bundle config
lives in [crates/rigel-app/tauri.conf.json](../crates/rigel-app/tauri.conf.json): `msi` and `nsis`
targets, `webviewInstallMode: downloadBootstrapper` (silent), downgrades refused.

### Signing

Signing is configured on the build machine, never committed.

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = "…"          # release-manifest signing key (rigel update)
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "…"
# Authenticode, through a certificate already in the user's store:
$env:RIGEL_CERT_THUMBPRINT = "…"               # see the signCommand below
cargo tauri build
```

Tauri's `bundle.windows.certificateThumbprint`, `digestAlgorithm` and `timestampUrl` fields are the
supported way to sign; they are deliberately absent from the committed config so a clone cannot
sign with someone else's identity. A build without a certificate produces an **unsigned**
installer, and the release notes say so. `rigel update verify` checks the artifact hash and the
manifest signature; Windows SmartScreen checks the Authenticode signature separately.

The update public key is `update-pubkey.txt` in the data directory, or `RIGEL_UPDATE_PUBKEY`.
`rigel update status` prints which one is in use, never the key itself.

## macOS — buildable, unsupported

`cargo build --workspace` and `cargo test --workspace` work; the keychain uses
`apple-native`, and the CLI is complete. What is missing is everything that makes it a release:

- no `.app` bundle is produced or notarised, and `bundle.icon` only lists the `.ico`;
- the installer and the update manifest assume Windows;
- no Apple Silicon or Intel build has been run against the harnesses.

If you build it, expect to fix the icon set and add a `macOS` bundle section first. Treat it as a
developer build, not a product.

## Linux — buildable, unsupported

The native agent, CLI, shelf and tests build and run on Linux (CI compiles the workspace there).
The keychain uses `sync-secret-service`, so a running Secret Service (GNOME Keyring, KWallet) is
required for `rigel keys`. Limits:

- no installer, no AppImage, no `.deb`;
- `apt-get` installs are **shown and refused**, because Rigel never runs an elevation tool —
  `rigel tools install ripgrep` prints `apt-get install -y ripgrep` and stops;
- WSL is a first-class *host* for detection (`wsl:<distro>`), which is not the same as running
  Rigel inside WSL.

## Anything else

Not tested. The code is `cfg`-guarded rather than assumed: `SystemEnvironment` searches `PATHEXT`
on Windows and plain names elsewhere, the shell is `cmd /C` or `/bin/sh -c`, and the data directory
follows `APPDATA`, then `XDG_DATA_HOME`, then `HOME`. A new platform should start by running
`cargo test --workspace` and `bash scripts/p7-exit-test.sh`, and by reading `SECURITY.md` before
changing anything in the path or approval code.
