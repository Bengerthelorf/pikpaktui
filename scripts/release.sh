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

CURRENT_BRANCH=$(git branch --show-current)
if [[ "$CURRENT_BRANCH" != "main" ]]; then
    error "Releases must be created from the main branch (current: ${CURRENT_BRANCH:-detached HEAD})."
fi

if [[ -n "$(git status --porcelain)" ]]; then
    error "Working directory is not clean. Commit, stash, or remove all tracked and untracked changes first."
fi

git remote get-url origin >/dev/null 2>&1 || error "Git remote 'origin' is not configured."
info "Refreshing origin/main and release tags..."
git fetch --quiet --tags origin "+refs/heads/main:refs/remotes/origin/main"

if [[ "$(git rev-parse HEAD)" != "$(git rev-parse refs/remotes/origin/main)" ]]; then
    error "Local main must exactly match origin/main before release preparation. Push or synchronize existing commits first."
fi

if git ls-remote --exit-code --tags origin "refs/tags/$TAG" >/dev/null 2>&1; then
    error "Tag $TAG already exists on origin."
fi

LOCAL_TAG_COMMIT=""
if git rev-parse --verify --quiet "refs/tags/$TAG^{commit}" >/dev/null; then
    LOCAL_TAG_COMMIT=$(git rev-parse "refs/tags/$TAG^{commit}")
    if [[ "$LOCAL_TAG_COMMIT" != "$(git rev-parse HEAD)" ]]; then
        error "Local tag $TAG exists but does not point to HEAD."
    fi
    warn "Local tag $TAG already points to HEAD; it will be reused."
fi

manifest_version() {
    awk -F '"' '/^version = "/ { print $2; exit }' Cargo.toml
}

lock_version() {
    awk -F '"' \
        '$0 == "name = \"pikpaktui\"" { getline; if ($0 ~ /^version = "/) { print $2; exit } }' \
        Cargo.lock
}

version_is_less() {
    local left_major left_minor left_patch
    local right_major right_minor right_patch
    IFS=. read -r left_major left_minor left_patch <<< "$1"
    IFS=. read -r right_major right_minor right_patch <<< "$2"

    if (( 10#$left_major != 10#$right_major )); then
        (( 10#$left_major < 10#$right_major ))
        return
    fi
    if (( 10#$left_minor != 10#$right_minor )); then
        (( 10#$left_minor < 10#$right_minor ))
        return
    fi
    (( 10#$left_patch < 10#$right_patch ))
}

CURRENT_VERSION=$(manifest_version)
[[ -n "$CURRENT_VERSION" ]] || error "Could not read the package version from Cargo.toml."
info "Current version: $CURRENT_VERSION → $VERSION"
if version_is_less "$VERSION" "$CURRENT_VERSION"; then
    error "Refusing to release version $VERSION because it is lower than current version $CURRENT_VERSION."
fi

# ==============================================================================
# Step 1: Bump version
# ==============================================================================

if [[ "$CURRENT_VERSION" != "$VERSION" ]]; then
    [[ -z "$LOCAL_TAG_COMMIT" ]] || error "Cannot bump Cargo.toml while local tag $TAG already exists."

    info "Bumping version in Cargo.toml..."
    CARGO_TMP=$(mktemp "$PROJECT_DIR/.Cargo.toml.XXXXXX")
    cleanup_release_tmp() {
        if [[ -n "${CARGO_TMP:-}" && -f "$CARGO_TMP" ]]; then
            rm -f -- "$CARGO_TMP"
        fi
    }
    trap cleanup_release_tmp EXIT

    if ! awk -v version="$VERSION" '
        BEGIN { replaced = 0 }
        !replaced && /^version = "/ {
            print "version = \"" version "\""
            replaced = 1
            next
        }
        { print }
        END { if (!replaced) exit 1 }
    ' Cargo.toml > "$CARGO_TMP"; then
        error "Could not update the package version in Cargo.toml."
    fi
    mv "$CARGO_TMP" Cargo.toml
    CARGO_TMP=""

    info "Synchronizing Cargo.lock..."
    cargo check --quiet
else
    info "Cargo.toml already has version $VERSION; no version bump commit is needed."
fi

if [[ "$(manifest_version)" != "$VERSION" ]]; then
    error "Cargo.toml version does not match requested release $VERSION."
fi
if [[ "$(lock_version)" != "$VERSION" ]]; then
    error "Cargo.lock version does not match requested release $VERSION."
fi

# ==============================================================================
# Step 2: Run tests
# ==============================================================================

info "Running tests..."
cargo test --quiet --locked

info "All tests passed."

# ==============================================================================
# Step 3: Commit, tag, and push
# ==============================================================================

if [[ "$CURRENT_VERSION" != "$VERSION" ]]; then
    CHANGED_FILES=$(git diff --name-only)
    EXPECTED_FILES=$'Cargo.lock\nCargo.toml'
    if [[ "$(printf '%s\n' "$CHANGED_FILES" | sort)" != "$EXPECTED_FILES" ]]; then
        error "Version preparation changed unexpected tracked files:\n$CHANGED_FILES"
    fi

    info "Committing version bump..."
    git add Cargo.toml Cargo.lock
    git commit -m "chore: bump version to $VERSION"
fi

if [[ -n "$(git status --porcelain)" ]]; then
    error "Working directory changed during release preparation; refusing to tag."
fi

if [[ -z "$LOCAL_TAG_COMMIT" ]]; then
    git tag "$TAG"
fi

info "Pushing main and $TAG atomically..."
git push --atomic origin "HEAD:refs/heads/main" "refs/tags/$TAG"

info "============================================"
info "Tag $TAG pushed!"
info "GitHub Actions will now build and publish the release."
info "Monitor at: https://github.com/Bengerthelorf/pikpaktui/actions"
info "============================================"
