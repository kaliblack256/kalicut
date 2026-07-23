# Build self-contained Windows x64 portable zip for Win10/11.
# Run on Windows (GitHub Actions windows-latest) or a local MSVC + Rust box.
#
# Output: dist/kalicut-<ver>-windows-x86_64.zip
# Contents:
#   kalicut.exe, libmpv-2.dll, ffmpeg.exe, ffprobe.exe, KALICUT.bat, docs
#
# Env (optional):
#   MPV_DEV_URL          override mpv-dev 7z URL
#   KALICUT_SKIP_MPV=1   build without embedded-mpv (ffmpeg preview only)
#   WINDOWS_PFX_BASE64   base64 of code-signing .pfx (Authenticode)
#   WINDOWS_PFX_PASSWORD password for .pfx
#   WINDOWS_TIMESTAMP_URL  RFC3161 timestamp (default DigiCert)
#   SIGN_MODE=self       create ephemeral self-signed cert (local test only)

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

function Get-7Zip {
    $cmd = Get-Command 7z -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $paths = @(
        "$env:ProgramFiles\7-Zip\7z.exe",
        "${env:ProgramFiles(x86)}\7-Zip\7z.exe"
    )
    foreach ($p in $paths) {
        if (Test-Path $p) { return $p }
    }
    $sevenZr = Join-Path $Cache "7zr.exe"
    if (-not (Test-Path $sevenZr)) {
        Write-Host "==> Downloading 7zr.exe"
        Invoke-WebRequest -Uri "https://www.7-zip.org/a/7zr.exe" -OutFile $sevenZr -UseBasicParsing
    }
    return $sevenZr
}

function Enter-VsDevEnv {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path $vswhere)) {
        throw "vswhere.exe not found — need Visual Studio Build Tools / MSVC"
    }
    $vsPath = & $vswhere -latest -products * `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath
    if (-not $vsPath) {
        throw "MSVC x64 tools not found"
    }
    $vcvars = Join-Path $vsPath "VC\Auxiliary\Build\vcvars64.bat"
    if (-not (Test-Path $vcvars)) {
        throw "vcvars64.bat missing at $vcvars"
    }
    Write-Host "==> MSVC env: $vsPath"
    # Import env from vcvars into this process
    $tempBat = Join-Path $env:TEMP "kalicut-vcvars-$PID.bat"
    @"
@echo off
call "$vcvars" >nul
set
"@ | Set-Content -Path $tempBat -Encoding ASCII
    $vars = & cmd /c "`"$tempBat`""
    Remove-Item $tempBat -Force -ErrorAction SilentlyContinue
    foreach ($line in $vars) {
        $idx = $line.IndexOf('=')
        if ($idx -gt 0) {
            $name = $line.Substring(0, $idx)
            $val = $line.Substring($idx + 1)
            [Environment]::SetEnvironmentVariable($name, $val, "Process")
        }
    }
    if (-not (Get-Command dumpbin -ErrorAction SilentlyContinue)) {
        throw "dumpbin not on PATH after vcvars"
    }
    if (-not (Get-Command lib -ErrorAction SilentlyContinue)) {
        throw "lib.exe not on PATH after vcvars"
    }
}

function New-MpvImportLibrary {
    param(
        [Parameter(Mandatory = $true)][string]$DllPath,
        [Parameter(Mandatory = $true)][string]$OutDir
    )
    $dllName = Split-Path $DllPath -Leaf
    $defPath = Join-Path $OutDir "mpv.def"
    $libPath = Join-Path $OutDir "mpv.lib"
    $expPath = Join-Path $OutDir "mpv.exp"

    Write-Host "==> Creating import lib for $dllName"
    $raw = & dumpbin /EXPORTS $DllPath 2>&1 | Out-String
    $names = [System.Collections.Generic.List[string]]::new()
    foreach ($line in ($raw -split "`r?`n")) {
        # dumpbin: "          1    0 00012345 mpv_create"
        if ($line -match '^\s+\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]+\s+(\S+)\s*$') {
            $sym = $Matches[1]
            if ($sym -and $sym -ne "name") { [void]$names.Add($sym) }
        }
    }
    if ($names.Count -lt 10) {
        throw "Too few exports from $DllPath ($($names.Count)). dumpbin output may have failed."
    }
    $def = New-Object System.Text.StringBuilder
    [void]$def.AppendLine("LIBRARY $dllName")
    [void]$def.AppendLine("EXPORTS")
    foreach ($n in $names) { [void]$def.AppendLine("    $n") }
    [System.IO.File]::WriteAllText($defPath, $def.ToString())

    & lib.exe /nologo /def:$defPath /out:$libPath /machine:x64
    if ($LASTEXITCODE -ne 0) { throw "lib.exe failed creating mpv.lib" }
    if (-not (Test-Path $libPath)) { throw "mpv.lib was not created" }
    Write-Host "    exports: $($names.Count) → $libPath"
    Remove-Item $expPath -Force -ErrorAction SilentlyContinue
    return $libPath
}

