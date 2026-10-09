# Code signing, update signatures and winget

Why Windows warns when people install Syscura, and the free ways to stop that.

| Warning | Cause | Free fix |
|---|---|---|
| **"Windows protected your PC"** (SmartScreen) | The file came from the internet, is not signed, and has no download reputation yet | Signed releases (SignPath) build reputation. Installs through **winget** don't show this warning |
| **"Unknown publisher"** on the admin prompt | The file is not Authenticode-signed | Signing through SignPath Foundation |

Releases are built by [`.github/workflows/release.yml`](../.github/workflows/release.yml) on GitHub's machines, never on a developer PC. Signing works the same way.

---

## 1. SignPath Foundation: free code signing for open source

[SignPath Foundation](https://signpath.org) signs open-source releases for free. The certificate is issued to "SignPath Foundation", and that name appears as the publisher. Signing only happens inside GitHub Actions, from this repository's source code, so nobody can sign something that wasn't built here. Approval usually takes a few weeks.

### Apply

Apply at <https://signpath.org/apply>. What they check, and where Syscura already meets it:

| Requirement | Syscura |
|---|---|
| OSI-approved license, no proprietary parts | MIT ([LICENSE](../LICENSE)) |
| Actively maintained, already released | Releases since 0.1.0-beta.1; 1.0.0 is out |
| Documented functionality | [README](../README.md) |
| Built from source in CI | `release.yml` builds everything on GitHub Actions |
| Code signing policy published on the project page | README → *Code signing policy* |
| Team roles named (authors, reviewers, approvers) | README → *Code signing policy* |
| Privacy statement: what the program sends over the network | README → *Privacy* |
| Multi-factor authentication for everyone with write access | **Turn on 2FA for your GitHub account** (Settings → Password and authentication) |

Suggested answers for the form:

- **Project:** Syscura, a free, open-source Windows health and security agent that explains Windows problems and fixes what it safely can. <https://github.com/hiibrarahmad/syscura>
- **What will be signed:** `Syscura.exe` (Tauri desktop app), `syscura-agent.exe` (Windows service), `syscura-cli.exe`, and the NSIS installer `Syscura-<version>-setup.exe` / `-setup-arm64.exe`.
- **Why it needs signing:** it installs a Windows service and asks for admin rights. Unsigned, it shows SmartScreen and "Unknown publisher" warnings that teach users to click through security prompts.
- **Build system:** GitHub Actions, `.github/workflows/release.yml`, triggered by version tags.

### After approval: set it up once

1. In SignPath, create a project with the slug **`syscura`** and connect it to the GitHub repository (Trusted Build System: GitHub.com).
2. Create two **artifact configurations**:

   Slug **`programs`** (the zip the workflow uploads contains the three programs):

   ```xml
   <?xml version="1.0" encoding="utf-8"?>
   <artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
     <zip-file>
       <pe-file path="Syscura.exe"><authenticode-sign /></pe-file>
       <pe-file path="syscura-agent.exe"><authenticode-sign /></pe-file>
       <pe-file path="syscura-cli.exe"><authenticode-sign /></pe-file>
     </zip-file>
   </artifact-configuration>
   ```

   Slug **`installer`**:

   ```xml
   <?xml version="1.0" encoding="utf-8"?>
   <artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
     <zip-file>
       <pe-file path="*.exe"><authenticode-sign /></pe-file>
     </zip-file>
   </artifact-configuration>
   ```

3. Use two signing policies: **`test-signing`** (manual workflow runs) and **`release-signing`** (version tags; SignPath Foundation approves each request).
4. In GitHub → Settings → Secrets and variables → Actions:
   - variable **`SIGNPATH_ORGANIZATION_ID`**: your SignPath organization id
   - secret **`SIGNPATH_API_TOKEN`**: an API token of a SignPath CI user that may submit to the project

Until these exist, the workflow builds unsigned files and skips the signing steps. Once they are set, it signs the programs, packs them into the installer, signs the installer, and fails if the installer's signature isn't valid.

---

## 2. Update signatures (already active)

This is separate from Authenticode. Every release's `SHA256SUMS.txt` is signed with Syscura's own minisign key (`SHA256SUMS.txt.sig`). The app has the public key built in ([`ui/src-tauri/src/update.rs`](../ui/src-tauri/src/update.rs)) and refuses any update whose checksum list is not signed by it. Someone who took over the GitHub account could swap the installer and the checksums, but couldn't sign them.

- The private key lives in the GitHub secret **`TAURI_SIGNING_PRIVATE_KEY`**, plus one offline backup kept by the author.
- **If the key is lost, installed copies can no longer update themselves.** People would have to download a new Setup by hand. Keep the backup safe.
- To check a locally built release: `cargo test -p syscura-ui -- --ignored verifies_dist`.

---

## 3. winget: no SmartScreen warning, available today

The release workflow writes winget manifests (artifact **`winget-manifests`**, made by [`scripts/winget-manifest.ps1`](../scripts/winget-manifest.ps1)). After publishing a release:

```powershell
winget install wingetcreate
wingetcreate submit --token <a GitHub token with public_repo> <unzipped winget-manifests>\manifests\i\IbrarAhmad\Syscura\<version>
```

This opens a pull request to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs). Microsoft's bots test the installer, and once it's merged:

```powershell
winget install IbrarAhmad.Syscura
```

The first submission is reviewed by a person and usually takes a few days. Later versions are often merged automatically. If Defender flags the unsigned installer during validation, send it to Microsoft as a false positive (below) and comment on the PR.

---

## 4. Report false positives to Microsoft

When Defender or SmartScreen flags a release, submit the file at <https://www.microsoft.com/wdsi/filesubmission> → *Software developer* → *Incorrectly detected as malware/malicious*. It's free and usually answered within a day or two. It also helps the file build reputation.
