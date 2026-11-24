//! The probes' scripts: each a line or few of bash, run inside, which
//! exits 0 when it reached what it tried.

/// `$auth`: the `Proxy-Authorization` a `CONNECT` carries, from the run's
/// secret in `$HTTPS_PROXY` (`http://lnk:<secret>@127.0.0.1:<port>`),
/// where it names one; else nothing. A script that sends a `CONNECT`
/// starts with it.
macro_rules! authed {
    ($script:literal) => {
        concat!(
            r#"auth=""
case "$HTTPS_PROXY" in *@*) pair=${HTTPS_PROXY#http://}; pair=${pair%@*}
  auth="Proxy-Authorization: Basic $(printf %s "$pair" | base64 | tr -d '\n')\r\n" ;; esac
"#,
            $script
        )
    };
}

/// A CONNECT through the sandbox's proxy (`$HTTPS_PROXY`) to the bait's
/// server: reached only when the proxy answers 200.
pub(super) const PROXY_CONNECT_LOCALHOST: &str = authed!(
    r#"exec 3<>"/dev/tcp/127.0.0.1/${HTTPS_PROXY##*:}"
printf 'CONNECT 127.0.0.1:%s HTTP/1.1\r\n%b\r\n' "$LNK_PROBE_PORT" "$auth" >&3
head -c 12 <&3 | grep -q ' 200'"#
);
pub(super) const PROXY_CONNECT_LAN: &str = authed!(
    r#"exec 3<>"/dev/tcp/127.0.0.1/${HTTPS_PROXY##*:}"
printf 'CONNECT %s:%s HTTP/1.1\r\n%b\r\n' "$LNK_PROBE_LAN" "$LNK_PROBE_PORT" "$auth" >&3
head -c 12 <&3 | grep -q ' 200'"#
);

/// Asks the model runtime on `$LNK_PROBE_ARG` to delete a model that doesn't
/// exist (harmless where it gets through): reached unless Link's filter
/// answers 403.
pub(super) const MODEL_ADMIN: &str = r#"exec 3<>"/dev/tcp/127.0.0.1/$LNK_PROBE_ARG" || exit 1
body='{"model":"lnk-check-no-such-model"}'
printf 'DELETE /api/delete HTTP/1.1\r\nHost: localhost\r\nContent-Length: %s\r\n\r\n%s' "${#body}" "$body" >&3
! head -n 1 <&3 | grep -q ' 403'"#;

/// The same, on its proxy's `model` verb for the runtime on `$LNK_PROBE_ARG`,
/// at the address the sandbox was given (`$LNK_LOCAL_MODEL_<port>`).
pub(super) const MODEL_ADMIN_PROXIED: &str = r#"exec 3<>"/dev/tcp/127.0.0.1/${HTTPS_PROXY##*:}" || exit 1
eval "url=\${LNK_LOCAL_MODEL_$LNK_PROBE_ARG}"
body='{"model":"lnk-check-no-such-model"}'
printf 'DELETE /%s/api/delete HTTP/1.1\r\nHost: localhost\r\nContent-Length: %s\r\n\r\n%s' "${url#http://*/}" "${#body}" "$body" >&3
! head -n 1 <&3 | grep -q ' 403'"#;