function Invoke-CodeSign {
    param(
        [Parameter(Mandatory = $true)][string[]]$Paths,
        [Parameter(Mandatory = $true)][string]$PfxPath,
        [string]$Password = "",
        [string]$TimestampUrl = "http://timestamp.digicert.com"
    )
    $signtool = Get-Command signtool -ErrorAction SilentlyContinue
    if (-not $signtool) {
        # Common SDK locations when PATH is incomplete
        $sdkRoots = @(
            "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
        )
        foreach ($root in $sdkRoots) {
            if (Test-Path $root) {
                $found = Get-ChildItem -Path $root -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
                    Where-Object { $_.FullName -match '\\x64\\' } |
                    Select-Object -First 1
                if ($found) {
                    $signtool = $found
                    break
                }
            }
        }
    }
    if (-not $signtool) {
        throw "signtool.exe not found (install Windows SDK)"
    }
    $st = if ($signtool.Source) { $signtool.Source } else { $signtool.FullName }
    foreach ($p in $Paths) {
        if (-not (Test-Path $p)) { continue }
        Write-Host "    signtool: $(Split-Path $p -Leaf)"
        $args = @(
            "sign", "/fd", "SHA256", "/td", "SHA256",
            "/tr", $TimestampUrl,
            "/f", $PfxPath
        )
        if ($Password) { $args += @("/p", $Password) }
        $args += $p
        & $st @args
        if ($LASTEXITCODE -ne 0) { throw "signtool failed for $p" }
    }
}

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

# --- libmpv (shinchiro/zhongfly mpv-dev: single fat libmpv-2.dll) ---
$SkipMpv = ($env:KALICUT_SKIP_MPV -eq "1")
$MpvDll = $null
$MpvLibDir = $null
$SignInfo = "unsigned"

