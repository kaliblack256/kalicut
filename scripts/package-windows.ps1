# Build self-contained Windows portable zip (x86_64 or aarch64).
# Run on matching Windows host (GitHub Actions windows-latest / windows-11-arm).
#
# Output: dist/kalicut-<ver>-windows-<arch>.zip
# Contents: kalicut.exe, libmpv-2.dll, ffmpeg.exe, ffprobe.exe, KALICUT.bat, docs
#
# Env:
#   KALICUT_WIN_ARCH     x86_64 | aarch64  (auto-detect if unset)
#   MPV_DEV_URL          override mpv-dev 7z URL
#   FFMPEG_ZIP_URL       override ffmpeg archive URL
#   KALICUT_SKIP_MPV=1   build without embedded-mpv
#   WINDOWS_PFX_BASE64 / WINDOWS_PFX_PASSWORD / WINDOWS_TIMESTAMP_URL
#   SIGN_MODE=self       ephemeral self-signed cert (local test only)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$Version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"').Matches.Groups[1].Value

function Resolve-WinArch {
    if ($env:KALICUT_WIN_ARCH) {
        $a = $env:KALICUT_WIN_ARCH.Trim().ToLower()
        switch -Regex ($a) {
            '^(x86_64|x64|amd64)$' { return "x86_64" }
            '^(aarch64|arm64)$' { return "aarch64" }
            default { throw "Unsupported KALICUT_WIN_ARCH=$a (use x86_64 or aarch64)" }
        }
    }
    $pa = $env:PROCESSOR_ARCHITECTURE
    if ($pa -eq "ARM64") { return "aarch64" }
    if ($pa -eq "AMD64") { return "x86_64" }
    $pa2 = $env:PROCESSOR_ARCHITEW6432
    if ($pa2 -eq "ARM64") { return "aarch64" }
    if ($pa2 -eq "AMD64") { return "x86_64" }
    return "x86_64"
}

$Arch = Resolve-WinArch
$RustTarget = if ($Arch -eq "aarch64") { "aarch64-pc-windows-msvc" } else { "x86_64-pc-windows-msvc" }
$LibMachine = if ($Arch -eq "aarch64") { "ARM64" } else { "x64" }
$Name = "kalicut-$Version-windows-$Arch"
$OutDir = Join-Path $Root "dist\$Name"
$ZipPath = Join-Path $Root "dist\$Name.zip"
$Cache = Join-Path $Root "dist\win-cache\$Arch"
New-Item -ItemType Directory -Force -Path $Cache | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Root "dist") | Out-Null

Write-Host "==> KALICUT Windows package $Version ($Arch / $RustTarget)"

# Pinned zhongfly release (reproducible). Override with MPV_DEV_URL / FFMPEG_ZIP_URL.
$MpvPinTag = "2026-07-23-1c4adc9819"
$MpvPinDate = "20260723"
$MpvPinHash = "1c4adc9819"
$FfmpegPinHash = "80eb9e99b"

function Get-7Zip {
    $cmd = Get-Command 7z -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    foreach ($p in @(
        "$env:ProgramFiles\7-Zip\7z.exe",
        "${env:ProgramFiles(x86)}\7-Zip\7z.exe"
    )) {
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
    param([string]$TargetArch = "x86_64")
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path $vswhere)) {
        throw "vswhere.exe not found — need Visual Studio Build Tools / MSVC"
    }
    $vsPath = & $vswhere -latest -products * -property installationPath
    if (-not $vsPath) { throw "Visual Studio installation not found" }

    if ($TargetArch -eq "aarch64") {
        $vcvars = Join-Path $vsPath "VC\Auxiliary\Build\vcvarsarm64.bat"
        if (-not (Test-Path $vcvars)) {
            $vcvarsAll = Join-Path $vsPath "VC\Auxiliary\Build\vcvarsall.bat"
            if (-not (Test-Path $vcvarsAll)) { throw "vcvarsall.bat / vcvarsarm64.bat missing" }
            $vcvarsCmd = "`"$vcvarsAll`" arm64"
        } else {
            $vcvarsCmd = "`"$vcvars`""
        }
    } else {
        $vcvars = Join-Path $vsPath "VC\Auxiliary\Build\vcvars64.bat"
        if (-not (Test-Path $vcvars)) {
            $vcvarsAll = Join-Path $vsPath "VC\Auxiliary\Build\vcvarsall.bat"
            if (-not (Test-Path $vcvarsAll)) { throw "vcvars64.bat / vcvarsall.bat missing" }
            $vcvarsCmd = "`"$vcvarsAll`" x64"
        } else {
            $vcvarsCmd = "`"$vcvars`""
        }
    }
    Write-Host "==> MSVC env: $vsPath ($TargetArch)"
    $tempBat = Join-Path $env:TEMP "kalicut-vcvars-$PID.bat"
    @"
