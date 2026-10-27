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
belong to the release operator. Two signatures travel with a release, and they use different keys:

| signature | key | checked by |
| --- | --- | --- |
| the MSI and NSIS wrappers **and every PE file inside them** | Authenticode code-signing certificate (`RIGEL_CERT_THUMBPRINT`) | Windows, SmartScreen |
| the update manifest (`version\nurl\nsha256`, plus the optional MSI URL/hash) | Ed25519 publisher key (`update-pubkey.txt` or `RIGEL_UPDATE_PUBKEY`) | `rigel update verify` |

They never substitute for each other. Tauri's updater key (`TAURI_SIGNING_PRIVATE_KEY`) is not
used here at all: Rigel's updater is its own manifest format, not `tauri-plugin-updater`'s.

An MSI is a container. Signing the MSI alone signs the wrapper; the copy of `rigel-app.exe` inside
stays unsigned, so the file installed in Program Files fails verification. Signing the executable
*before* `cargo tauri build` does not fix it either: Tauri patches the app with bundle-type
information while it packages (once for MSI, once for NSIS), and that patch makes the earlier
signature stale. The only order that works is the one Tauri's own signing hook provides — patch,
then sign, then package.

`sign-installers.ps1` writes a temporary config carrying the operator's thumbprint and passes it to
`cargo tauri build`, so Tauri signs the patched executable for each package type, the WiX helper
DLLs embedded in the MSI, and then the MSI and NSIS wrappers. The script then unpacks the MSI
(`msiexec /a`) and checks that every PE file it installs is validly signed by the same certificate
as the wrapper. The helper DLLs are not installed; the MSI's own signature covers those.

```powershell
$env:RIGEL_CERT_THUMBPRINT = "…"        # thumbprint of a code-signing certificate in this store
.\scripts\sign-installers.ps1 -Build    # build → Tauri signs payload and wrappers → verify both

# an already-built bundle: sign its wrappers, then verify both layers (a bundle packaged without
# signing fails the nested check on purpose; rerun with -Build to fix it)
.\scripts\sign-installers.ps1

# or, for a PFX you import first, and without a timestamp when offline
.\scripts\sign-installers.ps1 -Build -Thumbprint A1B2… -NoTimestamp

# no certificate at hand? still prove the build produced installers
.\scripts\sign-installers.ps1 -VerifyOnly
```

`sign-installers.ps1` finds `signtool.exe` in the installed Windows SDK, signs with SHA-256 and a
RFC 3161 timestamp, and then verifies each artifact and the MSI payload. A signature whose bytes no
longer match is a failure, not a note: the script exits non-zero when a file has no signature, when
its signature is invalid, or when a PE the MSI installs is unsigned, invalidly signed or signed by a
different certificate. It reports `ok trusted` for a CA-issued certificate and `note signed but
chain untrusted` for a self-signed test one — a release must show `ok trusted` for every file.

Tauri's `bundle.windows.certificateThumbprint` is deliberately absent from the committed config: a
thumbprint is machine-specific, and a clone must not sign with someone else's identity. The release
script passes it as a temporary `--config` instead. A build with no certificate produces an
**unsigned** installer, and the release notes say so. `rigel update verify` checks the artifact hash
and the Ed25519 manifest signature; Windows SmartScreen checks the Authenticode signature
separately.

The certificate is a Windows code-signing certificate with its private key in
`Cert:\CurrentUser\My` (or `Cert:\LocalMachine\My`). A self-signed certificate proves the pipeline
and produces valid inner and outer signatures, but SmartScreen still warns because the chain is not
trusted. An organization-issued or EV certificate is required for `ok trusted` and SmartScreen
reputation.

To make a self-signed certificate for local builds (it stays in this user's store; delete it when
it is no longer wanted):

```powershell
$cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=Rigel contributors" `
  -CertStoreLocation Cert:\CurrentUser\My -KeyUsage DigitalSignature -KeyExportPolicy Exportable
