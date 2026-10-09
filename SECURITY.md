# Security policy

Syscura runs with high privileges when installed as a service, so security reports are taken seriously.

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

Report privately through [GitHub's private vulnerability reporting](https://github.com/hiibrarahmad/syscura/security/advisories/new). Include what you found, how to reproduce it, and the impact you expect. You will get a reply within a few days, and credit in the release notes if you want it.

## Supported versions

Only the latest release receives fixes.

## Design promises you can hold us to

- The agent can only perform the fixed actions listed in `crates/syscura-agent/src/actions.rs`. No rule, file or network response can make it run an arbitrary command.
- Event data is treated as untrusted input: service names are validated, file paths are passed to Windows tools through environment variables rather than inside scripts.
- The local named pipe rejects remote clients, and normal users cannot create pipe instances (no impersonation of the agent).
- Any signed-in program may read through the pipe, but only Syscura's own programs (next to the agent, in Program Files) or an elevated process may change anything: run fixes, undo, ignore, mark as safe, or change options.
- The service's program always lives in a folder only administrators can change. A portable copy is copied to Program Files before it is installed as a service.
- Updates are installed only when the release's checksum list carries a valid signature from Syscura's release key, and the download matches its checksum.
- Releases are built by GitHub Actions from this repository. Dependencies are checked on every change for known vulnerabilities (`cargo deny`, `npm audit`).
- Downloaded pictures are accepted only if their bytes are a raster image (PNG, JPEG, WebP, GIF, AVIF); SVG is refused.
- Nothing is deleted by a fix; reversible changes record an undo step.

If you find a way around any of these, that is a vulnerability. Please report it.