@echo off
call $vcvarsCmd >nul
set
"@ | Set-Content -Path $tempBat -Encoding ASCII
    $vars = & cmd /c "`"$tempBat`""
    Remove-Item $tempBat -Force -ErrorAction SilentlyContinue
    foreach ($line in $vars) {
        $idx = $line.IndexOf('=')
        if ($idx -gt 0) {
            [Environment]::SetEnvironmentVariable($line.Substring(0, $idx), $line.Substring($idx + 1), "Process")
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
        [Parameter(Mandatory = $true)][string]$OutDir,
        [Parameter(Mandatory = $true)][string]$Machine
    )
    $dllName = Split-Path $DllPath -Leaf
    $defPath = Join-Path $OutDir "mpv.def"
    $libPath = Join-Path $OutDir "mpv.lib"
    $expPath = Join-Path $OutDir "mpv.exp"

    Write-Host "==> Creating import lib for $dllName (machine=$Machine)"
    $raw = & dumpbin /EXPORTS $DllPath 2>&1 | Out-String
    $names = [System.Collections.Generic.List[string]]::new()
    foreach ($line in ($raw -split "`r?`n")) {
        if ($line -match '^\s+\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]+\s+(\S+)\s*$') {
            $sym = $Matches[1]
            if ($sym -and $sym -ne "name") { [void]$names.Add($sym) }
        }
    }
    if ($names.Count -lt 10) {
        throw "Too few exports from $DllPath ($($names.Count)). dumpbin may have failed."
    }
    $def = New-Object System.Text.StringBuilder
    [void]$def.AppendLine("LIBRARY $dllName")
    [void]$def.AppendLine("EXPORTS")
    foreach ($n in $names) { [void]$def.AppendLine("    $n") }
    [System.IO.File]::WriteAllText($defPath, $def.ToString())

    & lib.exe /nologo /def:$defPath /out:$libPath /machine:$Machine
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
        $sdkRoot = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
        if (Test-Path $sdkRoot) {
            $found = Get-ChildItem -Path $sdkRoot -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -match '\\(x64|arm64)\\' } |
                Select-Object -First 1
            if ($found) { $signtool = $found }
        }
    }
    if (-not $signtool) { throw "signtool.exe not found (install Windows SDK)" }
    $st = if ($signtool.Source) { $signtool.Source } else { $signtool.FullName }
    foreach ($p in $Paths) {
        if (-not (Test-Path $p)) { continue }
        Write-Host "    signtool: $(Split-Path $p -Leaf)"
        $args = @("sign", "/fd", "SHA256", "/td", "SHA256", "/tr", $TimestampUrl, "/f", $PfxPath)
        if ($Password) { $args += @("/p", $Password) }
        $args += $p
        & $st @args
        if ($LASTEXITCODE -ne 0) { throw "signtool failed for $p" }
    }
}

function Invoke-Download {
    param(
        [Parameter(Mandatory = $true)][string]$Uri,
        [Parameter(Mandatory = $true)][string]$OutFile,
        [int]$Retries = 4
    )
    $dir = Split-Path $OutFile -Parent
    if ($dir) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
    for ($i = 1; $i -le $Retries; $i++) {
        try {
            Write-Host "    try $i/$Retries : $Uri"
            if (Test-Path $OutFile) { Remove-Item -Force $OutFile }
            $curl = Get-Command curl.exe -ErrorAction SilentlyContinue
            if ($curl) {
                & curl.exe -fsSL --retry 3 --retry-delay 2 -o $OutFile $Uri
                if ($LASTEXITCODE -eq 0 -and (Test-Path $OutFile) -and ((Get-Item $OutFile).Length -gt 1MB)) {
                    return
                }
            } else {
                Invoke-WebRequest -Uri $Uri -OutFile $OutFile -UseBasicParsing
                if ((Test-Path $OutFile) -and ((Get-Item $OutFile).Length -gt 1MB)) { return }
            }
        } catch {
            Write-Host "    download error: $($_.Exception.Message)"
        }
        Start-Sleep -Seconds (3 * $i)
    }
    throw "Failed to download: $Uri"
}

rustup target add $RustTarget 2>$null | Out-Null

# --- ffmpeg ---
$FfmpegExtract = Join-Path $Cache "ffmpeg"
$FfmpegBin = $null
$FfprobeBin = $null
if (Test-Path $FfmpegExtract) {
    $FfmpegBin = Get-ChildItem -Path $FfmpegExtract -Recurse -Filter ffmpeg.exe -ErrorAction SilentlyContinue | Select-Object -First 1
    $FfprobeBin = Get-ChildItem -Path $FfmpegExtract -Recurse -Filter ffprobe.exe -ErrorAction SilentlyContinue | Select-Object -First 1
}
if (-not $FfmpegBin -or -not $FfprobeBin) {
    Write-Host "==> Downloading ffmpeg ($Arch)"
    if (Test-Path $FfmpegExtract) { Remove-Item -Recurse -Force $FfmpegExtract }
    New-Item -ItemType Directory -Force -Path $FfmpegExtract | Out-Null

    $candidates = @()
    if ($env:FFMPEG_ZIP_URL) { $candidates += $env:FFMPEG_ZIP_URL }
    if ($Arch -eq "aarch64") {
        $candidates += @(
            "https://github.com/zhongfly/mpv-winbuild/releases/download/$MpvPinTag/ffmpeg-aarch64-git-$FfmpegPinHash.7z"
        )
    } else {
        $candidates += @(
            "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip",
            "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
            "https://github.com/zhongfly/mpv-winbuild/releases/download/$MpvPinTag/ffmpeg-x86_64-git-$FfmpegPinHash.7z"
        )
    }

    $ok = $false
    foreach ($url in $candidates) {
        $ext = if ($url -match '\.7z(\?|$)') { "7z" } else { "zip" }
        $archive = Join-Path $Cache "ffmpeg-download.$ext"
        try {
            Invoke-Download -Uri $url -OutFile $archive
            if ($ext -eq "7z") {
                $seven = Get-7Zip
                & $seven x "-o$FfmpegExtract" "-y" $archive
                if ($LASTEXITCODE -ne 0) { throw "7z extract failed" }
            } else {
                Expand-Archive -Path $archive -DestinationPath $FfmpegExtract -Force
            }
            $FfmpegBin = Get-ChildItem -Path $FfmpegExtract -Recurse -Filter ffmpeg.exe | Select-Object -First 1
            $FfprobeBin = Get-ChildItem -Path $FfmpegExtract -Recurse -Filter ffprobe.exe | Select-Object -First 1
            if ($FfmpegBin -and $FfprobeBin) {
                Write-Host "    using mirror: $url"
                $ok = $true
                break
            }
            throw "archive has no ffmpeg.exe"
        } catch {
            Write-Host "    mirror failed: $($_.Exception.Message)"
            if (Test-Path $FfmpegExtract) { Remove-Item -Recurse -Force $FfmpegExtract }
            New-Item -ItemType Directory -Force -Path $FfmpegExtract | Out-Null
        }
    }
    if (-not $ok) { throw "Could not download ffmpeg for $Arch from any mirror" }
}
Write-Host "ffmpeg: $($FfmpegBin.FullName)"

# --- libmpv ---
$SkipMpv = ($env:KALICUT_SKIP_MPV -eq "1")
$MpvDll = $null
$SignInfo = "unsigned"

if (-not $SkipMpv) {
    Enter-VsDevEnv -TargetArch $Arch

    $MpvDevUrl = $env:MPV_DEV_URL
    if (-not $MpvDevUrl) {
        $MpvDevUrl = "https://github.com/zhongfly/mpv-winbuild/releases/download/$MpvPinTag/mpv-dev-$Arch-$MpvPinDate-git-$MpvPinHash.7z"
    }
    $MpvArchive = Join-Path $Cache "mpv-dev.7z"
    $MpvRoot = Join-Path $Cache "mpv-dev"
    if (-not (Test-Path $MpvArchive)) {
        Write-Host "==> Downloading mpv-dev ($Arch)"
        Invoke-Download -Uri $MpvDevUrl -OutFile $MpvArchive
    }
    if (-not (Test-Path (Join-Path $MpvRoot "libmpv-2.dll"))) {
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

    New-MpvImportLibrary -DllPath $MpvDll.FullName -OutDir $MpvLibDir -Machine $LibMachine | Out-Null

    $env:KALICUT_MPV_DIR = $MpvLibDir
    if ($env:RUSTFLAGS) {
        $env:RUSTFLAGS = "$($env:RUSTFLAGS) -L native=$MpvLibDir"
    } else {
        $env:RUSTFLAGS = "-L native=$MpvLibDir"
    }
    if ($env:LIB) { $env:LIB = "$MpvLibDir;$env:LIB" } else { $env:LIB = $MpvLibDir }

    Write-Host "==> cargo build --release --target $RustTarget (embedded-mpv)"
    $env:CARGO_TERM_COLOR = "always"
    cargo build --release --target $RustTarget --bin kalicut
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
} else {
    Write-Host "==> cargo build --release --target $RustTarget --no-default-features"
    $env:CARGO_TERM_COLOR = "always"
    cargo build --release --target $RustTarget --no-default-features --bin kalicut
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

$Exe = Join-Path $Root "target\$RustTarget\release\kalicut.exe"
if (-not (Test-Path $Exe)) {
    $Exe = Join-Path $Root "target\release\kalicut.exe"
}
if (-not (Test-Path $Exe)) { throw "kalicut.exe missing" }

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

$TimestampUrl = if ($env:WINDOWS_TIMESTAMP_URL) { $env:WINDOWS_TIMESTAMP_URL } else { "http://timestamp.digicert.com" }
$ToSign = @(
    (Join-Path $OutDir "kalicut.exe"),
    (Join-Path $OutDir "ffmpeg.exe"),
    (Join-Path $OutDir "ffprobe.exe")
)
if ($MpvDll) { $ToSign += (Join-Path $OutDir "libmpv-2.dll") }

if ($env:WINDOWS_PFX_BASE64) {
    Write-Host "==> Authenticode sign (WINDOWS_PFX_BASE64)"
    if (-not $SkipMpv) { Enter-VsDevEnv -TargetArch $Arch }
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
    Write-Host "==> Self-signed code signing (local test only)"
    if (-not $SkipMpv) { Enter-VsDevEnv -TargetArch $Arch }
    $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=KALICUT Test" `
        -CertStoreLocation "Cert:\CurrentUser\My" -NotAfter (Get-Date).AddYears(1)
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
    Write-Host "==> No code signing secrets — shipping unsigned (docs/WINDOWS_SIGNING.md)"
    $SignInfo = "unsigned"
}

