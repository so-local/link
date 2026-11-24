//! Programs for the folder-backed cloud in [`crate::World`]. These are
//! inspectable stand-ins, not implementations of cloud or sandbox security.
//! Each constant is installed under the command name documented beside it.

pub const FILE: &str = r#"schema = 1
name = "app"
version = 1
workdir = "app"

[variables]
DATABASE_URL = "postgres://localhost:5432/app"

[[stage]]
name = "system"
run = "echo system >> stages"
user = "root"

[[stage]]
name = "clone"
run = "git clone https://github.com/me/app app"
sandbox = "clone.toml"

[[stage]]
name = "deps"
run = "echo deps >> stages"
sandbox = "deps.toml"
inputs = ["requirements.txt"]

[[stage]]
name = "pull"
run = "echo pull >> ../stages"
sandbox = "clone.toml"
when = "start"

[[service]]
name = "db"
run = "echo $$ > ../../db.pid; touch ../../db-up; exec sleep 300"
sandbox = "db.toml"
ready = { run = "test -f ../../db-up" }
"#;

/// `lnk-jobcloud`, an adapter for a cloud that runs jobs itself: its
/// images and jobs files under `jobcloud/`, each request kept in
/// `jobcloud.log`. A build says a line and makes the image (or fails,
/// with `jobcloud/fail-build`); a job is running while
/// `jobcloud/hold` is there, then ends as its stages say: each `true`
/// passes, anything else fails with code 3 and some output.
pub const JOBCLOUD: &str = r#"
import json, os, sys
command = sys.argv[1]
request = json.load(sys.stdin)
home = os.path.join(ROOT, "jobcloud")
os.makedirs(os.path.join(home, "images"), exist_ok=True)
os.makedirs(os.path.join(home, "jobs"), exist_ok=True)
with open(os.path.join(ROOT, "jobcloud.log"), "a") as log:
    log.write(command + " " + json.dumps(request) + "\n")
def image(h):
    return {"id": "img-" + h[:6], "name": "app", "state": "ready", "hash": h}
def reply(r):
    print(json.dumps(r))
    sys.exit(0)
def job_path(i):
    return os.path.join(home, "jobs", i + ".json")
if command == "image-build":
    h = request["environment"]["hash"]
    if not os.path.exists(os.path.join(home, "images", h)):
        if os.path.exists(os.path.join(home, "fail-build")):
            print("Error: the build failed at stage deps", file=sys.stderr)
            sys.exit(1)
        print("building app's image " + h, file=sys.stderr)
        open(os.path.join(home, "images", h), "w").close()
    reply({"images": [image(h)]})
if command == "job-run":
    h = request["environment"]["hash"]
    if not os.path.exists(os.path.join(home, "images", h)):
        print("Error: no image of the environment with hash " + h, file=sys.stderr)
        sys.exit(1)
    i = "j%d" % len(os.listdir(os.path.join(home, "jobs")))
    job = {"id": i, "state": "setting-up", "runs": [st["run"] for st in request["job"]["stages"]]}
    json.dump(job, open(job_path(i), "w"))
    reply({"job": {"id": i, "state": "setting-up"}})
if command in ("job-state", "job-stop"):
    i = request["id"]
    job = json.load(open(job_path(i)))
    if command == "job-stop" and job["state"] not in ("passed", "failed"):
        job["state"] = "cancelled"
        job["why"] = "cancelled"
    elif job["state"] in ("setting-up", "running"):
        if os.path.exists(os.path.join(home, "hold")):
            job["state"] = "running"
        elif all(run in ("true", "'true'") for run in job["runs"]):
            job.update(state="passed", exit_code=0, stages=[["pull", 0.5]])
        else:
            job.update(state="failed", exit_code=3, stages=[["pull", 0.5]],
                       output="hello from jobcloud\n\x1b[2Jnot ok 2\n")
    json.dump(job, open(job_path(i), "w"))
    out = {k: v for k, v in job.items() if k != "runs"}
    reply({"job": out})
print("Error: lnk-jobcloud has no command " + json.dumps(command), file=sys.stderr)
sys.exit(1)
"#;

