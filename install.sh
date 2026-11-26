#!/usr/bin/env bash
# Thocci automated installer for Linux and macOS (CLI + Desktop App).
# Works across Bash, Zsh, and Fish terminals.
# Pass --cli-only (or set THOCCI_CLI_ONLY=1) for headless/CLI-only installation.

set -euo pipefail

REPO="ItzEvhyx/Thocci"
FALLBACK_VERSION="v0.1.0"
INSTALL_DIR="${HOME}/.local/bin"
BIN_NAME="thocci"
DESKTOP_BIN_NAME="thocci-app"
TARGET_PATH="${INSTALL_DIR}/${BIN_NAME}"
DESKTOP_TARGET_PATH="${INSTALL_DIR}/${DESKTOP_BIN_NAME}"
CLI_ONLY="${THOCCI_CLI_ONLY:-0}"

for arg in "$@"; do
    case "${arg}" in
        --cli-only) CLI_ONLY="1" ;;
    esac
done

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

verify_sha256() {
    local file="$1"
    local sha_url="$2"
    local sha_file="${file}.sha256"
    if [ -n "${sha_url}" ] && curl -fsSL "${sha_url}" -o "${sha_file}" 2>/dev/null; then
        local expected actual
        expected="$(awk '{print $1}' "${sha_file}")"
        if command -v sha256sum >/dev/null 2>&1; then
            actual="$(sha256sum "${file}" | awk '{print $1}')"
        elif command -v shasum >/dev/null 2>&1; then
            actual="$(shasum -a 256 "${file}" | awk '{print $1}')"
        else
            actual="${expected}"
        fi
        if [ -n "${expected}" ] && [ "${expected}" != "${actual}" ]; then
            echo "Checksum mismatch for ${file}! Expected ${expected}, got ${actual}." >&2
            exit 1
        fi
    fi
}

is_graphical_session() {
    if [ "${CLI_ONLY}" = "1" ]; then
        return 1
    fi
    if [ "${OS}" = "Darwin" ]; then
        [ -z "${SSH_CONNECTION:-}" ] && return 0 || return 1
    fi
    if [ -n "${DISPLAY:-}" ] || [ -n "${WAYLAND_DISPLAY:-}" ]; then
        return 0
    fi
    return 1
}

install_linux_desktop_entry() {
    local exec_path="$1"
    local icon_src_dir="${2:-}"
    local apps_dir="${XDG_DATA_HOME:-${HOME}/.local/share}/applications"
    local icons_base="${XDG_DATA_HOME:-${HOME}/.local/share}/icons/hicolor"
    mkdir -p "${apps_dir}"

    local icon_value="thocci"
    if [ -n "${icon_src_dir}" ]; then
        for sz in 32 128 256 512; do
            if [ -f "${icon_src_dir}/thocci-${sz}.png" ]; then
                mkdir -p "${icons_base}/${sz}x${sz}/apps"
                cp "${icon_src_dir}/thocci-${sz}.png" "${icons_base}/${sz}x${sz}/apps/thocci.png"
            fi
        done
    fi

    cat > "${apps_dir}/thocci.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Thocci
Comment=Mechanical keyboard sounds for your desktop
Exec=${exec_path}
Icon=${icon_value}
Terminal=false
Categories=Utility;Audio;
StartupWMClass=thocci-app
EOF
}

echo "Installing Thocci..."
mkdir -p "${INSTALL_DIR}"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

LATEST_JSON=""
fetch_latest_json() {
    if [ -z "${LATEST_JSON}" ]; then
        LATEST_JSON="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null || true)"
    fi
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || pwd)"

# 1. Install CLI binary (always preserved)
if [ -f "${SCRIPT_DIR}/target/release/${BIN_NAME}" ]; then
    echo "Installing CLI from local release build (${SCRIPT_DIR}/target/release/${BIN_NAME})..."
    rm -f "${TARGET_PATH}"
    cp "${SCRIPT_DIR}/target/release/${BIN_NAME}" "${TARGET_PATH}"
    chmod +x "${TARGET_PATH}"
elif [ -f "${SCRIPT_DIR}/${BIN_NAME}" ]; then
    echo "Installing CLI from local binary (${SCRIPT_DIR}/${BIN_NAME})..."
    rm -f "${TARGET_PATH}"
    cp "${SCRIPT_DIR}/${BIN_NAME}" "${TARGET_PATH}"
    chmod +x "${TARGET_PATH}"