if (-not $SkipMpv) {
    Enter-VsDevEnv

    # Pinned release (reproducible). Override with MPV_DEV_URL.
    $MpvDevUrl = $env:MPV_DEV_URL
    if (-not $MpvDevUrl) {
        $MpvDevUrl = "https://github.com/zhongfly/mpv-winbuild/releases/download/2026-07-22-8ab3f8b66d/mpv-dev-x86_64-20260722-git-8ab3f8b66d.7z"
    }
    $MpvArchive = Join-Path $Cache "mpv-dev-x86_64.7z"
    $MpvRoot = Join-Path $Cache "mpv-dev"
    if (-not (Test-Path $MpvArchive)) {
        Write-Host "==> Downloading mpv-dev"
        Write-Host "    $MpvDevUrl"
        Invoke-WebRequest -Uri $MpvDevUrl -OutFile $MpvArchive -UseBasicParsing
    }
    $MpvDllCandidate = Join-Path $MpvRoot "libmpv-2.dll"
    if (-not (Test-Path $MpvDllCandidate)) {
        Write-Host "==> Extracting mpv-dev"
        if (Test-Path $MpvRoot) { Remove-Item -Recurse -Force $MpvRoot }
        New-Item -ItemType Directory -Force -Path $MpvRoot | Out-Null
        $seven = Get-7Zip
        & $seven x "-o$MpvRoot" "-y" $MpvArchive
        if ($LASTEXITCODE -ne 0) { throw "7z extract failed for mpv-dev" }
    }
    $MpvDll = Get-ChildItem -Path $MpvRoot -Recurse -Filter "libmpv-2.dll" | Select-Object -First 1
    if (-not $MpvDll) { throw "libmpv-2.dll not found in mpv-dev archive" }
    $MpvLibDir = $MpvDll.DirectoryName
    Write-Host "libmpv: $($MpvDll.FullName)"

    New-MpvImportLibrary -DllPath $MpvDll.FullName -OutDir $MpvLibDir | Out-Null

    $env:KALICUT_MPV_DIR = $MpvLibDir
    # libmpv2-sys links -lmpv; build.rs + RUSTFLAGS point the linker at mpv.lib
    if ($env:RUSTFLAGS) {
        $env:RUSTFLAGS = "$($env:RUSTFLAGS) -L native=$MpvLibDir"
    } else {
        $env:RUSTFLAGS = "-L native=$MpvLibDir"
    }
    # Also set LIB for MSVC link.exe fallback
    if ($env:LIB) {
        $env:LIB = "$MpvLibDir;$env:LIB"
    } else {
        $env:LIB = $MpvLibDir
    }

    Write-Host "==> cargo build --release (embedded-mpv + libmpv-2.dll)"
    $env:CARGO_TERM_COLOR = "always"
    cargo build --release --bin kalicut
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
} else {
    Write-Host "==> cargo build --release --no-default-features (no libmpv)"
    $env:CARGO_TERM_COLOR = "always"
    cargo build --release --no-default-features --bin kalicut
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

$Exe = Join-Path $Root "target\release\kalicut.exe"
if (-not (Test-Path $Exe)) { throw "kalicut.exe missing" }

# --- stage package ---
if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Copy-Item $Exe (Join-Path $OutDir "kalicut.exe")
Copy-Item $FfmpegBin.FullName (Join-Path $OutDir "ffmpeg.exe")
Copy-Item $FfprobeBin.FullName (Join-Path $OutDir "ffprobe.exe")

$ffmpegDir = $FfmpegBin.DirectoryName
Get-ChildItem $ffmpegDir -Filter *.dll -ErrorAction SilentlyContinue | ForEach-Object {
    Copy-Item $_.FullName $OutDir -Force
}

if ($MpvDll) {
    Copy-Item $MpvDll.FullName (Join-Path $OutDir "libmpv-2.dll") -Force
}

Copy-Item (Join-Path $Root "LICENSE") $OutDir -ErrorAction SilentlyContinue
Copy-Item (Join-Path $Root "README.md") $OutDir -ErrorAction SilentlyContinue

# --- code signing ---
$TimestampUrl = if ($env:WINDOWS_TIMESTAMP_URL) { $env:WINDOWS_TIMESTAMP_URL } else { "http://timestamp.digicert.com" }
$ToSign = @(
    (Join-Path $OutDir "kalicut.exe"),
    (Join-Path $OutDir "ffmpeg.exe"),
    (Join-Path $OutDir "ffprobe.exe")
)
if ($MpvDll) { $ToSign += (Join-Path $OutDir "libmpv-2.dll") }

if ($env:WINDOWS_PFX_BASE64) {
    Write-Host "==> Authenticode sign (WINDOWS_PFX_BASE64)"
    if (-not $SkipMpv) { Enter-VsDevEnv }
    $pfxPath = Join-Path $env:TEMP "kalicut-codesign-$PID.pfx"
    try {
        [IO.File]::WriteAllBytes($pfxPath, [Convert]::FromBase64String($env:WINDOWS_PFX_BASE64))
        $pwd = if ($env:WINDOWS_PFX_PASSWORD) { $env:WINDOWS_PFX_PASSWORD } else { "" }
        Invoke-CodeSign -Paths $ToSign -PfxPath $pfxPath -Password $pwd -TimestampUrl $TimestampUrl
        $SignInfo = "Authenticode (PFX)"
    } finally {
        Remove-Item $pfxPath -Force -ErrorAction SilentlyContinue
    }
} elseif ($env:SIGN_MODE -eq "self") {
    Write-Host "==> Self-signed code signing (local test only — SmartScreen will still warn strangers)"
    if (-not $SkipMpv) { Enter-VsDevEnv }
    $cert = New-SelfSignedCertificate `
        -Type CodeSigningCert `
        -Subject "CN=KALICUT Test" `
        -CertStoreLocation "Cert:\CurrentUser\My" `
        -NotAfter (Get-Date).AddYears(1)
    $pfxPath = Join-Path $env:TEMP "kalicut-selfsign-$PID.pfx"
    $plain = [Guid]::NewGuid().ToString("N")
    $secure = ConvertTo-SecureString -String $plain -Force -AsPlainText
    try {
        Export-PfxCertificate -Cert $cert -FilePath $pfxPath -Password $secure | Out-Null
        Invoke-CodeSign -Paths $ToSign -PfxPath $pfxPath -Password $plain -TimestampUrl $TimestampUrl
        $SignInfo = "self-signed (test only)"
    } finally {
        Remove-Item $pfxPath -Force -ErrorAction SilentlyContinue
        Remove-Item "Cert:\CurrentUser\My\$($cert.Thumbprint)" -Force -ErrorAction SilentlyContinue
    }
} else {
    Write-Host "==> No code signing secrets — shipping unsigned (see docs/WINDOWS_SIGNING.md)"
    $SignInfo = "unsigned"
}

@"
@echo off
REM KALICUT portable launcher — ffmpeg + libmpv next to this script
set "PATH=%~dp0;%PATH%"
start "" "%~dp0kalicut.exe" %*
"@ | Set-Content -Path (Join-Path $OutDir "KALICUT.bat") -Encoding ASCII

$mpvLine = if ($MpvDll) {
    "  - libmpv-2.dll           (embedded video preview)"
} else {
    "  - (no libmpv; ffmpeg preview fallback)"
}

@"
KALICUT $Version for Windows x64
================================

Windows 10 / 11 (64-bit).

Self-contained:
  - kalicut.exe
  - ffmpeg.exe / ffprobe.exe  (cut, export)
$mpvLine

Run:
  Double-click  KALICUT.bat
  or            kalicut.exe

Signing: $SignInfo
If Windows SmartScreen blocks the download:
  More info → Run anyway
  (or install a trusted Authenticode cert — docs/WINDOWS_SIGNING.md)

Source: https://github.com/kaliblack256/kalicut
"@ | Set-Content -Path (Join-Path $OutDir "RUN.txt") -Encoding UTF8

@"
KALICUT Windows package
version: $Version
signing: $SignInfo
libmpv: $(if ($MpvDll) { 'bundled libmpv-2.dll' } else { 'disabled' })
"@ | Set-Content -Path (Join-Path $OutDir "SIGNING.txt") -Encoding UTF8

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
Write-Host "Signing: $SignInfo"