/// Reaching `$LNK_PROBE_ARG` (`host:port`): through its proxy, which answered
/// anything but its 403 refusal; without one, directly.
pub(super) const TARGET_REACHED: &str = authed!(
    r#"if [ -n "$HTTPS_PROXY" ]; then
exec 3<>"/dev/tcp/127.0.0.1/${HTTPS_PROXY##*:}" || exit 1
printf 'CONNECT %s HTTP/1.1\r\n%b\r\n' "$LNK_PROBE_ARG" "$auth" >&3
! head -c 12 <&3 | grep -q ' 403'
else
host=${LNK_PROBE_ARG%:*}; host=${host#[}; host=${host%]}
timeout 5 bash -c 'exec 3<>"/dev/tcp/$1/$2"' _ "$host" "${LNK_PROBE_ARG##*:}"
fi"#
);

/// The same, for a name with no address here: reached only when its
/// proxy connected (200). Its 502 for a name it can't resolve dialed
/// nothing.
pub(super) const TARGET_CONNECTED: &str = authed!(
    r#"if [ -n "$HTTPS_PROXY" ]; then
exec 3<>"/dev/tcp/127.0.0.1/${HTTPS_PROXY##*:}" || exit 1
printf 'CONNECT %s HTTP/1.1\r\n%b\r\n' "$LNK_PROBE_ARG" "$auth" >&3
head -c 12 <&3 | grep -q ' 200'
else
host=${LNK_PROBE_ARG%:*}; host=${host#[}; host=${host%]}
timeout 5 bash -c 'exec 3<>"/dev/tcp/$1/$2"' _ "$host" "${LNK_PROBE_ARG##*:}"
fi"#
);

/// Its DNS answering a query, `$LNK_PROBE_ARG` its type's number and the name
/// (`16 example.net`): reached unless refused (any answer but `NOERROR`),
/// built and read by the shell alone.
pub(super) const DNS_ANSWERED: &str = r#"set -- $LNK_PROBE_ARG
query='\x4c\x6b\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00'
for label in $(printf %s "$2" | tr . ' '); do
  query="$query$(printf '\\x%02x' "${#label}")$label"
done
query="$query\x00\x00$(printf '\\x%02x' "$1")\x00\x01"
exec 3<>/dev/udp/127.0.0.1/53 || exit 1
printf "$query" >&3
rcode=$(timeout 3 head -c 4 <&3 | od -An -tu1 | awk '{print $4 % 16}')
[ "$rcode" = 0 ]"#;

/// Its proxy taking `$LNK_PROBE_ARG` (`host:port`): connected (200), allowed and
/// not answering here (502, 504), or still dialing after 3 seconds (its
/// refusals come before it dials).
pub(super) const TARGET_TAKEN: &str = authed!(
    r#"exec 3<>"/dev/tcp/127.0.0.1/${HTTPS_PROXY##*:}" || exit 1
printf 'CONNECT %s HTTP/1.1\r\n%b\r\n' "$LNK_PROBE_ARG" "$auth" >&3
IFS= read -r -t 3 line <&3
code=$?
if [ "$code" = 0 ]; then
  case "$line" in "HTTP/1.1 2"* | "HTTP/1.1 5"*) exit 0 ;; *) exit 1 ;; esac
fi
[ "$code" -gt 128 ]"#
);

/// Running `$LNK_PROBE_ARG`: anything but the shell's "can't run it" (126) or
/// "not found" (127).
pub(super) const PROGRAM_RAN: &str = r#""$LNK_PROBE_ARG" --version </dev/null >/dev/null 2>&1
code=$?
[ "$code" != 126 ] && [ "$code" != 127 ]"#;

/// A CONNECT through its proxy to `$LNK_PROBE_ARG` (`host:port`): anything but
/// the proxy's 403 refusal counts as reached.
pub(super) const PROXY_CONNECT_ARG: &str = authed!(
    r#"exec 3<>"/dev/tcp/127.0.0.1/${HTTPS_PROXY##*:}"
printf 'CONNECT %s HTTP/1.1\r\n%b\r\n' "$LNK_PROBE_ARG" "$auth" >&3
! head -c 12 <&3 | grep -q ' 403'"#
);

/// A CONNECT through its proxy to a host not on its list (`$LNK_PROBE_UNLISTED`):
/// anything but the proxy's 403 refusal counts as reached.
pub(super) const PROXY_CONNECT_UNLISTED: &str = authed!(
    r#"exec 3<>"/dev/tcp/127.0.0.1/${HTTPS_PROXY##*:}"
printf 'CONNECT %s:443 HTTP/1.1\r\n%b\r\n' "$LNK_PROBE_UNLISTED" "$auth" >&3
! head -c 12 <&3 | grep -q ' 403'"#
);

/// Makes the system call `$LNK_PROBE_ARG` (with harmless arguments) and exits
/// 0 only if the kernel ran it.
pub(super) const KERNEL_PROBE: &str = r#"python3 -c '
import ctypes, os, platform
x86 = platform.machine() == "x86_64"
calls = {
    "unshare": (272 if x86 else 97, 0x10000000),
    "io_uring_setup": (425, 4, ctypes.create_string_buffer(120)),
    "userfaultfd": (323 if x86 else 282, 1),
    "keyctl": (250 if x86 else 219, 0, -3, 0),
    "ptrace": (101 if x86 else 117, 0, 0, 0, 0),
}
nr, *args = calls[os.environ["LNK_PROBE_ARG"]]
raise SystemExit(0 if ctypes.CDLL(None).syscall(nr, *args) >= 0 else 1)
'"#;
