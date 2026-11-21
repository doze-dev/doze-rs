#!/bin/sh
# dmg.sh <dir-with-Doze.app> <version>
#
# Wraps a built, signed Doze.app (`cargo xtask app-bundle`) in <dir>/Doze-<version>.dmg: the app and a link to
# /Applications, so installing is a drag. With DOZE_SIGN_ID the image is signed too, and with NOTARY_KEY_PATH,
# NOTARY_KEY_ID and NOTARY_ISSUER_ID it is notarised and stapled, so Gatekeeper opens it without a word. Without them
# it is an unsigned image, fit for a dry run only. (Go's build/darwin/dmg.sh, unchanged in what it does.)
set -eu
DIR=$1
VERSION=$2
APP="$DIR/Doze.app"
DMG="$DIR/Doze-$VERSION.dmg"
[ -d "$APP" ] || { echo "dmg.sh: no $APP; run cargo xtask app-bundle first" >&2; exit 1; }

STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
# ditto keeps the signature's extended attributes and the bundle exactly as signed.
ditto "$APP" "$STAGE/Doze.app"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
hdiutil create -quiet -volname "doze" -srcfolder "$STAGE" -fs HFS+ -format ULFO -ov "$DMG"

if [ -n "${DOZE_SIGN_ID:-}" ]; then
  codesign --force --timestamp --sign "$DOZE_SIGN_ID" "$DMG"
  if [ -n "${NOTARY_KEY_PATH:-}" ] && [ -n "${NOTARY_KEY_ID:-}" ] && [ -n "${NOTARY_ISSUER_ID:-}" ]; then
    xcrun notarytool submit "$DMG" --wait \
      --key "$NOTARY_KEY_PATH" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER_ID"
    xcrun stapler staple "$DMG"
  fi
fi
hdiutil verify -quiet "$DMG"
echo "$DMG"
