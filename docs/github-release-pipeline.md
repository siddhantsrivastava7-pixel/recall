# GitHub Release Pipeline

Recall includes a local GitHub CLI release script:

```powershell
npm.cmd run release:github
```

The script:

1. Initializes Git if needed.
2. Ensures the GitHub repo exists.
3. Pushes the current code to `main`.
4. Reads the app version from `src-tauri/tauri.conf.json`.
5. Builds the signed Tauri app.
6. Uploads the `.msi` and `.sig` files to a GitHub release.
7. Writes `updates/latest.json` with the required Tauri static manifest shape.
8. Commits and pushes the updated manifest.

## Signing Requirements

Because `bundle.createUpdaterArtifacts` is enabled, release builds need the updater private key:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw "..\recall-updater-private.key"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "your-key-password"
npm.cmd run release:github
```

You can also pass a key path:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "your-key-password"
powershell -ExecutionPolicy Bypass -File scripts/release-github.ps1 -SigningPrivateKeyPath "..\recall-updater-private.key"
```

Do not commit the private key. The repository `.gitignore` excludes `*.key` and `*.key.pub`.

## Updater Manifest

The script writes:

```json
{
  "version": "0.1.0",
  "notes": "Release v0.1.0",
  "pub_date": "2026-04-10T12:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "INLINE_SIGNATURE_CONTENT",
      "url": "https://github.com/OWNER/recall/releases/download/v0.1.0/Recall_0.1.0_x64_en-US.msi"
    }
  }
}
```

The app currently checks:

```text
https://raw.githubusercontent.com/siddhantsrivastava7-pixel/recall/main/updates/latest.json
```

When releasing under another owner or repo name, update the updater endpoint in `src-tauri/tauri.conf.json` before shipping.
