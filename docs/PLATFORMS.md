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
cd crates/rigel-app
cargo tauri build
```

Run it from `crates/rigel-app`: Tauri executes `beforeBuildCommand` one level above the app
directory (`crates/`), which is why the committed hook says `npm --prefix ../ui run build`. From
the repository root the hook resolves the wrong directory and the build fails loudly rather than
shipping a stale `ui/dist`.

The output lands in `target/release/bundle/msi` and `target/release/bundle/nsis`. The bundle config
lives in [crates/rigel-app/tauri.conf.json](../crates/rigel-app/tauri.conf.json): `msi` and `nsis`
targets, `webviewInstallMode: downloadBootstrapper` (silent), downgrades refused.

### Signing

Signing happens on the build machine, never in this repository: the certificate and the update key
belong to the release operator.

```powershell
cargo tauri build

# Authenticode over MSI and NSIS output, then verification of every file
$env:RIGEL_CERT_THUMBPRINT = "…"        # thumbprint of a code-signing certificate in this store
.\scripts\sign-installers.ps1

# or, for a PFX you import first, and without a timestamp when offline
.\scripts\sign-installers.ps1 -Thumbprint A1B2… -NoTimestamp

# no certificate at hand? still prove the build produced installers
.\scripts\sign-installers.ps1 -VerifyOnly
```

`sign-installers.ps1` finds `signtool.exe` in the installed Windows SDK, signs with SHA-256 and a
timestamp, and then verifies each artifact. It reports `ok trusted` for a CA-issued certificate and
`note signed but chain untrusted` for a self-signed test one — a release must show `ok trusted` for
every file, and the script exits non-zero when a file has no signature at all.

Tauri's own `bundle.windows.certificateThumbprint` and `signCommand` do the same thing inside
`cargo tauri build`; they are deliberately absent from the committed config because a thumbprint is
machine-specific and a clone must not sign with someone else's identity. A build with no
certificate produces an **unsigned** installer, and the release notes say so. `rigel update verify`
checks the artifact hash and the manifest signature; Windows SmartScreen checks the Authenticode
signature separately.

The certificate is a Windows code-signing certificate with its private key in
`Cert:\CurrentUser\My` (or `Cert:\LocalMachine\My`). An organization-issued or EV certificate is
required for SmartScreen reputation; a self-signed one only proves the pipeline, not the trust.

### Exit test on a clean machine (P7.9)

`scripts/p7-exit-test.sh` covers what a developer machine can prove. The rest runs on a clean
Windows VM, from an elevated prompt:

```powershell
.\scripts\p7-vm-check.ps1 `
  -Msi .	argeteleaseundle\msi\Rigel_0.1.0_x64_en-US.msi `
  -Rigel C:	oolsigel.exe
```

It checks the Windows build (17763+), that Node is absent, the MSI's signature, a silent install,
that the WebView2 bootstrapper ran, that the window starts, `doctor`'s rows and exit code, one
native turn offline, the offline shelf, and then uninstalls. Two lines stay `manual`: SmartScreen
(only a browser-downloaded file is judged) and the look of the window on a fresh display.

If no VM is at hand, Windows Sandbox is a real clean machine for the install itself: enable it once
(`Enable-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM -All`, then
restart) and `scripts/p7-sandbox.ps1` writes a throwaway `.wsb` that maps this checkout read-only and
runs the same script at first logon. It gives a fresh Windows with no Rigel and no Node, which is
what the install checks need; it is the host's build, so the 1809 floor and SmartScreen still need
an image, and a signed build needs a certificate. `docs/qa/p7-acceptance.md` records which attempt
proved what.

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