/// `lnk`: boxes are folders, images are copies, and remote commands run
/// the real environment/job programs with a box's own home and identity.
pub(super) fn controller(root: &str, lnk_env: &str) -> String {
    format!(
        r#"root={root}
echo "lnk $*" >> $root/lnk.log
case "$1 $2" in
  "box list")
    if [ -f "$root/fail-inventory-after-create" ] && [ -f "$root/create-attempted" ]; then echo 'local inventory unavailable' >&2; exit 1; fi
    printf '['; sep=''; for b in $(cat $root/boxes 2>/dev/null); do printf '%s{{"name":"%s","account":"aws","kind":"aws","size":"fake-size","price":{{"machine":0.12}},"place":"here","id":"fake"}}' "$sep" "$b"; sep=','; done; echo ']' ;;
  "box image")
    case "$3" in
      list) hash=''; prev=''; for a; do [ "$prev" = --hash ] && hash=$a; prev=$a; done
        if [ -d "$root/images.d/$hash" ]; then echo '[{{"name":"app"}}]'; else echo '[]'; fi ;;
      create) mkdir -p $root/images.d; cp -R "$root/boxes.d/$4/home" "$root/images.d/$7" ;;
    esac ;;
  "box create")
    touch "$root/create-attempted"
    if [ -f "$root/fail-create" ]; then echo 'launch refused' >&2; exit 1; fi
    name=''; prev=''; for a; do [ "$prev" = --name ] && name=$a; prev=$a; done
    echo "$name" >> $root/boxes; mkdir -p "$root/boxes.d/$name"
    if [ -f "$root/fail-after-create" ] || [ -f "$root/fail-inventory-after-create" ]; then echo 'SSH setup refused' >&2; exit 1; fi
    # A box's background services are systemd's, which no test runs:
    # with services-up, its db is up already, as its service would have it.
    if [ -f $root/services-up ]; then touch "$root/boxes.d/$name/db-up"; fi
    hash=''; prev=''; for a; do [ "$prev" = --image-hash ] && hash=$a; prev=$a; done
    if [ -n "$hash" ]; then cp -R "$root/images.d/$hash" "$root/boxes.d/$name/home"; else mkdir -p "$root/boxes.d/$name/home"; fi
    if [ -f "$root/deadline-accelerated" ]; then
      python3 "$root/deadline.py" "$root" "$name" "$@" </dev/null >/dev/null 2>&1 &
    fi ;;
  "box delete") grep -vx "$3" $root/boxes > $root/boxes.new; mv $root/boxes.new $root/boxes; echo "$3" >> $root/removed ;;
  "box stop") echo "$3" >> $root/stopped ;;
  "box copy")
    name=$3; shift 3; for a; do last=$a; done
    mkdir -p "$root/boxes.d/$name/home/$last"
    n=$#; i=1; for a; do [ $i -lt $n ] && cp "$a" "$root/boxes.d/$name/home/$last/"; i=$((i + 1)); done ;;
  "box run")
    name=$3; shift 4
    case "$1 $2" in
      "env here") cd "$root/boxes.d/$name/home" || exit 1
        # Every byte the box is sent, recorded on its way in.
        fifo="$root/fifo-$$"; mkfifo "$fifo"
        exec 3<&0
        tee -a "$root/stream-$name" <&3 >"$fifo" 2>/dev/null 3<&- &
        HOME="$root/boxes.d/$name/home" LNK_EXE=$root/bin/boxlnk LNK_TEST_MACHINE_KIND=$root/box/kind.toml exec {lnk_env} here <"$fifo" 3<&- ;;
      "job handoff") cd "$root/boxes.d/$name/home" || exit 1
        HOME="$root/boxes.d/$name/home" LNK_EXE=$root/bin/boxlnk LNK_TEST_MACHINE_KIND=$root/box/kind.toml exec "$LNK_TEST_JOB_BIN" handoff ;;
      "job output") cd "$root/boxes.d/$name/home" || exit 1
        HOME="$root/boxes.d/$name/home" exec "$LNK_TEST_JOB_BIN" output show "$4" ;;
      "job fetch"|"job end") cd "$root/boxes.d/$name/home" || exit 1
        shift; HOME="$root/boxes.d/$name/home" exec "$LNK_TEST_JOB_BIN" "$@" ;;
      *) echo "$name: lnk $*" >> $root/box-lnk.log ;;
    esac ;;
  "box ssh") name=$3; cd "$root/boxes.d/$name/home" && HOME="$root/boxes.d/$name/home" exec sh -c "$5" ;;
  "env here") cd "$HOME" && LNK_EXE=$root/bin/boxlnk exec {lnk_env} here ;;
  "box check") printf '{{"kind":"plan","want":%s}}' "$(cat "$root/want.json")" ;;
  "cloud list") printf '['; sep=''; for f in "$root"/clouds.d/*.json; do printf '%s' "$sep"; cat "$f"; sep=','; done; echo ']' ;;
  "cloud env") if [ -f "$root/clouds.d/$3.json" ]; then cat "$root/clouds.d/$3.json"; else echo "no account $3" >&2; exit 1; fi ;;
  *) echo "lnk: no $*" >&2; exit 1 ;;
esac"#
    )
}

/// `boxlnk`: records sandbox inputs with secrets blanked, then runs the command.
pub(super) const BOX: &str = r#"case "$1 $2" in
  "sandbox run")
    shift 2; spec=none; fd=''
    while [ $# -gt 0 ]; do case "$1" in --) shift; break ;; --spec) spec=$2; shift 2 ;; --with-fd) fd=$2; shift 2 ;; --files.write|--files.write_dot_files|--internet.hosts) shift 2 ;; *) shift ;; esac; done
    mkdir -p "$HOME/../with"; n=$$
    if [ -n "$fd" ]; then python3 -c 'import json, sys
w = json.load(sys.stdin)
for s in w.get("secrets", []):
    s["secret"]["value"] = "***"
json.dump(w, sys.stdout)' <&3 > "$HOME/../with/$n.json"; else echo '{}' > "$HOME/../with/$n.json"; fi
    echo "$(basename "$spec") $*" >> "$HOME/../sandboxes"
    exec python3 -c 'import json, os, sys
w = json.load(open(sys.argv[1]))
env = dict(os.environ)
env.update(w.get("env", {}))
for s in w.get("secrets", []):
    env[s["url"]] = "http://127.0.0.1:40781/_link/secret/" + s["secret"]["name"]
    env[s["placeholder"]] = "link-wire-placeholder"
os.execvpe(sys.argv[2], sys.argv[2:], env)' "$HOME/../with/$n.json" "$@" ;;
  "box end-here") exec "$(dirname "$0")/sudo" -n -- /sbin/poweroff ;;
  "env here") exec "$LNK_TEST_ENV_BIN" here ;;
  "job handoff") exec "$LNK_TEST_JOB_BIN" handoff ;;
  "job worker") shift; exec "$LNK_TEST_JOB_BIN" "$@" ;;
  *) echo "box lnk: no $*" >&2; exit 1 ;;
esac"#;

/// `sudo`: records root commands, fakes poweroff and cancels fake systemd jobs.
pub(super) const SUDO: &str = r#"while [ "$1" != -- ]; do shift; done; shift
echo "$*" >> "$HOME/../as-root"
case "$1" in
  /sbin/poweroff)
    root=$(dirname "$0")/..; name=$(basename "$(dirname "$HOME")")
    grep -vx "$name" "$root/boxes" > "$root/boxes.new"; mv "$root/boxes.new" "$root/boxes"
    echo "$name" >> "$root/removed"; exit 0 ;;
  systemctl)
    id=${3#lnk-job-}; id=${id%.service}
    kill -TERM "$(cat "$HOME/.lnk/safe/envs/jobs/$id/unit.pid")"
    n=0; while [ ! -f "$HOME/.lnk/safe/envs/jobs/$id/record.json" ] || ! grep -q cancelled "$HOME/.lnk/safe/envs/jobs/$id/record.json"; do
      n=$((n+1)); [ "$n" -lt 100 ] || exit 1; sleep 0.1
    done
    exit 0 ;;
esac
exec "$@""#;

/// `git`: makes a clone directory without reaching a remote repository.
pub(super) const GIT: &str = r#"for a; do last=$a; done
mkdir -p "$last/.git"; echo cloned > "$last/README""#;

/// `lnk-aws`: advertises one size and no native job support, refusing
/// any other command as a generated command line does.
pub(super) const AWS: &str = r#"echo "lnk-aws $1" >> "$(dirname "$0")/../adapters.log"
if [ "$1" = sizes ]; then echo '{"sizes":[{"size":"fake-size","cpus":2,"memory_mb":2048,"arch":"x86_64","price":0.12}]}'; exit 0; fi
printf "error: unrecognized subcommand '%s'\n\n  tip: some similar subcommands exist: 'sizes'\n\nUsage: lnk aws [COMMAND]\n\nFor more information, try '--help'.\n" "$1" >&2; exit 2"#;

/// `lnk-store`: reads bucket files and signs upload URLs for ResultServer.
pub(super) fn store(root: &str) -> String {
    format!(
        r#"root={root}
if [ "$1" = upload-once ] && [ -f "$root/fail-results" ]; then echo 'result signing refused' >&2; exit 1; fi
exec python3 -c 'import json,os,sys
root,command=sys.argv[1:3]
request=json.load(sys.stdin)
path=request["bucket"]+"/"+request["object"]
if command=="object-read":
    found=os.path.join(root,"bucket",path)
    print(json.dumps({{"contents":open(found).read()}} if os.path.exists(found) else {{}}))
elif command=="upload-once":
    url=open(os.path.join(root,"results-url")).read()
    print(json.dumps({{"upload":{{"url":url+"/"+path,"headers":{{"if-none-match":"*"}}}}}}))
else:
    sys.exit("Error: lnk-store has no command "+json.dumps(command))' "$root" "$1""#
    )
}

/// `systemd-run`: starts a detached job and publishes its unit PID.
pub(super) const SYSTEMD_RUN: &str = r#"#!/bin/sh
echo "$*" >> "$HOME/../systemd-run.args"
while [ "${1#--}" != "$1" ]; do shift; done
eval "job=\${$(($# - 1))}"
setsid "$@" </dev/null >/dev/null 2>>"$HOME/unit.log" &
echo $! > "$HOME/.lnk/safe/envs/jobs/$job/unit.pid"
"#;
