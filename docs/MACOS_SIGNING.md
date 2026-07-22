# macOS signing for KALICUT

There are **several** practical ways to “sign”. They are **not** equal.

## Comparison

| Mode | Cost | Needs Apple account | Gatekeeper for *other* users |
|------|------|---------------------|------------------------------|
| **1. Ad-hoc** `codesign -s -` | Free | No | Still often **blocked** after download (quarantine) |
| **2. Self-signed cert** in Keychain | Free | No | Others must **trust your cert** — unrealistic for public release |
| **3. Developer ID + notarize** | ~$99/year | **Yes** (paid program) | **Works** — “Open” without fighting Security |

Default CI/package uses **(1) ad-hoc**. That is still better than a completely unsigned binary (install_name_tool + codesign stay consistent).

---

## 1. Ad-hoc (default, free)

```bash
SIGN_MODE=adhoc ./scripts/package-macos.sh
# or just:
./scripts/package-macos.sh
```

What it does:

- `codesign --sign -` on `bin/*`, `lib/*.dylib`, launcher
- No Apple login
- Local “damaged app” issues after `install_name_tool` often go away
- After download from the internet, users may still need:

```bash
xattr -dr com.apple.quarantine .
# or: System Settings → Privacy & Security → Open Anyway
```

---

## 2. Self-signed certificate (free, for your own Mac)

On a Mac:

```bash
# Create a Code Signing identity in Keychain Access, or:
security create-keychain -p "" build.keychain  # example only
# Better UI: Keychain Access → Certificate Assistant → Create a Certificate
#   Type: Code Signing
```

Then:

```bash
security find-identity -v -p codesigning
export SIGN_MODE=identity
export CODESIGN_IDENTITY="Your Self-Signed Name"
./scripts/package-macos.sh
```

Useful for **you** testing. **Not** for strangers downloading from GitHub.

---

## 3. Apple Developer ID + notarization (real distribution)

1. Join [Apple Developer Program](https://developer.apple.com/programs/) (~$99/year).
2. Create **Developer ID Application** certificate.
3. Export as `.p12` (with password).
4. Create an [app-specific password](https://appleid.apple.com) for notarytool  
   (or App Store Connect API key).

### Local build

```bash
export SIGN_MODE=identity
export CODESIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)"
export NOTARIZE=1
export APPLE_ID="you@example.com"
export APPLE_TEAM_ID="TEAMID"
export APPLE_APP_PASSWORD="xxxx-xxxx-xxxx-xxxx"
./scripts/package-macos.sh
```

### GitHub Actions secrets

| Secret | Purpose |
|--------|---------|
| `MACOS_CERTIFICATE_P12` | base64 of `.p12` file |
| `MACOS_CERTIFICATE_PASSWORD` | password for `.p12` |
| `MACOS_CODESIGN_IDENTITY` | e.g. `Developer ID Application: … (TEAMID)` |
| `APPLE_ID` | Apple ID email (if using app password) |
| `APPLE_TEAM_ID` | 10-char Team ID |
| `APPLE_APP_PASSWORD` | app-specific password |
| `NOTARIZE` | set to `1` to enable notarization step |

Workflow imports the cert into a temporary keychain, then runs:

```bash
SIGN_MODE=identity CODESIGN_IDENTITY="…" ./scripts/package-macos.sh
```

If secrets are **missing**, CI falls back to **ad-hoc** automatically.

---

## 4. Not signing, only remove quarantine (not a signature)

```bash
xattr -dr com.apple.quarantine /path/to/kalicut-folder
```

This is what many open-source projects document for unsigned builds.  
It is **not** signing; it only clears the “downloaded from the internet” flag **on that machine**.

---

## What we ship by default

- **Ad-hoc signed** portable `kalicut-*-macos-arm64.tar.gz` from CI  
- `SIGNING.txt` inside the archive describes the mode used  
- Full Developer ID when you add secrets (no code change required beyond secrets)

## Recommended path for a public GitHub app

1. Keep ad-hoc for free CI (done).  
2. When ready for “clean open on any Mac”: pay Developer Program → add secrets → re-run **macOS arm64** workflow / next tag.
