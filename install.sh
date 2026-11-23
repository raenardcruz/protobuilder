#!/bin/sh
# ProtoBuilder installer script
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/raenardcruz/protobuilder/main/install.sh | sh
#
# Options (via flags or environment variables):
#   -v, --version <VER>   Version to install (default: latest, or PROTOBUILDER_VERSION)
#   -d, --dir <DIR>       Install directory (default: $PROTOBUILDER_HOME/bin, or PROTOBUILDER_INSTALL_DIR)
#   --from-source         Build from source with Cargo instead of downloading a release binary
#   --no-modify-path      Do not modify shell profile files (or PROTOBUILDER_NO_MODIFY_PATH=1)
#   -h, --help            Show this help message

set -eu

REPO="${PROTOBUILDER_REPO:-raenardcruz/protobuilder}"
BIN_NAME="protobuilder"
VERSION="${PROTOBUILDER_VERSION:-latest}"
PB_HOME="${PROTOBUILDER_HOME:-$HOME/.protobuilder}"
INSTALL_DIR="${PROTOBUILDER_INSTALL_DIR:-$PB_HOME/bin}"
FROM_SOURCE="${PROTOBUILDER_FROM_SOURCE:-0}"
NO_MODIFY_PATH="${PROTOBUILDER_NO_MODIFY_PATH:-0}"
TMP_DIR=""

# --- Output helpers ---------------------------------------------------------

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    C_RESET='\033[0m'
    C_BOLD='\033[1m'
    C_GREEN='\033[0;32m'
    C_YELLOW='\033[0;33m'
    C_RED='\033[0;31m'
    C_CYAN='\033[0;36m'
else
    C_RESET=''
    C_BOLD=''
    C_GREEN=''
    C_YELLOW=''
    C_RED=''
    C_CYAN=''
fi

info() {
    printf '%b==>%b %s\n' "$C_CYAN$C_BOLD" "$C_RESET" "$1"
}

success() {
    printf '%b==>%b %s\n' "$C_GREEN$C_BOLD" "$C_RESET" "$1"
}

warn() {
    printf '%bwarning:%b %s\n' "$C_YELLOW$C_BOLD" "$C_RESET" "$1" >&2
}

err() {
    printf '%berror:%b %s\n' "$C_RED$C_BOLD" "$C_RESET" "$1" >&2
    exit 1
}

cleanup() {
    if [ -n "$TMP_DIR" ] && [ -d "$TMP_DIR" ]; then
        rm -rf "$TMP_DIR"
    fi
}

trap cleanup EXIT INT TERM HUP

usage() {
    cat <<EOF
ProtoBuilder Installer

Usage:
  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | sh
  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | sh -s -- [OPTIONS]

Options:
  -v, --version <VERSION>  Install a specific release tag (e.g. v1.0.0) [default: latest]
  -d, --dir <DIR>          Binary install directory [default: \$HOME/.protobuilder/bin]
      --from-source        Build from source using Cargo instead of prebuilt binaries
      --no-modify-path     Skip registering PROTOBUILDER_HOME and PATH in shell config
  -h, --help               Show this help message

Environment Variables:
  PROTOBUILDER_VERSION         Version to install (default: latest)
  PROTOBUILDER_HOME            Base directory (default: \$HOME/.protobuilder)
  PROTOBUILDER_INSTALL_DIR     Binary install directory (default: \$PROTOBUILDER_HOME/bin)
  PROTOBUILDER_FROM_SOURCE     Set to 1 to force building from source
  PROTOBUILDER_NO_MODIFY_PATH  Set to 1 to skip modifying shell profile files
  PROTOBUILDER_REPO            GitHub repository (default: raenardcruz/protobuilder)
EOF
}

# --- Argument parsing -------------------------------------------------------

parse_args() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            -v|--version)
                [ "$#" -ge 2 ] || err "Option $1 requires a value."
                VERSION="$2"
                shift 2
                ;;
            --version=*)
                VERSION="${1#*=}"
                shift
                ;;
            -d|--dir)
                [ "$#" -ge 2 ] || err "Option $1 requires a value."
                INSTALL_DIR="$2"
                shift 2
                ;;
            --dir=*)
                INSTALL_DIR="${1#*=}"
                shift
                ;;
            --from-source)
                FROM_SOURCE=1
                shift
                ;;
            --no-modify-path)
                NO_MODIFY_PATH=1
                shift
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                err "Unknown option: $1 (run with --help for usage)"
                ;;
        esac
    done
}

# --- System & version detection ---------------------------------------------

need_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        err "Required command '$1' was not found. Please install it and try again."
    fi
}

