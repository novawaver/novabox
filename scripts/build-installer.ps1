<#
.SYNOPSIS
Builds the Novabox Windows installer and the update feed that installed copies check.

.DESCRIPTION
Runs `tauri build` for the NSIS installer, signs it with the updater key, and writes
release\<version>\ with:

  Novabox_<version>_x64-setup.exe      the installer to publish
  Novabox_<version>_x64-setup.exe.sig  its updater signature
  latest.json                          the feed (plugins.updater.endpoints in tauri.conf.json)

Attach all three to the GitHub release tagged v<version> and publish it. Installed copies
find it on their next check and offer the install under Settings > About.

Needs the updater private key, made once with:
  npx tauri signer generate -w "$HOME\.tauri\novabox.key"
Its public half (the .pub file) must be the plugins.updater.pubkey in tauri.conf.json,
or installed copies reject the update.

.PARAMETER Notes
Release notes shown in the app before installing. Defaults to the "## <version>" section
of CHANGELOG.md when there is one.

.PARAMETER DownloadBase
URL folder the installer will be served from. Defaults to this version's GitHub release,
derived from the endpoint in tauri.conf.json.

.PARAMETER KeyPath
The updater private key. Defaults to $env:TAURI_SIGNING_PRIVATE_KEY_PATH, then
$HOME\.tauri\novabox.key. A key password goes in $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD.

.PARAMETER SkipBuild
Reuse the installer already in src-tauri\target\release\bundle\nsis and only rewrite the feed.

.EXAMPLE
npm run build:installer -- -Notes "Fixes sign-in on first start."
#>
[CmdletBinding()]
param(
  [string]$Notes = "",
  [string]$DownloadBase = "",
  [string]$KeyPath = "",
  [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Read-Json([string]$path) {
  Get-Content -LiteralPath $path -Raw -Encoding UTF8 | ConvertFrom-Json
}

# 1. The version comes from tauri.conf.json. The other manifests must agree, so the
#    installer name, the feed, and the app's own idea of its version all match.
$conf = Read-Json "src-tauri\tauri.conf.json"
$version = [string]$conf.version
$pkgVersion = [string](Read-Json "package.json").version
$cargoMatch = Select-String -Path "src-tauri\Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
$cargoVersion = $cargoMatch.Matches[0].Groups[1].Value
if ($pkgVersion -ne $version -or $cargoVersion -ne $version) {
  throw "Version mismatch: tauri.conf.json $version, package.json $pkgVersion, Cargo.toml $cargoVersion. Set all three to the same version."
}

# 2. The signing key. tauri build reads it from the environment and writes the .sig file.
if (-not $KeyPath) { $KeyPath = $env:TAURI_SIGNING_PRIVATE_KEY_PATH }
if (-not $KeyPath) { $KeyPath = Join-Path $HOME ".tauri\novabox.key" }
if (-not (Test-Path -LiteralPath $KeyPath)) {
  throw "Updater key not found at $KeyPath. Create one with: npx tauri signer generate -w `"$KeyPath`" and put the .pub contents in src-tauri\tauri.conf.json under plugins.updater.pubkey."
}
$pubPath = "$KeyPath.pub"
if (Test-Path -LiteralPath $pubPath) {
  $pub = (Get-Content -LiteralPath $pubPath -Raw).Trim()
  if ($pub -ne [string]$conf.plugins.updater.pubkey) {
    throw "The public key next to $KeyPath is not plugins.updater.pubkey in tauri.conf.json. Installed copies would reject this update."
  }
}
$env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content -LiteralPath $KeyPath -Raw).Trim()
if ($null -eq $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) { $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "" }

# 3. Build. The frontend (npm run build) runs first through beforeBuildCommand.
if (-not $SkipBuild) {
  if (-not (Test-Path -LiteralPath ".env") -and -not $env:NOVABOX_GOOGLE_CLIENT_ID) {
    Write-Warning "No .env and no NOVABOX_GOOGLE_CLIENT_ID: this installer cannot sign in to Google. See .env.example."
  }
  Write-Host "Building Novabox $version..."
  & npx tauri build --bundles nsis
  if ($LASTEXITCODE -ne 0) { throw "tauri build failed with exit code $LASTEXITCODE." }
}

# 4. Collect the installer and its signature.
$bundleDir = Join-Path $root "src-tauri\target\release\bundle\nsis"
$installer = Get-ChildItem -LiteralPath $bundleDir -Filter "*_${version}_*-setup.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if ($null -eq $installer) { throw "No installer for $version in $bundleDir. Run again without -SkipBuild." }
$sigPath = "$($installer.FullName).sig"
if (-not (Test-Path -LiteralPath $sigPath)) {
  throw "No signature next to $($installer.Name). Is bundle.createUpdaterArtifacts true in tauri.conf.json?"
}

$outDir = Join-Path $root "release\$version"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
Copy-Item -LiteralPath $installer.FullName -Destination $outDir -Force
Copy-Item -LiteralPath $sigPath -Destination $outDir -Force

# 5. The feed. An installed copy compares `version` with its own, then downloads `url`
#    and checks it against `signature` before running it.
if (-not $DownloadBase) {
  $endpoint = [string]$conf.plugins.updater.endpoints[0]
  if ($endpoint -match '^(https://github\.com/[^/]+/[^/]+)/releases/') {
    $DownloadBase = "$($Matches[1])/releases/download/v$version"
  } else {
    throw "Pass -DownloadBase: the URL folder $($installer.Name) will be served from."
  }
}
if (-not $Notes -and (Test-Path -LiteralPath "CHANGELOG.md")) {
  $section = New-Object System.Collections.Generic.List[string]
  $inside = $false
  foreach ($line in (Get-Content -LiteralPath "CHANGELOG.md" -Encoding UTF8)) {
    if ($line -match '^##\s+') {
      if ($inside) { break }
      $inside = $line -match ("^##\s+v?" + [regex]::Escape($version) + "(\s|$)")
      continue
    }
    if ($inside) { $section.Add($line) }
  }
  $Notes = ($section -join "`n").Trim()
}
$feed = [ordered]@{
  version   = $version
  notes     = $Notes
  pub_date  = [DateTime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = [ordered]@{
    "windows-x86_64" = [ordered]@{
      signature = (Get-Content -LiteralPath $sigPath -Raw).Trim()
      url       = "$($DownloadBase.TrimEnd('/'))/$($installer.Name)"
    }
  }
}
$feedPath = Join-Path $outDir "latest.json"
$utf8 = New-Object System.Text.UTF8Encoding $false
[IO.File]::WriteAllText($feedPath, ($feed | ConvertTo-Json -Depth 5), $utf8)

Write-Host ""
Write-Host "Release files in $outDir"
Get-ChildItem -LiteralPath $outDir | ForEach-Object { Write-Host ("  {0,-42} {1,12:N0} bytes" -f $_.Name, $_.Length) }
Write-Host ""
Write-Host "Publish: create the GitHub release v$version, attach all three files, and publish it."
Write-Host "Installed copies read latest.json from the release marked Latest, so a draft or pre-release is not offered."
