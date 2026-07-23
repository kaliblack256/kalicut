# Build self-contained Windows x64 portable zip.
# Run on Windows (GitHub Actions windows-latest).
#
# Output: dist/kalicut-<ver>-windows-x86_64.zip
# Contents: kalicut.exe, ffmpeg.exe, ffprobe.exe, README, RUN.bat
#
# Preview uses ffmpeg fallback (no libmpv link on MSVC). Cut/export fully work.

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$Version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"').Matches.Groups[1].Value
$Name = "kalicut-$Version-windows-x86_64"
$OutDir = Join-Path $Root "dist\$Name"
$ZipPath = Join-Path $Root "dist\$Name.zip"
$Cache = Join-Path $Root "dist\win-cache"
New-Item -ItemType Directory -Force -Path $Cache | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Root "dist") | Out-Null

Write-Host "==> KALICUT Windows package $Version"

# --- ffmpeg (static essentials) ---
$FfmpegZip = Join-Path $Cache "ffmpeg-essentials.zip"
if (-not (Test-Path $FfmpegZip)) {
    $url = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
    Write-Host "==> Downloading ffmpeg essentials"
    Invoke-WebRequest -Uri $url -OutFile $FfmpegZip -UseBasicParsing
}
$FfmpegExtract = Join-Path $Cache "ffmpeg"
if (-not (Test-Path (Join-Path $FfmpegExtract "ffmpeg.exe"))) {
    Write-Host "==> Extracting ffmpeg"
    if (Test-Path $FfmpegExtract) { Remove-Item -Recurse -Force $FfmpegExtract }
    Expand-Archive -Path $FfmpegZip -DestinationPath $FfmpegExtract -Force
}
$FfmpegBin = Get-ChildItem -Path $FfmpegExtract -Recurse -Filter ffmpeg.exe | Select-Object -First 1
$FfprobeBin = Get-ChildItem -Path $FfmpegExtract -Recurse -Filter ffprobe.exe | Select-Object -First 1
if (-not $FfmpegBin -or -not $FfprobeBin) {
    throw "ffmpeg.exe / ffprobe.exe not found after extract"
}
Write-Host "ffmpeg: $($FfmpegBin.FullName)"

# --- build (no libmpv: portable + MSVC-friendly; ffmpeg preview path in app) ---
Write-Host "==> cargo build --release --no-default-features"
$env:CARGO_TERM_COLOR = "always"
cargo build --release --no-default-features --bin kalicut
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$Exe = Join-Path $Root "target\release\kalicut.exe"
if (-not (Test-Path $Exe)) { throw "kalicut.exe missing" }

# --- stage package ---
if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Copy-Item $Exe (Join-Path $OutDir "kalicut.exe")
Copy-Item $FfmpegBin.FullName (Join-Path $OutDir "ffmpeg.exe")
Copy-Item $FfprobeBin.FullName (Join-Path $OutDir "ffprobe.exe")

# Optional: copy any companion DLLs next to static essentials (usually none)
$ffmpegDir = $FfmpegBin.DirectoryName
Get-ChildItem $ffmpegDir -Filter *.dll -ErrorAction SilentlyContinue | ForEach-Object {
    Copy-Item $_.FullName $OutDir -Force
}

Copy-Item (Join-Path $Root "LICENSE") $OutDir -ErrorAction SilentlyContinue
Copy-Item (Join-Path $Root "README.md") $OutDir -ErrorAction SilentlyContinue

@"
@echo off
REM KALICUT portable launcher — uses ffmpeg.exe next to this script
set "PATH=%~dp0;%PATH%"
start "" "%~dp0kalicut.exe" %*
"@ | Set-Content -Path (Join-Path $OutDir "KALICUT.bat") -Encoding ASCII

@"
KALICUT $Version for Windows x64
================================

Windows 10 / 11 (64-bit).

Self-contained:
  - kalicut.exe
  - ffmpeg.exe / ffprobe.exe  (cut, export, preview)

Run:
  Double-click  KALICUT.bat
  or            kalicut.exe

If Windows SmartScreen blocks the download:
  More info → Run anyway
  (unsigned open-source build)

Preview uses the ffmpeg path (no separate mpv install required).
Export is lossless stream-copy by default.

Source: https://github.com/kaliblack256/kalicut
"@ | Set-Content -Path (Join-Path $OutDir "RUN.txt") -Encoding UTF8

# zip
if (Test-Path $ZipPath) { Remove-Item -Force $ZipPath }
Write-Host "==> Compress-Archive $ZipPath"
Compress-Archive -Path (Join-Path $OutDir "*") -DestinationPath $ZipPath -Force

# checksum
$sha = (Get-FileHash -Algorithm SHA256 $ZipPath).Hash.ToLower()
"$sha  $(Split-Path $ZipPath -Leaf)" | Set-Content -Path "$ZipPath.sha256" -Encoding ASCII

Write-Host "Created: $ZipPath"
Get-Item $ZipPath | Format-List Name, Length
Write-Host "SHA256: $sha"
