# Windows code signing for KALICUT

| Mode | Cost | SmartScreen for strangers |
|------|------|---------------------------|
| **Unsigned** (default CI) | Free | Often warns → *More info → Run anyway* |
| **Self-signed** `SIGN_MODE=self` | Free | Still warns — only for your own test PCs |
| **Authenticode PFX** (OV/EV cert) | Paid (~$100–400/yr or Azure Trusted Signing) | **Much better**; EV establishes reputation faster |

Default CI ships **unsigned** unless repository secrets are set.

---

## 1. Unsigned (default)

```powershell
./scripts/package-windows.ps1
```

Users may see SmartScreen. Documented in `RUN.txt` inside the zip.

---

## 2. Self-signed (local smoke only)

Requires Windows + Windows SDK (`signtool`):

```powershell
$env:SIGN_MODE = "self"
./scripts/package-windows.ps1
```

Creates a temporary code-signing cert, signs `kalicut.exe`, `ffmpeg.exe`, `ffprobe.exe`, and `libmpv-2.dll`, then discards the cert.

**Not** suitable for public GitHub downloads.

---

## 3. Real Authenticode (recommended for public Windows)

1. Buy a **Code Signing** certificate (OV or EV) from a public CA, **or** use [Azure Trusted Signing](https://learn.microsoft.com/en-us/azure/trusted-signing/).
2. Export a `.pfx` (PKCS#12) with private key + password.
3. Base64-encode the file:

```bash
# Linux/macOS
base64 -w0 your-cert.pfx > pfx.b64
```

```powershell
# Windows
[Convert]::ToBase64String([IO.File]::ReadAllBytes("your-cert.pfx")) | Set-Content pfx.b64
```

4. Add GitHub Actions **secrets**:

| Secret | Purpose |
|--------|---------|
| `WINDOWS_PFX_BASE64` | entire `.pfx` as base64 |
| `WINDOWS_PFX_PASSWORD` | PFX password |
| `WINDOWS_TIMESTAMP_URL` | optional; default `http://timestamp.digicert.com` |

5. Re-run workflow **Windows** or push a new tag.

`package-windows.ps1` and `.github/workflows/windows.yml` pick up the secrets automatically and run `signtool sign /fd SHA256` with an RFC3161 timestamp.

### Local Authenticode

```powershell
$env:WINDOWS_PFX_BASE64 = Get-Content -Raw pfx.b64
$env:WINDOWS_PFX_PASSWORD = "your-password"
./scripts/package-windows.ps1
```

---

## Architectures

| Zip | Runner / machine |
|-----|------------------|
| `kalicut-*-windows-x86_64.zip` | Win10/11 x64 · CI `windows-latest` |
| `kalicut-*-windows-aarch64.zip` | Windows on ARM · CI `windows-11-arm` |

Override arch when packaging:

```powershell
$env:KALICUT_WIN_ARCH = "aarch64"   # or x86_64
./scripts/package-windows.ps1
```

Both arches use the same Authenticode secrets when set.

## Package contents & libmpv

The Windows zip is self-contained:

| File | Role |
|------|------|
| `kalicut.exe` | GUI (built **with** `embedded-mpv`) |
| `libmpv-2.dll` | Video preview (shinchiro/zhongfly mpv-dev) |
| `ffmpeg.exe` / `ffprobe.exe` | Cut / export / metadata |
| `KALICUT.bat` | Sets `PATH` to the folder, launches the app |

Emergency build **without** libmpv:

```powershell
$env:KALICUT_SKIP_MPV = "1"
./scripts/package-windows.ps1
```

---

## What we ship by default

- **libmpv embedded** + ffmpeg/ffprobe
- **Unsigned** unless `WINDOWS_PFX_*` secrets exist
- `SIGNING.txt` inside the archive records the mode used

## Recommended path for a public Windows build

1. Keep unsigned CI until you have a cert (works today).
2. Add OV/EV or Azure Trusted Signing → set secrets → rebuild / next tag.
3. Optional: submit the binary to Microsoft SmartScreen / build download reputation over time.
