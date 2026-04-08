#!/bin/sh
# tardigrade installer — https://github.com/gnathoi/tardigrade
# Usage: curl -fsSL https://raw.githubusercontent.com/gnathoi/tardigrade/main/install.sh | sh
set -e

REPO="gnathoi/tardigrade"
INSTALL_DIR="${TDG_INSTALL_DIR:-$HOME/.tardigrade/bin}"

main() {
    detect_platform
    get_latest_version
    download_and_install
    setup_path
    verify
}

detect_platform() {
    OS=$(uname -s)
    ARCH=$(uname -m)

    case "$OS" in
        Linux)  OS_TARGET="unknown-linux-gnu" ;;
        Darwin) OS_TARGET="apple-darwin" ;;
        *)      err "unsupported OS: $OS (try cargo install tardigrade)" ;;
    esac

    case "$ARCH" in
        x86_64|amd64)   ARCH_TARGET="x86_64" ;;
        aarch64|arm64)  ARCH_TARGET="aarch64" ;;
        *)              err "unsupported architecture: $ARCH" ;;
    esac

    TARGET="${ARCH_TARGET}-${OS_TARGET}"
    ASSET="tdg-${TARGET}.tar.gz"
    echo "  platform: ${OS}/${ARCH} (${TARGET})"
}

get_latest_version() {
    # Follow the redirect from /releases/latest to extract the version tag
    if command -v curl >/dev/null 2>&1; then
        REDIRECT_URL=$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/${REPO}/releases/latest")
    elif command -v wget >/dev/null 2>&1; then
        REDIRECT_URL=$(wget --max-redirect=1 -q -O /dev/null --server-response "https://github.com/${REPO}/releases/latest" 2>&1 | grep -i "Location:" | tail -1 | awk '{print $2}' | tr -d '\r')
    else
        err "curl or wget required"
    fi

    VERSION=$(echo "$REDIRECT_URL" | grep -oE '[^/]+$' | sed 's/^v//')
    if [ -z "$VERSION" ]; then
        err "could not determine latest version"
    fi
    echo "  version:  v${VERSION}"
}

download_and_install() {
    BASE_URL="https://github.com/${REPO}/releases/download/v${VERSION}"
    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT

    echo "  downloading ${ASSET}..."
    fetch "${BASE_URL}/${ASSET}" "${TMP_DIR}/${ASSET}"

    # Try to verify checksum (non-fatal if checksums not available)
    if fetch "${BASE_URL}/B3SUMS" "${TMP_DIR}/B3SUMS" 2>/dev/null; then
        if command -v b3sum >/dev/null 2>&1; then
            echo "  verifying checksum..."
            EXPECTED=$(grep "$ASSET" "${TMP_DIR}/B3SUMS" | awk '{print $1}')
            ACTUAL=$(b3sum "${TMP_DIR}/${ASSET}" | awk '{print $1}')
            if [ "$EXPECTED" != "$ACTUAL" ]; then
                err "checksum mismatch! expected ${EXPECTED}, got ${ACTUAL}"
            fi
            echo "  checksum:  ok"
        elif command -v sha256sum >/dev/null 2>&1 && fetch "${BASE_URL}/SHA256SUMS" "${TMP_DIR}/SHA256SUMS" 2>/dev/null; then
            echo "  verifying checksum (SHA256)..."
            EXPECTED=$(grep "$ASSET" "${TMP_DIR}/SHA256SUMS" | awk '{print $1}')
            ACTUAL=$(sha256sum "${TMP_DIR}/${ASSET}" | awk '{print $1}')
            if [ "$EXPECTED" != "$ACTUAL" ]; then
                err "checksum mismatch! expected ${EXPECTED}, got ${ACTUAL}"
            fi
            echo "  checksum:  ok"
        elif command -v shasum >/dev/null 2>&1 && fetch "${BASE_URL}/SHA256SUMS" "${TMP_DIR}/SHA256SUMS" 2>/dev/null; then
            echo "  verifying checksum (SHA256)..."
            EXPECTED=$(grep "$ASSET" "${TMP_DIR}/SHA256SUMS" | awk '{print $1}')
            ACTUAL=$(shasum -a 256 "${TMP_DIR}/${ASSET}" | awk '{print $1}')
            if [ "$EXPECTED" != "$ACTUAL" ]; then
                err "checksum mismatch! expected ${EXPECTED}, got ${ACTUAL}"
            fi
            echo "  checksum:  ok"
        else
            echo "  checksum:  skipped (no b3sum or sha256sum found)"
        fi
    else
        echo "  checksum:  skipped (checksums not available for this release)"
    fi

    echo "  extracting..."
    mkdir -p "$INSTALL_DIR"
    tar xzf "${TMP_DIR}/${ASSET}" -C "$INSTALL_DIR"
    chmod +x "${INSTALL_DIR}/tdg"
}

setup_path() {
    case ":$PATH:" in
        *":${INSTALL_DIR}:"*) return ;;
    esac

    echo ""
    echo "  Add tdg to your PATH by adding this to your shell config:"
    echo ""

    SHELL_NAME=$(basename "${SHELL:-/bin/sh}")
    case "$SHELL_NAME" in
        zsh)  RC="~/.zshrc" ;;
        bash) RC="~/.bashrc" ;;
        fish)
            echo "    set -Ux fish_user_paths ${INSTALL_DIR} \$fish_user_paths"
            return
            ;;
        *)    RC="~/.profile" ;;
    esac

    echo "    echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ${RC}"
    echo ""
}

verify() {
    if [ -x "${INSTALL_DIR}/tdg" ]; then
        INSTALLED_VERSION=$("${INSTALL_DIR}/tdg" --version 2>/dev/null | awk '{print $NF}')
        echo ""
        echo "  installed tdg v${INSTALLED_VERSION} to ${INSTALL_DIR}/tdg"
        echo ""
    else
        err "installation failed — tdg not found at ${INSTALL_DIR}/tdg"
    fi
}

fetch() {
    URL="$1"
    OUTPUT="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$OUTPUT" "$URL"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$OUTPUT" "$URL"
    else
        err "curl or wget required"
    fi
}

err() {
    echo "  error: $1" >&2
    exit 1
}

main
