# Releasing PhraseXp

Tag a release and CI does the rest:

```sh
# 1. Bump the version in desktop/src-tauri/tauri.conf.json and desktop/src-tauri/Cargo.toml
#    (npm's package.json is dev-only; bundles read tauri.conf.json).
# 2. Commit, tag, push:
git tag v1.4.2 && git push origin main --tags
```

The `release` workflow builds deb/appimage (Linux), msi/nsis (Windows), and
app/dmg (macOS) on all three runners and opens a **draft** GitHub release with
the artifacts attached. Review, then publish.

## Signing (optional but recommended for distribution)

Without signing everything still builds — users just see OS warnings.

### macOS — Developer ID + notarization

One-time: join the Apple Developer Program, create a **Developer ID Application**
certificate, and export it as a `.p12`.

Repo secrets:

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | base64 of the exported `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | the `.p12` password |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID` | Apple ID email for notarization |
| `APPLE_PASSWORD` | app-specific password (appleid.apple.com) |
| `APPLE_TEAM_ID` | 10-char team id |

Tauri signs and staples the notarization ticket automatically when these are set.

### Windows — Authenticode

Get an OV/EV code-signing certificate. Set the certificate's
`certificateThumbprint` under `bundle.windows` in `tauri.conf.json`, and provide
`CSC_LINK` / `CSC_KEY_PASSWORD` repo secrets (a `.pfx` and its password). EV
certificates live on hardware tokens and need a signing service instead — out of
scope here.

### Linux

No signing infrastructure is wired (deb repositories would need a GPG key and a
hosted apt repo). The deb and AppImage are unsigned, like most non-store Linux apps.

## Notes

- macOS builds are **not Mac App Store eligible** — global event taps are banned by
  the sandbox. Notarized direct download only.
- Unsigned macOS builds: right-click → Open on first launch (Gatekeeper), and the
  user must grant Accessibility + Input Monitoring (see `platform.rs`).
- Unsigned Windows builds trigger SmartScreen; signed OV certs lose that warning
  once reputation builds up.
