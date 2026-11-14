#!/usr/bin/env bash
# Sign a macOS vigent with the Developer ID and notarize it, when the Apple
# secrets are in the environment. Without them it leaves the ad-hoc signature
# the linker gave the binary (Apple silicon won't run an unsigned one; a binary
# with none gets one), and says so. The Binaries workflow runs it in the darwin jobs, before the binary
# is packed:
#
#   scripts/sign-macos.sh dist-bin/vigent-darwin-arm64/vigent
#
#   APPLE_CERTIFICATE_P12        the Developer ID Application certificate and its key, a .p12, base64
#   APPLE_CERTIFICATE_PASSWORD   the .p12's password
#   APPLE_SIGNING_IDENTITY       e.g. "Developer ID Application: Farming Labs (TEAMID1234)"
#   APPLE_ID                     the Apple ID that notarizes
#   APPLE_TEAM_ID                its team ID
#   APPLE_APP_PASSWORD           an app-specific password for that Apple ID
#
# The certificate goes into a temporary keychain that's deleted afterwards. A
# bare command-line binary can't have its notarization ticket stapled to it
# (stapler only staples apps, packages and disk images), so Gatekeeper looks
# the ticket up online the first time a downloaded copy runs.
set -euo pipefail

bin="${1:?usage: scripts/sign-macos.sh <binary>}"
[[ -f "$bin" ]] || { echo "sign-macos: no $bin" >&2; exit 1; }

secrets=(APPLE_CERTIFICATE_P12 APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_ID APPLE_TEAM_ID APPLE_APP_PASSWORD)
missing=()
for name in "${secrets[@]}"; do [[ -n "${!name:-}" ]] || missing+=("$name"); done

if (( ${#missing[@]} == ${#secrets[@]} )); then
	echo "::notice title=macOS binary not notarized::$bin keeps its ad-hoc signature: the Apple signing secrets aren't set (docs/binaries.md, Signing for macOS). Downloaded in a browser, it needs: xattr -d com.apple.quarantine <path>"
	# arm64 links come ad-hoc signed; an x86_64 one may not, so it gets the same
	codesign --verify --strict "$bin" 2>/dev/null || codesign --force --sign - "$bin"
	codesign --verify --strict "$bin"
	codesign --display --verbose=2 "$bin" 2>&1 | grep -E '^(Signature|CodeDirectory)' || true
	exit 0
fi
if (( ${#missing[@]} > 0 )); then
	# some set and some not is a mistake, not a choice: don't quietly ship it unsigned
	echo "::error title=Apple signing secrets incomplete::missing ${missing[*]}" >&2
	exit 1
fi

tmp="$(mktemp -d)"
keychain="$tmp/vigent-signing.keychain-db"
cleanup() {
	security delete-keychain "$keychain" 2>/dev/null || true
	rm -rf "$tmp"
}
trap cleanup EXIT

# a keychain of its own, unlocked for this job, that codesign may use without asking
keychain_password="$(openssl rand -hex 24)"
printf '%s' "$APPLE_CERTIFICATE_P12" | base64 --decode > "$tmp/certificate.p12"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 3600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$tmp/certificate.p12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" -f pkcs12 -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$keychain" > /dev/null
# searched first, beside the keychains already there (for the certificate's chain)
existing=()
while IFS= read -r k; do k="${k#"${k%%[![:space:]]*}"}"; existing+=("${k//\"/}"); done < <(security list-keychains -d user)
security list-keychains -d user -s "$keychain" "${existing[@]}"

# the hardened runtime and a secure timestamp: both are required for notarization
codesign --force --options runtime --timestamp --keychain "$keychain" --sign "$APPLE_SIGNING_IDENTITY" "$bin"
codesign --verify --strict --verbose=2 "$bin"
codesign --display --verbose=2 "$bin" 2>&1 | grep -E '^(Authority|TeamIdentifier|Timestamp|CodeDirectory)'

# notarytool takes a zip, a package or a disk image, not a bare binary
ditto -c -k --keepParent "$bin" "$tmp/vigent.zip"
xcrun notarytool submit "$tmp/vigent.zip" \
	--apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" \
	--wait --timeout 30m --output-format json > "$tmp/notary.json" || true
status="$(plutil -extract status raw -o - "$tmp/notary.json" 2>/dev/null || echo unknown)"
id="$(plutil -extract id raw -o - "$tmp/notary.json" 2>/dev/null || echo "")"
echo "notarization: ${status} ${id}"
if [[ "$status" != "Accepted" ]]; then
	cat "$tmp/notary.json" >&2 || true
	if [[ -n "$id" ]]; then
		xcrun notarytool log "$id" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" >&2 || true
	fi
	echo "::error title=Notarization failed::$bin: $status" >&2
	exit 1
fi

# Gatekeeper's verdict, as for a downloaded file (online: the ticket isn't stapled).
# spctl's execute check only rates apps, so a bare binary is assessed as opened.
if spctl --assess --type open --context context:primary-signature --verbose=2 "$bin" 2>&1 | tee "$tmp/spctl.txt"; then
	grep -q "Notarized Developer ID" "$tmp/spctl.txt" || echo "::warning title=Gatekeeper::$bin is accepted, but not as notarized yet: $(cat "$tmp/spctl.txt")"
else
	echo "::warning title=Gatekeeper::spctl didn't accept $bin: $(cat "$tmp/spctl.txt")"
fi
echo "signed and notarized: $bin"
