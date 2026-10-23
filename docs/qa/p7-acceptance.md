# P7.9 acceptance on a clean machine — attempt log

P7.9 asks for the exit test to run on a **clean Windows VM with the signed build**. This file records
each attempt, what it actually executed, and what is still unexecuted. Nothing here is ticked on the
strength of "it should work".

## 22 Oct 2026, second attempt: the install path executed for real

An elevated prompt was available this time, so `scripts/p7-vm-check.ps1` ran **without**
`-SkipInstall` against the built MSI and the release CLI:

| Step | Result |
|---|---|
| `msiexec /i … /qn /norestart` | exit 0 |
| installed payload | `C:\Program Files\Rigel\rigel-app.exe` (per-machine) |
| `Start-Process` on the installed exe | still alive after 20 s, so the window started |
| one native turn, `doctor`, shelf search | all pass with `HTTP(S)_PROXY` at a dead port |
| `msiexec /x … /qn /norestart` | exit 0; no `Program Files\Rigel`, no uninstall entry |

The full transcript is [records/p7-host-install.txt](records/p7-host-install.txt) with this checkout's
path redacted to `%REPO%`.

What this **does** prove: the MSI installs silently for all users, the payload is where the WiX
project says it is, the shipped UI and WebView2 host start together, the CLI works offline on a
machine with no repository and no network, and the uninstall leaves the machine as it found it.

What it **does not** prove, and this is the whole reason P7.9 names a VM:

1. That the install works where **nothing of Rigel ever existed** — here it replaced an earlier state
   and `%APPDATA%\Rigel` already existed.
2. The WebView2 **bootstrapper** actually fetching the runtime: WebView2 155.0.4283.62 was already
   installed, so the check passed trivially.
3. The **1809 floor**: this host is build 26300, so only the comparison is exercised.
4. **SmartScreen**, which judges a file that arrived with a browser's zone identifier.
5. That the build is **signed**: it is not. See below.

### Two rows that fail here by design

- `no Node.js on PATH` — this machine has Node; the acceptance machine must not.
- `the MSI carries an Authenticode signature` — the committed artifacts are unsigned, and the
  signing store is empty: `Get-ChildItem Cert:\CurrentUser\My,Cert:\LocalMachine\My -CodeSigningCert`
  returns nothing in either store. A throwaway self-signed cert from the first attempt was removed
  again after the exercise; it could never produce `ok trusted` because no CA issued it.

### `doctor` exits 1 on a cleanish machine, and that is correct

`doctor`'s contract is `0` all clear, `1` warnings, `2` failures. On a machine with no harnesses
installed and no tool cache it emits *warnings* (`harness:… not installed`, `tools` never probed) and
therefore exits 1. "Green" in P7.9 is asserted as **no `fail` row** plus an exit code that matches
the report's own summary, which is exactly what the script checks. Do not read exit 1 here as a
failing acceptance.

## The clean machine is now one command away: Windows Sandbox

Hyper-V was already enabled on this host and the session had an elevated prompt, so
`Containers-DisposableClientVM` was enabled on 22 Oct 2026. **It needs one restart before
`WindowsSandbox.exe` exists** — until then `scripts/p7-sandbox.ps1` prints the config it would use
and exits 2.

After that boot:

```powershell
cargo tauri build --manifest-path crates/rigel-app/Cargo.toml   # bundles
cargo build --release -p rigel-cli                             # target\release\rigel.exe
.\scripts\p7-sandbox.ps1                                       # launches the sandbox
```

`scripts/p7-sandbox.ps1` writes a throwaway `.wsb` with absolute paths (nothing machine-specific is
committed), maps this checkout **read-only** at `C:\rigel` and `docs\qa\records` **writable** at
`C:\out`, and runs `scripts/sandbox/p7-in-sandbox.ps1` at first logon. Inside, the same
`p7-vm-check.ps1` runs; the transcript lands in `docs/qa/records/p7-sandbox-<stamp>.txt`.