else
    fetch_latest_json
    TAR_URL=""
    SHA_URL=""
    if [ -n "${LATEST_JSON}" ]; then
        TAR_URL="$(printf '%s\n' "${LATEST_JSON}" | grep '"browser_download_url"' | grep -v '\.sha256"' | grep "/thocci-[^/]*${ASSET_TARGET}\.tar\.gz" | head -n 1 | cut -d '"' -f 4 || true)"
        SHA_URL="$(printf '%s\n' "${LATEST_JSON}" | grep '"browser_download_url"' | grep "/thocci-[^/]*${ASSET_TARGET}\.tar\.gz\.sha256" | head -n 1 | cut -d '"' -f 4 || true)"
    fi
    if [ -z "${TAR_URL}" ]; then
        TAR_URL="https://github.com/${REPO}/releases/download/${FALLBACK_VERSION}/thocci-${FALLBACK_VERSION}-${ASSET_TARGET}.tar.gz"
        SHA_URL="${TAR_URL}.sha256"
    fi

    echo "Downloading ${TAR_URL}..."
    curl -fsSL "${TAR_URL}" -o "${TMP_DIR}/thocci.tar.gz"
    verify_sha256 "${TMP_DIR}/thocci.tar.gz" "${SHA_URL}"

    tar -xzf "${TMP_DIR}/thocci.tar.gz" -C "${TMP_DIR}"
    rm -f "${TARGET_PATH}"
    mv "${TMP_DIR}/${BIN_NAME}" "${TARGET_PATH}"
    chmod +x "${TARGET_PATH}"
fi

