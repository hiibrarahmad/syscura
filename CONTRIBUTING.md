# Contributing to Syscura

Thanks for helping! Syscura aims to be the PC health tool people trust, so the bar is simple: **accurate, safe, tiny.**

## Ways to help

| You have... | Do this |
|---|---|
| 10 minutes | Report a false alarm, a missed problem or a bug with the [issue forms](https://github.com/hiibrarahmad/syscura/issues/new/choose). |
| 15 minutes | [Report wrong or missing hardware info](https://github.com/hiibrarahmad/syscura/issues/new?template=hardware_info.yml) with the app's **Copy report**, so Syscura reads your kind of PC correctly. |
| An evening | Improve or add a [rule](#adding-or-changing-a-rule), or pick an issue labelled `good first issue`. |

## Development setup

You need Windows 10/11, Rust stable (MSVC toolchain), the Visual Studio C++ Build Tools and Node.js 20+.

```powershell
git clone https://github.com/hiibrarahmad/syscura
cd syscura\ui; npm ci; npm run build; cd ..
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Run it:

```powershell
cargo run -p syscura-agent -- run --data-dir .\dev-data   # agent in a console
cd ui; npx tauri dev                                        # app with hot reload
```

## Adding or changing a rule

Rules live in [`kb/rules.toml`](kb/rules.toml). Each rule matches Windows events and says, in plain language, what happened, whether it is harmful, what to do, and which fixes are allowed.

- Write for a non-technical reader. No jargon without explaining it.
- Be honest about harm: `no`, `maybe` or `yes`. When in doubt, `maybe`.
- Fixes may only use the actions the agent implements (see `crates/syscura-agent/src/actions.rs`). New actions need a code review of their own: they must never delete data and should record an undo step where possible.
- `cargo test -p syscura-rules` checks every rule parses and only uses known actions.

## Improving hardware detection

Hardware is read in [`crates/syscura-hw`](crates/syscura-hw) (WMI, SetupAPI, the registry, SMBIOS and NVIDIA's NVML), and turned into the report in [`ui/src/lib/report.ts`](ui/src/lib/report.ts).

- Only show what the PC actually reports. If a value is missing, the app says *Not reported*. Never guess.
- Makers fill placeholder text into firmware ("To Be Filled By O.E.M.", "Default string"). Add new ones to `is_placeholder` in [`crates/syscura-hw/src/parse.rs`](crates/syscura-hw/src/parse.rs) with a test.
- Parsing lives in pure functions in `parse.rs` so it can be tested without the hardware.

## Design

The app uses the **Verdict** design: tokens and components in [`ui/src/app.css`](ui/src/app.css). Colour comes from tokens only; indigo is for actions and the one highlighted panel per screen; green, amber and red are only for status. Each screen has one headline that answers "is my PC OK?".

## Pull requests

- Keep changes focused; one topic per PR.
- `cargo clippy ... -D warnings` and `cargo test --workspace` must pass (CI checks both).
- Match the style of the code around you. Comments explain *why*.
- Anything touching fixes or security gets an extra careful review. That is a feature, not friction.

## Code of conduct

Be kind and constructive. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