A sandbox gives a genuinely fresh Windows — no Rigel, no `%APPDATA%\Rigel`, no Node — which covers
items 1 and 4 above. It does **not** give a Windows 10 1809 image (it is the host's build), and it
does not produce a SmartScreen prompt. Items 2, 3 and 4 (signing) survive it.

## First attempt, 22 Oct 2026: no clean Windows VM existed

| Route | State at the time |
|---|---|
| Windows Sandbox | feature disabled; needed elevation **and** a reboot |
| Hyper-V / VirtualBox / VMware | Hyper-V present, no ISO, no VM; elevation and a reboot needed |
| Docker or Podman (Windows containers) | neither installed |
| WSL | a Linux userland on this same host — not a clean Windows |
| A VM on another machine | the intended route |

That attempt ran with `-SkipInstall` and proved only the non-installing half: the build floor
comparison, `doctor` with no failure row and a matching exit code, one native turn and a shelf search
behind a dead proxy. It also failed the Node and signature rows, as it should.

## Still unexecuted, and what would close each

| # | Item | Closes it |
|---|---|---|
| 1 | A **signed** MSI reporting `ok trusted` | a CA-issued or EV certificate thumbprint in `RIGEL_CERT_THUMBPRINT`, then `scripts/sign-installers.ps1` |
| 2 | First install on a machine with **no WebView2** | any fresh image without the runtime; watch `the WebView2 bootstrapper ran` |
| 3 | The **1809** floor | a Windows 10 1809 image (build 17763) |
| 4 | **SmartScreen** | download the signed MSI through a browser on a fresh machine; a person reads the prompt |
| 5 | **The look of the window** | a person looks at it in the sandbox or the VM |

Until 1–3 are recorded here, P7.9 stays open with the reason, as the handoff requires.

## The command a VM needs

From an elevated PowerShell in the repository, after the bundles are built (see
[docs/PLATFORMS.md](../PLATFORMS.md)):

```powershell
.\scripts\sign-installers.ps1 -Thumbprint $env:RIGEL_CERT_THUMBPRINT
.\scripts\p7-vm-check.ps1 `
  -Msi .\target\release\bundle\msi\Rigel_0.1.0_x64_en-US.msi `
  -Rigel C:\tools\rigel.exe
```

On a Windows 10 1809 image with no WebView2 preinstalled and no Node, every line should be `ok`
except the two `manual` ones. Paste that transcript here with the date and the VM image name; that
closes P7.9.

## 22 Oct 2026, 22:30: the restart had not happened

The next step was "launch the acceptance in Windows Sandbox". It could not start, and the sandbox is
not the reason: **Windows itself had not restarted since the feature was enabled.**

| Evidence | Value at the attempt |
|---|---|
| `Win32_OperatingSystem.LastBootUpTime` | Sat 17 Oct 2026 21:26:53 |
| Uptime | 5 d 1 h |
| Last `Microsoft-Windows-Kernel-Boot` event 27 | 19 Oct 2026 22:12:40, **boot type `0x1` (hiberboot)** |
| CBS `RebootPending` key | present |
| `HiberbootEnabled` | `1` |
| `Containers-DisposableClientVM` | `Enabled`, `RestartNeeded: False` |
| `C:\Windows\System32\WindowsSandbox.exe` | absent |
| Appx, provisioned packages, `winget search "Windows Sandbox"` | nothing |

With Fast Startup on, "shut down and start again" resumes a hibernated session and applies no
servicing: the component store holds the enable flag, but the payload is never staged, so the
feature reads `Enabled` while its binary does not exist. `Restart` (or a firmware reboot) is what
applies it. The no-reboot routes were checked and closed — on this build the sandbox is not a Store
package and there is nothing to install.

### What was made ready instead

- `scripts/p7-sandbox.ps1 -DryRun` now prints and writes the `.wsb` **before** the binary check, so
  the config can be inspected on a machine that is still waiting for its boot. Verified: the file
  parses, two mounts (`C:\rigel` read-only, `C:\out` writable) and the logon command line intact.
- `scripts/p7-queue-sandbox.ps1` registers a one-shot logon task, `Rigel P7.9 sandbox acceptance`,
  for the current user: 45 s after the next sign-in it runs the launcher, writes
  `docs/qa/records/p7-sandbox-launch.log`, and deletes itself. Exercised now — exit 2 and a log
  saying the sandbox is absent. After a real boot the same task should exit 0, open the sandbox, and
  the guest writes `p7-sandbox-<stamp>.txt` into `docs/qa/records/`.

```powershell
.\scripts\p7-queue-sandbox.ps1            # arm it (already armed on this host)
.\scripts\p7-queue-sandbox.ps1 -Show      # inspect without changing anything
.\scripts\p7-queue-sandbox.ps1 -Remove    # cancel
```

### Reading the result

`p7-sandbox-launch.log` is the host half and UTF-16 (PowerShell `*>` redirection); the guest
transcript it produces is the record that matters. Two exit codes cover the host half:

- **0** — the sandbox was launched. Its window runs the acceptance; wait for `p7-sandbox-<stamp>.txt`.
- **2** — `WindowsSandbox.exe` still is not there, i.e. the machine booted without applying the
  feature. Re-arm with `scripts/p7-queue-sandbox.ps1` and reboot with `Restart`, not shut down.

### What the sandbox still cannot prove

It is the host's build with WebView2 likely present, so the Windows 10 1809 floor, a genuinely
WebView2-less first install, SmartScreen and the look of the window on a fresh display still need
the 1809 image and a person, exactly as the table above says. A green sandbox run narrows P7.9; it
does not close it.
