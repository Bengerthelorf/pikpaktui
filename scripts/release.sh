#!/bin/bash
set -euo pipefail

# ==============================================================================
# pikpaktui Release Script
# Usage: ./scripts/release.sh <version>
# Example: ./scripts/release.sh 0.0.53
#
# This script bumps Cargo.toml, runs tests, commits, tags, and pushes.
# GitHub Actions handles the rest: build, release, crates.io, homebrew.
# ==============================================================================

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC} $1"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# ==============================================================================
# Validate arguments
# ==============================================================================

VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
    error "Usage: $0 <version>  (e.g. 0.0.53)"
fi

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    error "Version must be in format X.Y.Z (e.g. 0.0.53)"
fi

TAG="v$VERSION"

info "Preparing release: $VERSION (tag $TAG)"

# ==============================================================================
# Pre-flight checks
# ==============================================================================

cd "$PROJECT_DIR"

if ! git diff --quiet || ! git diff --cached --quiet; then
    error "Working directory has uncommitted changes. Commit or stash first."
fi

if git rev-parse "$TAG" >/dev/null 2>&1; then
    error "Tag $TAG already exists."
fi

CURRENT_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
info "Current version: $CURRENT_VERSION → $VERSION"

# ==============================================================================
# Step 1: Bump version
# ==============================================================================

info "Bumping version in Cargo.toml..."
sed -i '' "s/^version = \"$CURRENT_VERSION\"/version = \"$VERSION\"/" Cargo.toml

info "Updating Cargo.lock..."
cargo generate-lockfile --quiet

# ==============================================================================
# Step 2: Run tests
# ==============================================================================

info "Running tests..."
cargo test --quiet

info "All tests passed."

# ==============================================================================
# Step 3: Commit, tag, and push
# ==============================================================================

info "Committing version bump..."
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to $VERSION"

git tag "$TAG"

info "Pushing to remote..."
git push origin main
git push origin "$TAG"

info "============================================"
info "Tag $TAG pushed!"
info "GitHub Actions will now build and publish the release."
info "Monitor at: https://github.com/Bengerthelorf/pikpaktui/actions"
info "============================================"
