#!/usr/bin/env bash
# Thocci automated installer for Linux and macOS.
# Works across Bash, Zsh, and Fish terminals.

set -euo pipefail

REPO="ItzEvhyx/Thocci"
FALLBACK_VERSION="v0.1.0"
INSTALL_DIR="${HOME}/.local/bin"
BIN_NAME="thocci"
TARGET_PATH="${INSTALL_DIR}/${BIN_NAME}"

echo "Installing Thocci..."
mkdir -p "${INSTALL_DIR}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || pwd)"
if [ -f "${SCRIPT_DIR}/target/release/${BIN_NAME}" ]; then
    echo "Installing from local release build (${SCRIPT_DIR}/target/release/${BIN_NAME})..."
    rm -f "${TARGET_PATH}"
    cp "${SCRIPT_DIR}/target/release/${BIN_NAME}" "${TARGET_PATH}"
    chmod +x "${TARGET_PATH}"
elif [ -f "${SCRIPT_DIR}/${BIN_NAME}" ]; then
    echo "Installing from local binary (${SCRIPT_DIR}/${BIN_NAME})..."
    rm -f "${TARGET_PATH}"
    cp "${SCRIPT_DIR}/${BIN_NAME}" "${TARGET_PATH}"
    chmod +x "${TARGET_PATH}"
else
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "${OS}" in
        Linux*)
            case "${ARCH}" in
                x86_64) ASSET_TARGET="x86_64-unknown-linux-gnu" ;;
                aarch64|arm64) ASSET_TARGET="aarch64-unknown-linux-gnu" ;;
                *) echo "Unsupported Linux architecture: ${ARCH}" >&2; exit 1 ;;
            esac
            ;;
        Darwin*)
            case "${ARCH}" in
                x86_64) ASSET_TARGET="x86_64-apple-darwin" ;;
                arm64|aarch64) ASSET_TARGET="aarch64-apple-darwin" ;;
                *) echo "Unsupported macOS architecture: ${ARCH}" >&2; exit 1 ;;
            esac
            ;;
        *)
            echo "Unsupported OS: ${OS}. Please use install.ps1 on Windows." >&2
            exit 1
            ;;
    esac

    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "${TMP_DIR}"' EXIT

    TAR_URL=""
    SHA_URL=""
    LATEST_JSON="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null || true)"
    if [ -n "${LATEST_JSON}" ]; then
        TAR_URL="$(printf '%s\n' "${LATEST_JSON}" | grep '"browser_download_url"' | grep -v '\.sha256"' | grep "${ASSET_TARGET}\.tar\.gz" | head -n 1 | cut -d '"' -f 4 || true)"
        SHA_URL="$(printf '%s\n' "${LATEST_JSON}" | grep '"browser_download_url"' | grep "${ASSET_TARGET}\.tar\.gz\.sha256" | head -n 1 | cut -d '"' -f 4 || true)"
    fi

    if [ -z "${TAR_URL}" ]; then
        TAR_URL="https://github.com/${REPO}/releases/download/${FALLBACK_VERSION}/thocci-${FALLBACK_VERSION}-${ASSET_TARGET}.tar.gz"
        SHA_URL="${TAR_URL}.sha256"
    fi

    echo "Downloading ${TAR_URL}..."
    curl -fsSL "${TAR_URL}" -o "${TMP_DIR}/thocci.tar.gz"

    if [ -n "${SHA_URL}" ] && curl -fsSL "${SHA_URL}" -o "${TMP_DIR}/thocci.tar.gz.sha256" 2>/dev/null; then
        EXPECTED="$(awk '{print $1}' "${TMP_DIR}/thocci.tar.gz.sha256")"
        if command -v sha256sum >/dev/null 2>&1; then
            ACTUAL="$(sha256sum "${TMP_DIR}/thocci.tar.gz" | awk '{print $1}')"
        elif command -v shasum >/dev/null 2>&1; then
            ACTUAL="$(shasum -a 256 "${TMP_DIR}/thocci.tar.gz" | awk '{print $1}')"
        else
            ACTUAL="${EXPECTED}"
        fi
        if [ "${EXPECTED}" != "${ACTUAL}" ]; then
            echo "Checksum mismatch! Expected ${EXPECTED}, got ${ACTUAL}." >&2
            exit 1
        fi
    fi

    tar -xzf "${TMP_DIR}/thocci.tar.gz" -C "${TMP_DIR}"
    rm -f "${TARGET_PATH}"
    mv "${TMP_DIR}/${BIN_NAME}" "${TARGET_PATH}"
    chmod +x "${TARGET_PATH}"
fi

echo ""
echo "✓ thocci was successfully installed to ${TARGET_PATH}!"
echo ""

case ":${PATH}:" in
    *:"${INSTALL_DIR}":*) ;;
    *)
        echo "NOTE: ${INSTALL_DIR} is not currently in your PATH."
        echo "Add it with:"
        echo "  • Fish:  set -Ua fish_user_paths ~/.local/bin"
        echo "  • Bash:  echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.bashrc && source ~/.bashrc"
        echo "  • Zsh:   echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.zshrc && source ~/.zshrc"
        echo ""
        ;;
esac

if [ "$(uname -s)" = "Linux" ]; then
    if ! id -nG 2>/dev/null | tr ' ' '\n' | grep -qx 'input'; then
        echo "NOTE: Linux keyboard capture requires membership in the 'input' group."
        echo "Run once, then log out and back in:"
        echo "  sudo usermod -aG input \"\$USER\""
        echo ""
    fi
fi

"${TARGET_PATH}" --welcome
