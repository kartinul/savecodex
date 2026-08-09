#!/usr/bin/env bash
set -e

if [ -z "$1" ]; then
  echo "Usage: $0 <new_version>"
  echo "Example: $0 1.0.2"
  exit 1
fi

NEW_VERSION=$1

echo "📦 Updating Cargo.toml version to $NEW_VERSION..."
# Update version in [workspace.package] section
awk -v ver="$NEW_VERSION" '
  /^\[workspace\.package\]/ { in_section=1; print; next }
  /^\[/ && in_section { in_section=0 }
  in_section && /^[[:space:]]*version[[:space:]]*=/{ sub(/"[^"]*"/, "\""ver"\""); print; next }
  { print }
' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml

echo "📦 Updating frontend/package.json version to $NEW_VERSION..."
if command -v jq >/dev/null 2>&1; then
  jq ".version = \"$NEW_VERSION\"" frontend/package.json > frontend/package.json.tmp && mv frontend/package.json.tmp frontend/package.json
else
  sed -i '' "s/\(\"version\"\s*:\s*\"\)[^\"]*\"/\1$NEW_VERSION\"/" frontend/package.json
fi

echo "🔒 Updating Cargo.lock..."
cargo check > /dev/null 2>&1

echo "🏷️ Committing and tagging v$NEW_VERSION..."
git add Cargo.toml Cargo.lock frontend/package.json
if ! git diff --cached --quiet; then
  git commit -m "Bump version to v$NEW_VERSION"
fi
git tag -a "v$NEW_VERSION" -m "v$NEW_VERSION"

echo "✅ Done! Run 'git push --follow-tags' to push the release."
