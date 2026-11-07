<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../branding/headers/releasing-dark.png">
  <img alt="Risquette: Releasing and updates" src="../branding/headers/releasing-light.png">
</picture>

Risquette ships as a Windows installer attached to GitHub releases. The update server reads those releases and tells the app when a newer version exists.

```
GitHub release ──► Risquette-Backend ──► app: "0.2.0 is available"
 (installer + .sha256)   (/v1/updates)       download → verify → installer --update
```

<table>
  <tr>
    <td width="54%"><img alt="The in-app update dialog" src="../branding/framed/update-dialog.png"></td>
    <td width="46%" valign="middle"><img alt="The updater window replacing the app" src="../branding/framed/installer-update.png"></td>
  </tr>
  <tr>
    <td align="center"><sub>1. The app offers the update with its release notes</sub></td>
    <td align="center"><sub>2. The installer swaps in the new version and reopens the app</sub></td>
  </tr>
</table>

## Cutting a release

Only the repository owner can publish releases. The "Protect release tags" ruleset stops anyone else creating, moving or deleting `v*` tags, the release job refuses to run for anyone but the owner, and it runs in the `release` environment, which only accepts `v*` tags.

1. Bump `version` in the root `Cargo.toml`.
2. Tag and push:
   ```sh
   git tag v0.2.0 && git push origin v0.2.0
   ```
   A tag with a suffix, such as `v0.3.0-beta.1`, becomes a pre-release and is offered only on the beta channel. That makes a beta the safe way to try a release: switch one install to the beta channel in Settings → Updates and check that it updates before you publish the stable version.
3. `.github/workflows/release.yml` runs the tests, builds the installer and attaches two files to the release:
   - `RisquetteSetup-<version>-windows-x86_64.exe`
   - `RisquetteSetup-<version>-windows-x86_64.exe.sha256`. The app won't install an update without this file.

Release notes come from GitHub's automatic notes (the pull requests merged since the last release). Edit the release text on GitHub if you want to say more; the app's update dialog shows it as the "What's new" section.

### Settings

| Kind | Name | Purpose |
| --- | --- | --- |
| Repository variable (optional) | `RISQUETTE_UPDATE_URL` | A different update server to bake into the app. Without it, builds use the official server, `https://risquette.xnoid.studio`. |
| `release` environment secret (optional) | `SIGNTOOL_CERT_BASE64` | A code-signing `.pfx`, base64-encoded. |
| `release` environment secret (optional) | `SIGNTOOL_PASSWORD` | Its password. |

Put the signing secrets in the `release` environment (Settings → Environments → release) rather than at repository level, so only release runs can read them.

## Building an installer locally

```powershell
./scripts/package.ps1
```

Add `-UpdateUrl https://updates.example.com` to build against a self-hosted update server.

This writes the installer and its checksum to `dist/`.

## Installer command line

<p align="center">
  <img alt="The Risquette setup wizard" src="../branding/framed/installer-welcome.png" width="620">
</p>

| Command | Effect |
| --- | --- |
| `risquette-setup.exe` | The install wizard. |
| `risquette-setup.exe --silent [--dir PATH]` | An unattended install. |
| `risquette-setup.exe --update --wait-pid N --launch` | Used by the app's updater: waits for the app to exit, replaces it, then reopens it. |
| `uninstall.exe [--silent]` | Uninstall. Opening `uninstall.exe` in the install folder shows the uninstaller; Apps & features runs `uninstall.exe --uninstall`. |

Installs are per-user and need no administrator rights:

- Files go to `%LOCALAPPDATA%\Programs\Risquette`.
- The uninstall entry goes under `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Risquette`.
- Uninstalling removes the app, its shortcuts and the uninstall entry. If the user ticks "Also remove my settings and saved logins", it also deletes `%APPDATA%\Risquette` (settings and logins) and `%LOCALAPPDATA%\Risquette` (logs and crash reports).

## The update server

The official server runs at `https://risquette.xnoid.studio` on Fly.io, behind Cloudflare. It also serves the project website at `/`. Its code is in the separate **Risquette-Backend** project, whose README covers deploying to Fly. To run your own, deploy it anywhere that runs a container or a Rust binary, and point it at this repository:

| Variable | Purpose |
| --- | --- |
| `GITHUB_REPO` | `owner/name` of this repository (default `tyler-Github/risquette`). |
| `GITHUB_TOKEN` | Optional. Raises GitHub's rate limit. |
| `CACHE_SECONDS` | How long to cache release data (default 300). |
| `BIND` | Listen address (default `0.0.0.0:8787`). |

The app calls `GET /v1/updates/windows-x86_64?channel=stable&current=<version>`. Users can point the app at a different server in Settings → Updates.
