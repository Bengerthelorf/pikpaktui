#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}>>> pikpaktui Installer${NC}"

# Detect OS and Architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

echo -e "Detected OS: ${GREEN}${OS}${NC}"
echo -e "Detected Arch: ${GREEN}${ARCH}${NC}"

ASSET_NAME=""

if [ "$OS" = "Linux" ]; then
    if [ "$ARCH" = "x86_64" ]; then
        ASSET_NAME="pikpaktui-x86_64-linux.tar.gz"
    elif [ "$ARCH" = "aarch64" ] || [ "$ARCH" = "arm64" ]; then
        ASSET_NAME="pikpaktui-aarch64-linux.tar.gz"
    else
        echo -e "${RED}Error: Unsupported Linux architecture: ${ARCH}${NC}"
        echo "Currently supported: x86_64, aarch64"
        exit 1
    fi
    INSTALL_DIR="/usr/bin"
elif [ "$OS" = "Darwin" ]; then
    if [ "$ARCH" = "x86_64" ]; then
        ASSET_NAME="pikpaktui-x86_64-macos.tar.gz"
    elif [ "$ARCH" = "arm64" ]; then
        ASSET_NAME="pikpaktui-aarch64-macos.tar.gz"
    else
        echo -e "${RED}Error: Unsupported macOS architecture: ${ARCH}${NC}"
        exit 1
    fi
    INSTALL_DIR="/usr/local/bin"
elif [ "$OS" = "FreeBSD" ]; then
    if [ "$ARCH" = "amd64" ] || [ "$ARCH" = "x86_64" ]; then
        ASSET_NAME="pikpaktui-x86_64-freebsd.tar.gz"
    else
        echo -e "${RED}Error: Unsupported FreeBSD architecture: ${ARCH}${NC}"
        exit 1
    fi
    INSTALL_DIR="/usr/local/bin"
else
    echo -e "${RED}Error: Unsupported OS: ${OS}${NC}"
    echo "For Windows, download from GitHub Releases directly."
    exit 1
fi

# Define URLs
DOWNLOAD_URL="https://github.com/Bengerthelorf/pikpaktui/releases/latest/download/${ASSET_NAME}"

echo -e "Target Asset: ${GREEN}${ASSET_NAME}${NC}"
echo -e "Download URL: ${BLUE}${DOWNLOAD_URL}${NC}"

# Create temp directory
TMP_DIR=$(mktemp -d)
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

# Download
echo -e "${BLUE}>>> Downloading...${NC}"
if curl -L --fail --progress-bar -o "${TMP_DIR}/${ASSET_NAME}" "$DOWNLOAD_URL"; then
    echo -e "${GREEN}Download successful.${NC}"
else
    echo -e "${RED}Download failed. Retrying with TLS v1.2 / HTTP 1.1...${NC}"
    if curl -L --fail --progress-bar --tlsv1.2 --http1.1 -o "${TMP_DIR}/${ASSET_NAME}" "$DOWNLOAD_URL"; then
        echo -e "${GREEN}Download successful (TLS v1.2 / HTTP 1.1).${NC}"
    else
        echo -e "${RED}Download failed! Please check if the release asset exists.${NC}"
        exit 1
    fi
fi

# Extract
echo -e "${BLUE}>>> Extracting...${NC}"
tar -xzf "${TMP_DIR}/${ASSET_NAME}" -C "$TMP_DIR"

# Find binary
BINARY_PATH=$(find "$TMP_DIR" -type f -name "pikpaktui" | head -n 1)

if [ -z "$BINARY_PATH" ]; then
    echo -e "${RED}Error: 'pikpaktui' binary not found in the archive.${NC}"
    exit 1
fi

echo -e "Found binary at: ${BINARY_PATH}"

# Install
TARGET_PATH="${INSTALL_DIR}/pikpaktui"
echo -e "${BLUE}>>> Installing to ${TARGET_PATH}...${NC}"

if [ -w "$INSTALL_DIR" ]; then
    mv "$BINARY_PATH" "$TARGET_PATH"
else
    echo "Sudo permissions required to install to ${INSTALL_DIR}"
    sudo mv "$BINARY_PATH" "$TARGET_PATH"
fi

if [ -w "$TARGET_PATH" ]; then
     chmod +x "$TARGET_PATH"
else
     echo "Sudo permissions required to chmod ${TARGET_PATH}"
     sudo chmod +x "$TARGET_PATH"
fi

echo -e "${GREEN}>>> Installation Complete!${NC}"
echo -e "Run ${BLUE}pikpaktui --version${NC} to verify."