detect_platform() {
    raw_os="$(uname -s)"
    case "$raw_os" in
        Darwin)
            OS="macos"
            ;;
        Linux)
            OS="linux"
            ;;
        MINGW*|MSYS*|CYGWIN*)
            err "Windows shell detected ($raw_os). Use PowerShell to install on Windows:
  irm https://raw.githubusercontent.com/${REPO}/main/install.ps1 | iex"
            ;;
        *)
            err "Unsupported operating system: $raw_os"
            ;;
    esac

    raw_arch="$(uname -m)"
    case "$raw_arch" in
        arm64|aarch64)
            ARCH="arm64"
            ;;
        x86_64|amd64)
            ARCH="x86_64"
            ;;
        *)
            err "Unsupported CPU architecture: $raw_arch"
            ;;
    esac

    # Detect Apple Silicon running under Rosetta 2
    if [ "$OS" = "macos" ] && [ "$ARCH" = "x86_64" ]; then
        if [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || printf '0')" = "1" ]; then
            ARCH="arm64"
        fi
    fi
}

resolve_version() {
    if [ "$VERSION" = "latest" ]; then
        info "Resolving latest release version from github.com/${REPO}..."
        latest_url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/${REPO}/releases/latest" 2>/dev/null || true)"
        resolved="$(printf '%s\n' "$latest_url" | sed -n 's#.*/releases/tag/\([^/?#]*\).*#\1#p')"

        if [ -z "$resolved" ]; then
            api_json="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null || true)"
            resolved="$(printf '%s\n' "$api_json" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
        fi

        if [ -z "$resolved" ]; then
            err "Could not determine the latest release version. Specify one explicitly with --version (e.g. --version v1.0.0)."
        fi
        VERSION="$resolved"
    else
        case "$VERSION" in
            v*) ;;
            *)  VERSION="v${VERSION}" ;;
        esac
    fi
}

# --- Download & build -------------------------------------------------------

download_prebuilt() {
    archive_file="$TMP_DIR/protobuilder.tar.gz"
    extract_dir="$TMP_DIR/extract"
    mkdir -p "$extract_dir"

    # Candidate asset names for the detected OS/arch
    candidates="protobuilder-${VERSION}-${OS}-${ARCH}.tar.gz"
    if [ "$ARCH" = "x86_64" ]; then
        candidates="${candidates} protobuilder-${VERSION}-${OS}-amd64.tar.gz"
    elif [ "$ARCH" = "arm64" ]; then
        candidates="${candidates} protobuilder-${VERSION}-${OS}-aarch64.tar.gz"
    fi
    if [ "$OS" = "macos" ]; then
        candidates="${candidates} protobuilder-${VERSION}-darwin-${ARCH}.tar.gz"
    fi

    downloaded=0
    for asset in $candidates; do
        url="https://github.com/${REPO}/releases/download/${VERSION}/${asset}"
        info "Checking release archive: ${asset}"
        if curl -fsSL -o "$archive_file" "$url" 2>/dev/null; then
            downloaded=1
            break
        fi
    done

    if [ "$downloaded" -ne 1 ]; then
        return 1
    fi

    info "Extracting ${BIN_NAME}..."
    tar -xzf "$archive_file" -C "$extract_dir" 2>/dev/null || tar -xzf "$archive_file" -C "$extract_dir"

    if [ -f "$extract_dir/$BIN_NAME" ]; then
        BUILT_BIN="$extract_dir/$BIN_NAME"
    else
        BUILT_BIN="$(find "$extract_dir" -type f -name "$BIN_NAME" | head -n 1)"
    fi

    if [ -z "${BUILT_BIN:-}" ] || [ ! -f "$BUILT_BIN" ]; then
        err "Release archive was downloaded, but '${BIN_NAME}' binary was not found inside."
    fi

    return 0
}

build_from_source() {
    if ! command -v cargo >/dev/null 2>&1; then
        err "No prebuilt binary is available for ${OS}-${ARCH} in ${VERSION}, and 'cargo' is not installed.
Install Rust via https://rustup.rs and re-run this script, or download a release manually from:
  https://github.com/${REPO}/releases"
    fi

    src_archive="$TMP_DIR/source.tar.gz"
    src_extract="$TMP_DIR/src"
    mkdir -p "$src_extract"

    src_url="https://github.com/${REPO}/archive/refs/tags/${VERSION}.tar.gz"
    info "Downloading source archive for ${VERSION}..."
    if ! curl -fsSL -o "$src_archive" "$src_url"; then
        err "Failed to download source archive from ${src_url}"
    fi

    tar -xzf "$src_archive" -C "$src_extract"
    cargo_toml="$(find "$src_extract" -maxdepth 3 -type f -name 'Cargo.toml' | head -n 1)"
    if [ -z "$cargo_toml" ]; then
        err "Could not locate Cargo.toml in extracted source archive."
    fi
    src_dir="$(dirname "$cargo_toml")"

    info "Building ${BIN_NAME} from source with Cargo (release mode)..."
    (cd "$src_dir" && cargo build --release)

    BUILT_BIN="$src_dir/target/release/$BIN_NAME"
    if [ ! -f "$BUILT_BIN" ]; then
        err "Cargo build completed, but binary was not found at ${BUILT_BIN}."
    fi
}

