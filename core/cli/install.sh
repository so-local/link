#!/bin/sh
# Installs lnk, the Link CLI, from the public releases.
#
#   curl -fsSL https://local.link/install.sh | sh
#
# From a checkout: sh core/cli/install.sh --source
# Requires Rust and Cargo. Installs only the CLI; lnk up installs modules.
#
# Environment:
#   LNK_INSTALL_DIR   where to put lnk (default: ~/.lnk/exec/bin)
#   LNK_VERSION       a specific version such as 0.14.0 (default: latest)
#   LNK_INSECURE=1    install even without ssh-keygen (no signature check)
#
# Releases are signed: this script checks SHA256SUMS.sig with ssh-keygen
# (OpenSSH 8.1+, on macOS and Linux) against the key below before trusting
# any checksum. `lnk upgrade` does the same check itself. The website
# serves this script from its own trusted build, independently of the
# download repository and public tunnel traffic.
set -eu

# The website or sealed release's version: the latest release, when no
# version is asked for, is never older. Empty in a source checkout.
MIN_VERSION=''

# Keep in sync with scripts/release/link-signers (a test checks).
RELEASE_SIGNERS='link-release namespaces="link-release" ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIM3FwsOmJLMiqN+wwSasZuK5mkkjM0rpC5P/UxdQRjmV link-release'

# Whether the signed SHA256SUMS lists the file $1.
listed() {
    awk -v asset="$1" '$2 == asset { found = 1 } END { exit !found }' "$tmp/SHA256SUMS"
}

# Whether version $1 comes before version $2, as lnk orders them: by
# major.minor.patch, then a prerelease (0.15.0-staging.202610131900) before its
# release, and prereleases part by part, numbers as numbers before words.
older() {
    LC_ALL=C awk -v a="$1" -v b="$2" 'BEGIN {
        ap = index(a, "-"); bp = index(b, "-")
        an = ap ? substr(a, 1, ap - 1) : a; apre = ap ? substr(a, ap + 1) : ""
        bn = bp ? substr(b, 1, bp - 1) : b; bpre = bp ? substr(b, bp + 1) : ""
        split(an, x, "."); split(bn, y, ".")
        for (i = 1; i <= 3; i++) {
            if (x[i] + 0 < y[i] + 0) exit 0
            if (x[i] + 0 > y[i] + 0) exit 1
        }
        if (apre == bpre || apre == "") exit 1
        if (bpre == "") exit 0
        n = split(apre, p, "."); m = split(bpre, q, ".")
        for (i = 1; i <= n && i <= m; i++) {
            pn = p[i] ~ /^[0-9]+$/; qn = q[i] ~ /^[0-9]+$/
            if (pn && qn) {
                if (length(p[i]) != length(q[i])) exit (length(p[i]) < length(q[i])) ? 0 : 1
                if ((p[i] "") != (q[i] "")) exit ((p[i] "") < (q[i] "")) ? 0 : 1
                continue
            }
            if (pn != qn) exit pn ? 0 : 1
            # As strings, never numbers (a word such as 1e5).
            if ((p[i] "") != (q[i] "")) exit ((p[i] "") < (q[i] "")) ? 0 : 1
        }
        exit (n < m) ? 0 : 1
    }'
}

# Downloads $1 from the release into $tmp, and checks it against the signed
# SHA256SUMS.
fetch() {
    curl -fsSL --retry 3 "$base/$1" -o "$tmp/$1"
    expected=$(awk -v asset="$1" '$2 == asset { print $1 }' "$tmp/SHA256SUMS")
    if command -v sha256sum >/dev/null 2>&1; then
        actual=$(sha256sum "$tmp/$1" | awk '{ print $1 }')
    else
        actual=$(shasum -a 256 "$tmp/$1" | awk '{ print $1 }')
    fi
    if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
        echo "lnk: checksum mismatch for $1; not installing" >&2
        exit 1
    fi
}

