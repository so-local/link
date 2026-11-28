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
#   LNK_VERSION       a specific version such as 0.0.1 (default: latest)
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
# major.minor.patch, then a prerelease (0.0.1-staging.202610131900) before its
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

# Downloads $1 from the release into $tmp. Returns curl's status: 22 when
# the server answered an error, or 37 when a file:// release's folder
# holds no such file (the release has none), another when the network
# didn't answer.
download() {
    status=0
    curl -fsSL --retry 3 "$base/$1" -o "$tmp/$1" 2>"$tmp/curl.err" || status=$?
    return "$status"
}

# Says why downloading $1 failed with curl's status $2, then stops.
failed() {
    case "$2" in
        22 | 37) echo "lnk: the release at $base has no $1 ($(tail -n 1 "$tmp/curl.err"))" >&2 ;;
        *) echo "lnk: couldn't download $1 from $base: $(tail -n 1 "$tmp/curl.err") (curl exit $2); check your connection and try again" >&2 ;;
    esac
    exit 1
}

# Downloads $1 from the release into $tmp, and checks it against the signed
# SHA256SUMS.
fetch() {
    download "$1" || failed "$1" "$?"
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

# The line that puts $1 on the PATH of the shell this user logs in with,
# in that shell's own file: fish's own command, bash's login file on macOS
# (Terminal starts a login shell) and .bashrc elsewhere, zsh's .zshrc, and
# .profile for any other shell.
path_hint() {
    case "${SHELL:-}" in
        */fish) echo "  fish_add_path $1" ;;
        */bash)
            case "$(uname -s)" in
                Darwin) rc=.bash_profile ;;
                *) rc=.bashrc ;;
            esac
            echo "  echo 'export PATH=\"$1:\$PATH\"' >> ~/$rc && exec bash"
            ;;
        */zsh) echo "  echo 'export PATH=\"$1:\$PATH\"' >> ~/.zshrc && exec zsh" ;;
        *) echo "  echo 'export PATH=\"$1:\$PATH\"' >> ~/.profile, then log in again" ;;
    esac
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
    # lnk installs its modules beside itself, so its folder is one this
    # user writes: said before anything is downloaded.
    if ! mkdir -p "$dir" 2>/dev/null || ! [ -w "$dir" ]; then
        echo "lnk: you can't write to $dir, where lnk would install itself and its modules" >&2
        echo "     set LNK_INSTALL_DIR to a folder you own, or leave it out for ~/.lnk/exec/bin" >&2
        exit 1
    fi

    tmp=$(mktemp -d)
    # The downloaded lnk runs from a folder beside where it goes, which can
    # run programs, as a /tmp mounted noexec can't.
    unpacked=$(mktemp -d "$dir/.lnk-install.XXXXXX")
    trap 'rm -rf "$tmp" "$unpacked"' EXIT
    # Interrupted, it stops, and the EXIT trap removes what it downloaded.
    trap 'exit 130' INT
    trap 'exit 143' TERM
    echo "Downloading lnk ($version, $target)"
    download SHA256SUMS || failed SHA256SUMS "$?"
    download SHA256SUMS.sig || {
        status=$?
        if [ "$status" = 22 ] || [ "$status" = 37 ]; then
            echo "lnk: this release is not signed (it has no SHA256SUMS.sig); not installing" >&2
            exit 1
        fi
        failed SHA256SUMS.sig "$status"
    }
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

    tar -xzf "$tmp/$asset" -C "$unpacked"
    chmod 755 "$unpacked/lnk"
    # The lnk inside is signed, so the version it reports is the one it is.
    if ! said=$("$unpacked/lnk" --version 2>"$tmp/version.err"); then
        echo "lnk: the downloaded lnk didn't run in $dir ($(tail -n 1 "$tmp/version.err")); not installing it" >&2
        exit 1
    fi
    found_version=$(echo "$said" | awk 'NR == 1 { print $2 }')
    if [ "$found_version" != "$expected_version" ]; then
        echo "lnk: the release is lnk $expected_version but holds lnk ${found_version:-(no version)}; not installing it" >&2
        exit 1
    fi
    # lnk, which runs its modules from the same folder. Replaced by rename,
    # so a running lnk is safe.
    mv -f "$unpacked/lnk" "$dir/lnk"
    echo "Installed $("$dir/lnk" --version | head -n 1) to $dir/lnk"
    # Modules an older install left are its release's until upgraded.
    other_modules=''
    for path in "$dir"/lnk-*; do
        [ -f "$path" ] || continue
        module_version=$("$path" --version 2>/dev/null | awk 'NR == 1 { print $2 }')
        [ "$module_version" = "$expected_version" ] || other_modules="$other_modules $(basename "$path")"
    done

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
            echo
            echo "$dir is not on your PATH. Add it, for your shell:"
            path_hint "$dir"
            ;;
    esac
    found=$(command -v lnk 2>/dev/null || true)
    if [ -n "$found" ] && [ "$found" != "$dir/lnk" ]; then
        echo
        echo "Note: \`lnk\` currently runs $found, not $dir/lnk."
        echo "Remove the other one or put $dir first on your PATH."
    fi
    echo
    if [ -n "$other_modules" ]; then
        echo "Your modules are another release's:$other_modules."
        echo "Next: lnk upgrade (brings them, and your files, to lnk $expected_version)"
    else
        echo "Next: lnk up (choose what you want Link for)"
    fi
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
