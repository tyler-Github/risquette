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

1. Bump `version` in the root `Cargo.toml`.
2. Tag and push:
   ```sh
   git tag v0.2.0 && git push origin v0.2.0
   ```
   A tag with a suffix, such as `v0.3.0-beta.1`, becomes a pre-release and is offered only on the beta channel.
3. `.github/workflows/release.yml` runs the tests, builds the installer and attaches two files to the release:
   - `RisquetteSetup-<version>-windows-x86_64.exe`
   - `RisquetteSetup-<version>-windows-x86_64.exe.sha256`. The app won't install an update without this file.

### Repository settings

| Kind | Name | Purpose |
| --- | --- | --- |
| Variable (optional) | `RISQUETTE_UPDATE_URL` | A different update server to bake into the app. Without it, builds use the official server, `https://risquette.xnoid.studio`. |
| Secret (optional) | `SIGNTOOL_CERT_BASE64` | A code-signing `.pfx`, base64-encoded. |
| Secret (optional) | `SIGNTOOL_PASSWORD` | Its password. |

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
| `uninstall.exe --uninstall [--silent]` | Uninstall. Apps & features runs this too. |

Installs are per-user and need no administrator rights:

- Files go to `%LOCALAPPDATA%\Programs\Risquette`.
- The uninstall entry goes under `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Risquette`.

## The update server

The official server runs at `https://risquette.xnoid.studio`. Risquette-Backend is a separate project; to run your own, deploy it anywhere that runs a container or a Rust binary, and point it at this repository:

| Variable | Purpose |
| --- | --- |
| `GITHUB_REPO` | `owner/name` of this repository (default `tyler-Github/risquette`). |
| `GITHUB_TOKEN` | Optional. Raises GitHub's rate limit. |
| `CACHE_SECONDS` | How long to cache release data (default 300). |
| `BIND` | Listen address (default `0.0.0.0:8787`). |

The app calls `GET /v1/updates/windows-x86_64?channel=stable&current=<version>`. Users can point the app at a different server in Settings → Updates.
