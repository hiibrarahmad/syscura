# Contributing to Syscura

Thanks for helping! Syscura aims to be the PC health tool people trust, so the bar is simple: **accurate, safe, tiny.**

## Ways to help

| You have... | Do this |
|---|---|
| 10 minutes | Report a false alarm, a missed problem or a bug with the [issue forms](https://github.com/hiibrarahmad/syscura/issues/new/choose). |
| 30 minutes | [Add your motherboard](#adding-a-board-profile) so others with the same board get the exact photo view. |
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

## Adding a board profile

A board profile ([`ui/src/lib/board/profiles.ts`](ui/src/lib/board/profiles.ts)) is the maker's top-down photo plus the position of each labelled part as `[x, y, width, height]` in **percent of the photo**.

1. Find the board's product page on the maker's site and its top-down photo (the one link previews show).
2. Open the photo in any editor that shows pixel positions. Use the white labels printed on the board (`DIMM_A1`, `PCIEX16_1`, `M.2_1`, ...) to identify each part. Never guess a label.
3. Add the profile and the photo link to `BOARD_PHOTOS` in [`crates/syscura-online/src/query.rs`](crates/syscura-online/src/query.rs).
4. Check it in the app (Hardware → Real photo, tick *All labels*) and include a screenshot in your pull request.

Can't code? Open a [board profile request](https://github.com/hiibrarahmad/syscura/issues/new?template=board_profile.yml) with the product page link.

## Pull requests

- Keep changes focused; one topic per PR.
- `cargo clippy ... -D warnings` and `cargo test --workspace` must pass (CI checks both).
- Match the style of the code around you. Comments explain *why*.
- Anything touching fixes or security gets an extra careful review. That is a feature, not friction.

## Code of conduct

Be kind and constructive. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
