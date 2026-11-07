<#
.SYNOPSIS
  Build the Risquette installer for a release.

.DESCRIPTION
  1. Builds risquette.exe (release) with the update server baked in.
  2. Builds risquette-setup.exe with risquette.exe embedded as its payload.
  3. Writes dist\RisquetteSetup-<version>-windows-x86_64.exe and a matching
     .sha256 file. These names are what the update server looks for on
     GitHub releases, and the app refuses updates without the checksum.

.PARAMETER UpdateUrl
  Base URL of the Risquette update server (Risquette-Backend). Defaults to
  $env:RISQUETTE_UPDATE_URL, and otherwise the app uses the official server,
  https://risquette.xnoid.studio. Users can change it in Settings > Updates.

.EXAMPLE
  ./scripts/package.ps1
  ./scripts/package.ps1 -UpdateUrl https://updates.example.com   # a self-hosted server
#>
param(
    [string]$UpdateUrl = $env:RISQUETTE_UPDATE_URL
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $meta = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    $version = ($meta.packages | Where-Object name -eq "risquette").version
    Write-Host "Packaging Risquette $version" -ForegroundColor Magenta

    if ($UpdateUrl) {
        $env:RISQUETTE_UPDATE_URL = $UpdateUrl.TrimEnd("/")
        Write-Host "Update server: $env:RISQUETTE_UPDATE_URL"
    } else {
        Remove-Item Env:RISQUETTE_UPDATE_URL -ErrorAction SilentlyContinue
        Write-Host "Update server: https://risquette.xnoid.studio (official)"
    }

    cargo build --release --locked -p risquette
    if ($LASTEXITCODE -ne 0) { throw "building risquette failed" }

    $env:RISQUETTE_PAYLOAD = Join-Path $root "target\release\risquette.exe"
    cargo build --release --locked -p risquette-installer
    if ($LASTEXITCODE -ne 0) { throw "building the installer failed" }
    Remove-Item Env:RISQUETTE_PAYLOAD

    $dist = Join-Path $root "dist"
    New-Item -ItemType Directory -Force $dist | Out-Null
    $name = "RisquetteSetup-$version-windows-x86_64.exe"
    $out = Join-Path $dist $name
    Copy-Item (Join-Path $root "target\release\risquette-setup.exe") $out -Force

    # Optional Authenticode signing: set SIGNTOOL_CERT (path to .pfx) and SIGNTOOL_PASSWORD.
    if ($env:SIGNTOOL_CERT) {
        $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" | Sort-Object FullName | Select-Object -Last 1
        & $signtool.FullName sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /f $env:SIGNTOOL_CERT /p $env:SIGNTOOL_PASSWORD $out
        if ($LASTEXITCODE -ne 0) { throw "signing failed" }
    }

    # Checksum last, after signing changes the file.
    $hash = (Get-FileHash $out -Algorithm SHA256).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText("$out.sha256", "$hash  $name`n")

    $size = "{0:N1} MB" -f ((Get-Item $out).Length / 1MB)
    Write-Host "Wrote $out ($size)" -ForegroundColor Green
    Write-Host "Wrote $out.sha256"
} finally {
    Pop-Location
}
