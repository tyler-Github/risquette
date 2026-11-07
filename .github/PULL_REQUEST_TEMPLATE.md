## What this changes

<!-- A short summary of the change and why it's needed. -->

Closes #

## Type of change

- [ ] Bug fix
- [ ] New feature
- [ ] UI / look and feel
- [ ] Refactor or cleanup (no behaviour change)
- [ ] Docs, branding or CI
- [ ] Installer, updater or release tooling

## Screenshots

<!-- For UI changes, add before/after screenshots, ideally in light and dark themes. Crop out personal details. -->

## How it was tested

<!-- What you ran and what you checked by hand. -->

- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass (CI runs the same checks)
- [ ] Tried it in the running app
- [ ] For features that change data on the server, I went through the relevant items in the [manual checklist](https://github.com/tyler-Github/risquette/blob/main/docs/development.md#manual-checklist)

## Checklist

- [ ] Automated tests don't send write requests (POST/PUT/PATCH/DELETE) to a real Discuit instance. They use the local test server instead.
- [ ] No cookies, passwords or tokens in code, logs, tests or screenshots
- [ ] Docs updated if behaviour changed (`README.md`, `docs/`)
- [ ] Branding images regenerated if the icon or screenshots changed (`python scripts/make_branding.py`)