main() {
    releases="${LNK_RELEASES_URL:-https://github.com/so-local/link-releases/releases}"
    version="${LNK_VERSION:-latest}"
    if [ "$version" = latest ]; then
        base="$releases/latest/download"
    else
        base="$releases/download/v${version#v}"
    fi

    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64) target=aarch64-apple-darwin ;;
        Darwin-x86_64) target=x86_64-apple-darwin ;;
        Linux-x86_64) target=x86_64-unknown-linux-musl ;;
        Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-musl ;;
        *)
            echo "lnk: no prebuilt binary for $(uname -s) $(uname -m)." >&2
            echo "     Build from source instead: cargo install --path core/cli, then each module's folder" >&2
            exit 1
            ;;
    esac
    dir="${LNK_INSTALL_DIR:-$HOME/.lnk/exec/bin}"

    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT INT TERM
    echo "Downloading lnk ($version, $target)"
    curl -fsSL --retry 3 "$base/SHA256SUMS" -o "$tmp/SHA256SUMS"
    if ! curl -fsSL --retry 3 "$base/SHA256SUMS.sig" -o "$tmp/SHA256SUMS.sig"; then
        echo "lnk: this release is not signed; not installing" >&2
        exit 1
    fi
    if command -v ssh-keygen >/dev/null 2>&1; then
        echo "$RELEASE_SIGNERS" > "$tmp/signers"
        if ! ssh-keygen -Y verify -f "$tmp/signers" -I link-release -n link-release \
                -s "$tmp/SHA256SUMS.sig" < "$tmp/SHA256SUMS" >/dev/null 2>&1; then
            echo "lnk: the release signature did not verify; not installing" >&2
            echo "     (it needs OpenSSH 8.1 or newer: ssh -V)" >&2
            exit 1
        fi
    elif [ "${LNK_INSECURE:-}" = 1 ]; then
        echo "lnk: ssh-keygen not found; LNK_INSECURE=1, so installing without"
        echo "     checking the release signature (HTTPS and the checksum only)."
    else
        echo "lnk: ssh-keygen (OpenSSH) is needed to check the release signature;" >&2
        echo "     install OpenSSH, or set LNK_INSECURE=1 to skip the check" >&2
        exit 1
    fi

    # The release's signed VERSION must be the version asked for: someone
    # with the releases repo could otherwise put an old signed release
    # under this tag.
    if ! listed VERSION; then
        echo "lnk: the release doesn't say which version it is (no VERSION); not installing it" >&2
        exit 1
    fi
    fetch VERSION
    expected_version=$(tr -d ' \t\r\n' < "$tmp/VERSION")
    expected_version=${expected_version#v}
    if [ "$version" != latest ] && [ "$expected_version" != "${version#v}" ]; then
        echo "lnk: the release under v${version#v} is lnk $expected_version; not installing it" >&2
        exit 1
    fi
    # Someone with the releases repo could point `latest` at an older
    # signed release, one with a known hole. Right after a release, the
    # website can be up before its release is.
    if [ "$version" = latest ] && [ -n "$MIN_VERSION" ] && older "$expected_version" "$MIN_VERSION"; then
        echo "lnk: the latest release is lnk $expected_version, behind this installer's $MIN_VERSION; not installing it" >&2
        echo "     (a release being published: try again in a few minutes, or set LNK_VERSION)" >&2
        exit 1
    fi

    # lnk alone: `lnk up` adds the modules you want.
    asset="lnk-core-$target.tar.gz"
    if ! listed "$asset"; then
        echo "lnk: lnk $expected_version has no build for $(uname -s) $(uname -m) ($target); a newer release may" >&2
        exit 1
    fi
    fetch "$asset"

    mkdir "$tmp/unpacked"
    tar -xzf "$tmp/$asset" -C "$tmp/unpacked"
    # The lnk inside is signed, so the version it reports is the one it is.
    found_version=$("$tmp/unpacked/lnk" --version 2>/dev/null | awk 'NR == 1 { print $2 }')
    if [ "$found_version" != "$expected_version" ]; then
        echo "lnk: the release is lnk $expected_version but holds lnk ${found_version:-(no version)}; not installing it" >&2
        exit 1
    fi
    mkdir -p "$dir"
    # lnk (and, from an older release, its modules), which run from the
    # same folder. Replace via rename so a running lnk is safe.
    for path in "$tmp"/unpacked/lnk "$tmp"/unpacked/lnk-*; do
        [ -f "$path" ] || continue
        name=$(basename "$path")
        cp "$path" "$dir/.$name.new"
        chmod 755 "$dir/.$name.new"
        mv -f "$dir/.$name.new" "$dir/$name"
    done
    echo "Installed $("$dir/lnk" --version | head -n 1) to $dir/lnk"

    # The same directory written with a literal ~ (e.g. PATH="~/.lnk/exec/bin:..."
    # in a profile): shells don't expand ~ inside quotes, so it never matches.
    tilde="~${dir#"$HOME"}"
    case ":$PATH:" in
        *":$dir:"*) ;;
        *":$tilde:"*)
            echo
            echo "Your PATH contains \"$tilde\" with a literal ~, which the shell does not"
            echo "expand inside quotes, so lnk won't be found. In your shell profile, write"
            echo "  export PATH=\"\$HOME${dir#"$HOME"}:\$PATH\""
            echo "instead of ~${dir#"$HOME"}, then open a new terminal."
            ;;
        *)
            # The shell this user logs in with: its own file, and itself
            # started again.
            case "${SHELL:-}" in
                */bash) rc=.bashrc shell=bash ;;
                *) rc=.zshrc shell=zsh ;;
            esac
            echo
            echo "$dir is not on your PATH. Add it, for $shell:"
            echo "  echo 'export PATH=\"$dir:\$PATH\"' >> ~/$rc && exec $shell"
            ;;
    esac
    found=$(command -v lnk 2>/dev/null || true)
    if [ -n "$found" ] && [ "$found" != "$dir/lnk" ]; then
        echo
        echo "Note: \`lnk\` currently runs $found, not $dir/lnk."
        echo "Remove the other one or put $dir first on your PATH."
    fi
    echo
    echo "Next: lnk up (choose what you want Link for)"
}

case "${1:-}" in
    --source)
        if [ "$#" -gt 2 ]; then
            echo "Usage: sh install.sh --source [checkout]" >&2
            exit 2
        fi
        command -v cargo >/dev/null 2>&1 || {
            echo "lnk: source installation needs Rust and Cargo: https://rustup.rs" >&2
            exit 1
        }
        source_dir="${2:-$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)}"
        source_dir=$(CDPATH='' cd -- "$source_dir" && pwd)
        exec cargo install --locked --force --path "$source_dir/core/cli" --bin lnk --target-dir "$source_dir/target"
        ;;
    "") main ;;
    *) echo "Usage: sh install.sh [--source [checkout]]" >&2; exit 2 ;;
esac
