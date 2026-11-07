# Contributing to Risquette

Thanks for helping out! Bug reports, ideas and pull requests are all welcome.

## Reporting bugs and ideas

Use the [issue forms](https://github.com/tyler-Github/risquette/issues/new/choose). If something also goes wrong on the Discuit website, it's a Discuit issue: report it [to Discuit](https://github.com/discuitnet/discuit/issues) instead.

## Making a change

1. **Fork** the repository and create a branch for your change. Collaborators can branch in the main repository instead.
2. Make the change. [docs/development.md](docs/development.md) covers building, running and testing.
3. Before pushing, run the same checks as CI:
   ```sh
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
4. If people using the app will notice the change, **add a line to [CHANGELOG.md](CHANGELOG.md)** under `## [Unreleased]`, in plain words. It becomes the release notes users read in the update dialog. Maintainers label other pull requests `no-changelog`.
5. **Open a pull request** against `main` and fill in the template.

## How pull requests get merged

- CI checks formatting, lints, tests, doc links and the changelog, and audits dependencies. Every check must pass.
- Pull requests from first-time contributors wait for a maintainer to approve the CI run. That's a safety measure, not a judgement.
- Each pull request needs an approving review from a collaborator. Changes to CI, the release pipeline, the installer and updater, or dependencies need the owner's review ([CODEOWNERS](.github/CODEOWNERS)).
- Only the owner publishes releases.

## Ground rules

- **Never test write actions against a real Discuit instance from automated tests.** discuit.org is a production site. Tests use the local test server in `crates/discuit-api`, and features that change data are checked by hand using the [manual checklist](docs/development.md#manual-checklist).
- Keep cookies, passwords and tokens out of code, logs, tests and screenshots.
- Update the docs when behaviour changes.

By contributing, you agree that your contributions are licensed under the [Apache 2.0 licence](LICENSE).