@"
@echo off
REM KALICUT portable launcher — ffmpeg + libmpv next to this script
set "PATH=%~dp0;%PATH%"
start "" "%~dp0kalicut.exe" %*
"@ | Set-Content -Path (Join-Path $OutDir "KALICUT.bat") -Encoding ASCII

$mpvLine = if ($MpvDll) { "  - libmpv-2.dll           (embedded video preview)" } else { "  - (no libmpv; ffmpeg preview fallback)" }
$archLabel = if ($Arch -eq "aarch64") { "Windows on ARM (arm64)" } else { "Windows 10 / 11 (x64)" }

@"
KALICUT $Version for Windows ($Arch)
====================================

$archLabel

Self-contained:
  - kalicut.exe
  - ffmpeg.exe / ffprobe.exe  (cut, export)
$mpvLine

Run:
  Double-click  KALICUT.bat
  or            kalicut.exe

Signing: $SignInfo
If Windows SmartScreen blocks:
  More info → Run anyway
  (docs/WINDOWS_SIGNING.md)

Source: https://github.com/kaliblack256/kalicut
"@ | Set-Content -Path (Join-Path $OutDir "RUN.txt") -Encoding UTF8

@"
KALICUT Windows package
version: $Version
arch: $Arch
signing: $SignInfo
libmpv: $(if ($MpvDll) { 'bundled libmpv-2.dll' } else { 'disabled' })
"@ | Set-Content -Path (Join-Path $OutDir "SIGNING.txt") -Encoding UTF8

if (Test-Path $ZipPath) { Remove-Item -Force $ZipPath }
Write-Host "==> Compress-Archive $ZipPath"
Compress-Archive -Path (Join-Path $OutDir "*") -DestinationPath $ZipPath -Force

$sha = (Get-FileHash -Algorithm SHA256 $ZipPath).Hash.ToLower()
"$sha  $(Split-Path $ZipPath -Leaf)" | Set-Content -Path "$ZipPath.sha256" -Encoding ASCII

Write-Host "Created: $ZipPath"
Get-Item $ZipPath | Format-List Name, Length
Write-Host "SHA256: $sha"
Write-Host "Signing: $SignInfo"
Write-Host "Arch: $Arch"