# 2. Install Desktop GUI bundle alongside CLI when not CLI-only
DESKTOP_LAUNCH_TARGET=""
if [ "${CLI_ONLY}" != "1" ]; then
    if [ "${OS}" = "Darwin" ]; then
        MAC_APP_DEST="/Applications/Thocci.app"
        if [ ! -w "/Applications" ]; then
            MAC_APP_DEST="${HOME}/Applications/Thocci.app"
            mkdir -p "${HOME}/Applications"
        fi

        if [ -d "${SCRIPT_DIR}/target/release/bundle/macos/Thocci.app" ]; then
            echo "Installing macOS app from local bundle..."
            rm -rf "${MAC_APP_DEST}"
            cp -R "${SCRIPT_DIR}/target/release/bundle/macos/Thocci.app" "${MAC_APP_DEST}"
            DESKTOP_LAUNCH_TARGET="${MAC_APP_DEST}"
        elif [ -d "${SCRIPT_DIR}/Thocci.app" ]; then
            echo "Installing macOS app from local Thocci.app..."
            rm -rf "${MAC_APP_DEST}"
            cp -R "${SCRIPT_DIR}/Thocci.app" "${MAC_APP_DEST}"
            DESKTOP_LAUNCH_TARGET="${MAC_APP_DEST}"
        else
            fetch_latest_json
            DESKTOP_URL=""
            DESKTOP_SHA=""
            if [ -n "${LATEST_JSON}" ]; then
                DESKTOP_URL="$(printf '%s\n' "${LATEST_JSON}" | grep '"browser_download_url"' | grep -v '\.sha256"' | grep "thocci-desktop-[^/]*${ASSET_TARGET}\.tar\.gz" | head -n 1 | cut -d '"' -f 4 || true)"
                DESKTOP_SHA="$(printf '%s\n' "${LATEST_JSON}" | grep '"browser_download_url"' | grep "thocci-desktop-[^/]*${ASSET_TARGET}\.tar\.gz\.sha256" | head -n 1 | cut -d '"' -f 4 || true)"
            fi
            if [ -z "${DESKTOP_URL}" ]; then
                DESKTOP_URL="https://github.com/${REPO}/releases/download/${FALLBACK_VERSION}/thocci-desktop-${FALLBACK_VERSION}-${ASSET_TARGET}.tar.gz"
                DESKTOP_SHA="${DESKTOP_URL}.sha256"
            fi
            if curl -fsSL "${DESKTOP_URL}" -o "${TMP_DIR}/thocci-desktop.tar.gz" 2>/dev/null; then
                verify_sha256 "${TMP_DIR}/thocci-desktop.tar.gz" "${DESKTOP_SHA}"
                mkdir -p "${TMP_DIR}/desktop"
                tar -xzf "${TMP_DIR}/thocci-desktop.tar.gz" -C "${TMP_DIR}/desktop"
                if [ -d "${TMP_DIR}/desktop/Thocci.app" ]; then
                    rm -rf "${MAC_APP_DEST}"
                    mv "${TMP_DIR}/desktop/Thocci.app" "${MAC_APP_DEST}"
                    DESKTOP_LAUNCH_TARGET="${MAC_APP_DEST}"
                fi
            fi
        fi
    else
        # Linux desktop binary + XDG desktop launcher + ESC icons
        LOCAL_ICON_DIR="${SCRIPT_DIR}/assets/desktop/brand/png"
        if [ -f "${SCRIPT_DIR}/target/release/${DESKTOP_BIN_NAME}" ]; then
            echo "Installing desktop app from local release build..."
            rm -f "${DESKTOP_TARGET_PATH}"
            cp "${SCRIPT_DIR}/target/release/${DESKTOP_BIN_NAME}" "${DESKTOP_TARGET_PATH}"
            chmod +x "${DESKTOP_TARGET_PATH}"
            install_linux_desktop_entry "${DESKTOP_TARGET_PATH}" "${LOCAL_ICON_DIR}"
            DESKTOP_LAUNCH_TARGET="${DESKTOP_TARGET_PATH}"
        elif [ -f "${SCRIPT_DIR}/${DESKTOP_BIN_NAME}" ]; then
            echo "Installing desktop app from local binary..."
            rm -f "${DESKTOP_TARGET_PATH}"
            cp "${SCRIPT_DIR}/${DESKTOP_BIN_NAME}" "${DESKTOP_TARGET_PATH}"
            chmod +x "${DESKTOP_TARGET_PATH}"
            install_linux_desktop_entry "${DESKTOP_TARGET_PATH}" "${LOCAL_ICON_DIR}"
            DESKTOP_LAUNCH_TARGET="${DESKTOP_TARGET_PATH}"
        else
            fetch_latest_json
            DESKTOP_URL=""
            DESKTOP_SHA=""
            if [ -n "${LATEST_JSON}" ]; then
                DESKTOP_URL="$(printf '%s\n' "${LATEST_JSON}" | grep '"browser_download_url"' | grep -v '\.sha256"' | grep "thocci-desktop-[^/]*${ASSET_TARGET}\.tar\.gz" | head -n 1 | cut -d '"' -f 4 || true)"
                DESKTOP_SHA="$(printf '%s\n' "${LATEST_JSON}" | grep '"browser_download_url"' | grep "thocci-desktop-[^/]*${ASSET_TARGET}\.tar\.gz\.sha256" | head -n 1 | cut -d '"' -f 4 || true)"
            fi
            if [ -z "${DESKTOP_URL}" ]; then
                DESKTOP_URL="https://github.com/${REPO}/releases/download/${FALLBACK_VERSION}/thocci-desktop-${FALLBACK_VERSION}-${ASSET_TARGET}.tar.gz"
                DESKTOP_SHA="${DESKTOP_URL}.sha256"
            fi
            if curl -fsSL "${DESKTOP_URL}" -o "${TMP_DIR}/thocci-desktop.tar.gz" 2>/dev/null; then
                verify_sha256 "${TMP_DIR}/thocci-desktop.tar.gz" "${DESKTOP_SHA}"
                mkdir -p "${TMP_DIR}/desktop"
                tar -xzf "${TMP_DIR}/thocci-desktop.tar.gz" -C "${TMP_DIR}/desktop"
                if [ -f "${TMP_DIR}/desktop/${DESKTOP_BIN_NAME}" ]; then
                    rm -f "${DESKTOP_TARGET_PATH}"
                    mv "${TMP_DIR}/desktop/${DESKTOP_BIN_NAME}" "${DESKTOP_TARGET_PATH}"
                    chmod +x "${DESKTOP_TARGET_PATH}"
                    install_linux_desktop_entry "${DESKTOP_TARGET_PATH}" "${TMP_DIR}/desktop/icons"
                    DESKTOP_LAUNCH_TARGET="${DESKTOP_TARGET_PATH}"
                fi
            fi
        fi
    fi
fi

echo ""
echo "✓ thocci CLI was successfully installed to ${TARGET_PATH}!"
if [ -n "${DESKTOP_LAUNCH_TARGET}" ]; then
    echo "✓ Thocci Desktop app was installed to ${DESKTOP_LAUNCH_TARGET}!"
fi
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

if [ "${OS}" = "Linux" ]; then
    if ! id -nG 2>/dev/null | tr ' ' '\n' | grep -qx 'input'; then
        echo "NOTE: Linux keyboard capture requires membership in the 'input' group."
        echo "Run once, then log out and back in:"
        echo "  sudo usermod -aG input \"\$USER\""
        echo ""
    fi
fi

if [ -n "${DESKTOP_LAUNCH_TARGET}" ] && is_graphical_session; then
    echo "Launching Thocci..."
    if [ "${OS}" = "Darwin" ]; then
        open -a "${DESKTOP_LAUNCH_TARGET}" >/dev/null 2>&1 || "${TARGET_PATH}" --welcome
    else
        nohup "${DESKTOP_LAUNCH_TARGET}" >/dev/null 2>&1 &
    fi
else
    "${TARGET_PATH}" --welcome
fi