install_binary() {
    mkdir -p "$INSTALL_DIR"
    tmp_target="${INSTALL_DIR}/.${BIN_NAME}.tmp.$$"
    cp "$BUILT_BIN" "$tmp_target"
    chmod 755 "$tmp_target"
    mv -f "$tmp_target" "${INSTALL_DIR}/${BIN_NAME}"

    if [ "$OS" = "macos" ] && command -v xattr >/dev/null 2>&1; then
        xattr -d com.apple.quarantine "${INSTALL_DIR}/${BIN_NAME}" 2>/dev/null || true
    fi

    success "Installed ${BIN_NAME} to ${INSTALL_DIR}/${BIN_NAME}"
}

# --- Environment variable & PATH registration -------------------------------

format_home_expr() {
    target_path="$1"
    home_clean="${HOME%/}"
    case "$target_path" in
        "$home_clean")
            printf '%s' '$HOME'
            ;;
        "$home_clean"/*)
            printf '$HOME/%s' "${target_path#"$home_clean"/}"
            ;;
        *)
            printf '%s' "$target_path"
            ;;
    esac
}

append_posix_profile() {
    profile_file="$1"
    home_expr="$2"
    dir_expr="$3"
    use_home_bin="$4"

    if [ -z "$PRIMARY_PROFILE" ]; then
        PRIMARY_PROFILE="$profile_file"
    fi

    if [ -f "$profile_file" ]; then
        if [ "$use_home_bin" = "1" ] && grep -Fq 'PROTOBUILDER_HOME/bin' "$profile_file" 2>/dev/null; then
            info "Environment variables already registered in ${profile_file}"
            return 0
        fi
        if grep -Fq "$dir_expr" "$profile_file" 2>/dev/null || grep -Fq "$INSTALL_DIR" "$profile_file" 2>/dev/null; then
            info "Environment variables already registered in ${profile_file}"
            return 0
        fi
    else
        mkdir -p "$(dirname "$profile_file")"
        : > "$profile_file"
    fi

    if [ -s "$profile_file" ]; then
        printf '\n' >> "$profile_file"
    fi

    if [ "$use_home_bin" = "1" ]; then
        {
            printf '# ProtoBuilder\n'
            printf 'export PROTOBUILDER_HOME="%s"\n' "$home_expr"
            printf 'export PATH="$PROTOBUILDER_HOME/bin:$PATH"\n'
        } >> "$profile_file"
    else
        {
            printf '# ProtoBuilder\n'
            printf 'export PROTOBUILDER_HOME="%s"\n' "$home_expr"
            printf 'export PATH="%s:$PATH"\n' "$dir_expr"
        } >> "$profile_file"
    fi

    success "Registered PROTOBUILDER_HOME and PATH in ${profile_file}"
}

append_fish_profile() {
    profile_file="$1"
    home_expr="$2"
    dir_expr="$3"
    use_home_bin="$4"

    if [ -z "$PRIMARY_PROFILE" ]; then
        PRIMARY_PROFILE="$profile_file"
    fi

    if [ -f "$profile_file" ]; then
        if [ "$use_home_bin" = "1" ] && grep -Fq 'PROTOBUILDER_HOME/bin' "$profile_file" 2>/dev/null; then
            info "Environment variables already registered in ${profile_file}"
            return 0
        fi
        if grep -Fq "$dir_expr" "$profile_file" 2>/dev/null || grep -Fq "$INSTALL_DIR" "$profile_file" 2>/dev/null; then
            info "Environment variables already registered in ${profile_file}"
            return 0
        fi
    else
        mkdir -p "$(dirname "$profile_file")"
        : > "$profile_file"
    fi

    if [ -s "$profile_file" ]; then
        printf '\n' >> "$profile_file"
    fi

    if [ "$use_home_bin" = "1" ]; then
        {
            printf '# ProtoBuilder\n'
            printf 'set -gx PROTOBUILDER_HOME "%s"\n' "$home_expr"
            printf 'fish_add_path "$PROTOBUILDER_HOME/bin"\n'
        } >> "$profile_file"
    else
        {
            printf '# ProtoBuilder\n'
            printf 'set -gx PROTOBUILDER_HOME "%s"\n' "$home_expr"
            printf 'fish_add_path "%s"\n' "$dir_expr"
        } >> "$profile_file"
    fi

    success "Registered PROTOBUILDER_HOME and PATH in ${profile_file}"
}

register_env() {
    PRIMARY_PROFILE=""
    if [ "$NO_MODIFY_PATH" = "1" ]; then
        info "Skipping shell profile modification (--no-modify-path)."
        return 0
    fi

    pb_home_clean="${PB_HOME%/}"
    install_dir_clean="${INSTALL_DIR%/}"
    home_expr="$(format_home_expr "$pb_home_clean")"
    dir_expr="$(format_home_expr "$install_dir_clean")"

    if [ "$install_dir_clean" = "${pb_home_clean}/bin" ]; then
        use_home_bin="1"
    else
        use_home_bin="0"
    fi

    shell_name="$(basename "${SHELL:-sh}")"
    case "$shell_name" in
        zsh)
            zdot="${ZDOTDIR:-$HOME}"
            append_posix_profile "${zdot%/}/.zshrc" "$home_expr" "$dir_expr" "$use_home_bin"
            ;;
        bash)
            if [ "$OS" = "macos" ]; then
                append_posix_profile "${HOME%/}/.bash_profile" "$home_expr" "$dir_expr" "$use_home_bin"
                if [ -f "${HOME%/}/.bashrc" ]; then
                    append_posix_profile "${HOME%/}/.bashrc" "$home_expr" "$dir_expr" "$use_home_bin"
                fi
            else
                append_posix_profile "${HOME%/}/.bashrc" "$home_expr" "$dir_expr" "$use_home_bin"
                if [ -f "${HOME%/}/.profile" ] && ! grep -Fq '.bashrc' "${HOME%/}/.profile" 2>/dev/null; then
                    append_posix_profile "${HOME%/}/.profile" "$home_expr" "$dir_expr" "$use_home_bin"
                fi
            fi
            ;;
        fish)
            fish_cfg="${XDG_CONFIG_HOME:-${HOME%/}/.config}/fish/config.fish"
            append_fish_profile "$fish_cfg" "$home_expr" "$dir_expr" "$use_home_bin"
            ;;
        *)
            append_posix_profile "${HOME%/}/.profile" "$home_expr" "$dir_expr" "$use_home_bin"
            ;;
    esac
}

# --- Main -------------------------------------------------------------------

main() {
    parse_args "$@"

    INSTALL_DIR="${INSTALL_DIR%/}"
    PB_HOME="${PB_HOME%/}"

    need_cmd curl
    need_cmd tar
    need_cmd mktemp
    need_cmd uname
    need_cmd chmod
    need_cmd mkdir
    need_cmd cp
    need_cmd mv

    detect_platform
    resolve_version

    TMP_DIR="$(mktemp -d)"

    info "Installing ${BIN_NAME} ${VERSION} (${OS}-${ARCH})..."

    if [ "$FROM_SOURCE" = "1" ]; then
        build_from_source
    else
        if ! download_prebuilt; then
            warn "No prebuilt archive found for ${OS}-${ARCH} in ${VERSION}. Falling back to building from source..."
            build_from_source
        fi
    fi

    install_binary
    register_env

    # Verify installed binary
    if "${INSTALL_DIR}/${BIN_NAME}" --version >/dev/null 2>&1; then
        installed_ver="$("${INSTALL_DIR}/${BIN_NAME}" --version)"
        success "Verified: ${installed_ver}"
    else
        warn "Binary installed at ${INSTALL_DIR}/${BIN_NAME}, but '--version' check failed."
    fi

    # Post-install activation hint
    case ":${PATH:-}:" in
        *":${INSTALL_DIR}:"*)
            printf '\nProtoBuilder is ready to use! Run:\n  %s --help\n' "$BIN_NAME"
            ;;
        *)
            printf '\nTo start using %s in your current terminal, run:\n' "$BIN_NAME"
            if [ -n "$PRIMARY_PROFILE" ]; then
                printf '  source "%s"\n' "$PRIMARY_PROFILE"
            else
                printf '  export PROTOBUILDER_HOME="%s"\n' "$PB_HOME"
                printf '  export PATH="%s:$PATH"\n' "$INSTALL_DIR"
            fi
            printf '\nOr open a new terminal window and run:\n  %s --version\n' "$BIN_NAME"
            ;;
    esac
}

main "$@"
