#!/usr/bin/env bash
set -euo pipefail

# Use given string or fall back to `git describe --tags --long`
DESC="${1:-$(git describe --tags --long)}"

# Extract parts: <tag>-<commits>-g<sha>
COMMITS="${DESC%-g*}"
COMMITS="${COMMITS##*-}"     # number between last '-' and '-g'
TAG="${DESC%-${COMMITS}-g*}" # everything before "-<commits>-g<sha>"

# Parse tag: optional 'v', MAJOR.MINOR.PATCH and optional -prerelease
if [[ "$TAG" =~ ^(v)?([0-9]+)\.([0-9]+)\.([0-9]+)(-[0-9A-Za-z\.-]+)?$ ]]; then
  V="${BASH_REMATCH[1]}"
  MAJ="${BASH_REMATCH[2]}"
  MIN="${BASH_REMATCH[3]}"
  PAT="${BASH_REMATCH[4]}"
  PRE="${BASH_REMATCH[5]:-}"
else
  echo "Error: tag '$TAG' is not semver-like (vMAJOR.MINOR.PATCH[-prerelease])." >&2
  exit 1
fi

# Bump patch by the commit count
NEW_PATCH=$((PAT + COMMITS))

# Output new version
VERSION="${V}${MAJ}.${MIN}.${NEW_PATCH}${PRE}"

if [ -n "$PRE" ]; then
  PRERELEASE="true"
else
  PRERELEASE="false"
fi
case $PRE in
  "-alpha")
    RELEASE_NAME="Alpha $VERSION"
    CHANNEL="alpha"
    ;;
  "-beta")
    RELEASE_NAME="Beta $VERSION"
    CHANNEL="beta"
    ;;
  *)
    RELEASE_NAME="$VERSION"
    CHANNEL="stable"
esac
art="${ARTIFACT_BASENAME}"


echo "version=$VERSION"
echo "prerelease=$PRERELEASE"
echo "tag=$VERSION"
echo "name=$RELEASE_NAME"
echo "channel=$CHANNEL"
echo "artifact_basename=$art"