$env:RIGEL_CERT_THUMBPRINT = $cert.Thumbprint
.\scripts\sign-installers.ps1 -Build
```

### Publishing an update

The app checks the newest GitHub release's `latest.json` at launch; Settings → Updates can turn
that check and auto-install off. A tag starting with `v` runs `.github/workflows/release.yml`,
which first waits for the `ci` workflow to finish on the tagged commit: the tag is pushed with the
branch, so that run is normally still going. A failed run stops the release before anything is
built or uploaded, and a commit with no successful run in 45 minutes stops it too — the workflow
never publishes a tag the checks have not cleared. It then stamps that version into the build,
builds the sidecar and both installers, signs the manifest, verifies that signature against the
committed public key, and uploads the MSI, NSIS setup and `latest.json` to the release. The
workflow needs one repository secret:

| secret | value |
| --- | --- |
| `RIGEL_UPDATE_SIGNING_KEY` | the publisher's 32-byte Ed25519 seed, 64 hex characters |

The matching public key is committed at `crates/rigel-app/resources/update-pubkey.txt`; the app
copies it into the data directory at launch, and `RIGEL_UPDATE_PUBKEY` overrides it for a
development build. Do not put the private seed in a tracked file. `scripts/sign-update-manifest.mjs`
signs exactly the bytes `rigel-core::update::UpdateManifest::signed_bytes` verifies.

```bash
# bump every manifest, write the changelog, commit, tag and push
node scripts/release.mjs patch
```

The script refuses a dirty tree, a branch other than `main`, a tag that already exists, and a
release with no commits since the previous tag. `--dry-run` prints the version and the changelog
bullets without changing anything; `--no-push` stops after the local commit and tag. A hand-written
`git tag v1.2.3 && git push origin v1.2.3` still publishes a build, but skips the version bump and
the changelog entry, so the tree no longer matches the tag.

The release workflow publishes the tag. The app's next launch offers the update (or installs it if
the second switch is on); the NSIS installer restarts the new build, and a per-machine MSI update
shows its own progress window. The installer is unsigned until the Authenticode certificate
exists; the manifest signature and SHA-256 still catch a changed artifact, but SmartScreen may
warn about the unsigned installer.

### The Store MSIX route (a side quest)

An MSIX submitted to the Microsoft Store is re-signed by Microsoft: no certificate purchase and no
SmartScreen prompt. Tauri has no first-class MSIX bundler, so this means wrapping the build with
MakeAppx or the MSIX Packaging Tool. It does **not** replace signing the MSI: the Store does not
re-sign MSI/EXE submissions, and a sideloaded MSI still needs a trusted certificate. Treat it as an
alternative distribution path, not as a way to ship an unsigned installer.

### Exit test on a clean machine (P7.9)

`scripts/p7-exit-test.sh` covers what a developer machine can prove. The rest runs on a clean
Windows VM, from an elevated prompt:

```powershell
.\scripts\p7-vm-check.ps1 `
  -Msi .	argeteleaseundle\msi\Rigel_0.1.0_x64_en-US.msi `
  -Rigel C:	oolsigel.exe
```

It checks the Windows build (17763+), that Node is absent, the MSI's signature, a silent install,
that the installed executable carries the installer's certificate (the MSI's signature covers only
the wrapper), that the WebView2 bootstrapper ran, that the window starts, `doctor`'s rows and exit
code, one native turn offline, the offline shelf, and then uninstalls. Two lines stay `manual`: SmartScreen
(only a browser-downloaded file is judged) and the look of the window on a fresh display.

If no VM is at hand, Windows Sandbox is a real clean machine for the install itself: enable it once
(`Enable-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM -All`, then
restart) and `scripts/p7-sandbox.ps1` writes a throwaway `.wsb` that maps this checkout read-only and
runs the same script at first logon. It gives a fresh Windows with no Rigel and no Node, which is
what the install checks need; it is the host's build, so the 1809 floor and SmartScreen still need
an image, and a signed build needs a certificate. `docs/qa/p7-acceptance.md` records which attempt
proved what.

The update public key is `update-pubkey.txt` in the data directory, or `RIGEL_UPDATE_PUBKEY`.
A release build bundles the public half in `crates/rigel-app/resources/update-pubkey.txt` and copies
it into the data directory at launch. `rigel update status` prints which one is in use, never the
key itself.

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
