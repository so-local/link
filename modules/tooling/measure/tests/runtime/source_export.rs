//! Retained job export and real local OTLP acknowledgements.
use super::*;
#[test]
fn a_selected_retained_job_is_exported_once_and_receiver_errors_are_refused() {
    use link_measure_contract::{Point, Report, Unit, otlp};
    use link_test_kit::{ChildGroup, gate, net::Held, wait};
    use std::time::Duration;
    if !gate::program("python3", &["--version"])
        || !gate::program(
            "python3",
            &[
                "-I",
                "-c",
                "import socket; s=socket.socket(); s.bind(('127.0.0.1',0)); s.close()",
            ],
        )
    {
        return;
    }
    let w = World::new();
    if !w.trusted_home() {
        return;
    }
    let path = w.root.join("export-profile.jsonl");
    let samples: Vec<_> = [
        ("job", "a1", None),
        ("stage", "a1/build", Some("a1")),
        ("job", "other", None),
    ]
    .into_iter()
    .map(|(kind, id, parent)| Report {
        unit: Unit {
            id: id.into(),
            kind: kind.into(),
            parent: parent.map(str::to_string),
            ..Unit::default()
        },
        points: vec![Point::new("link.cpu.used.sum", 2.5, 100)],
    })
    .collect();
    std::fs::write(&path, otlp(&samples, "lnk-job").to_string()).unwrap();
    std::fs::write(
        w.root.join("modules.json"),
        json!([{
            "name":"job", "installed":true, "answers":["measure.job"]
        }])
        .to_string(),
    )
    .unwrap();
    std::fs::write(w.root.join("profile-path.json"), json!(path).to_string()).unwrap();
    let before = std::fs::read(&path).unwrap();
    for status in [200, 500] {
        let listener = Held::loopback();
        let url = format!("http://{}", listener.address());
        let ready = w.root.join(format!("collector-{status}.ready"));
        let captured = w.root.join(format!("collector-{status}.json"));
        let mut command = env::command("python3", &w.home);
        command.args(["-I", "-c", r#"
import http.server,json,os,pathlib,socket,sys
class Receiver(http.server.BaseHTTPRequestHandler):
    def log_message(self,*args): pass
    def do_POST(self):
        body=self.rfile.read(int(self.headers['Content-Length']))
        pathlib.Path(sys.argv[2]).write_text(json.dumps({'path':self.path,'body':json.loads(body)}))
        self.send_response(int(sys.argv[3]));self.send_header('Content-Length','0');self.end_headers()
server=http.server.HTTPServer(('127.0.0.1',0),Receiver,bind_and_activate=False)
server.socket=socket.socket(fileno=int(os.environ['LNK_TEST_LISTENER']))
server.server_address=server.socket.getsockname()
pathlib.Path(sys.argv[1]).write_text('ready')
with server: server.handle_request()
"#]).arg(&ready).arg(&captured).arg(status.to_string());
        listener.hand_to(&mut command, "LNK_TEST_LISTENER");
        let collector = ChildGroup::spawn(&mut command).unwrap();
        drop(command);
        wait::eventually(
            "owned OTLP collector is ready",
            Duration::from_secs(10),
            || ready.is_file(),
        );
        let configured = measure(&w, &["export", "set", "--url", &url]);
        assert!(configured.status.success(), "{configured:?}");
        let exported = measure(&w, &["export", "set", "--job", "a1"]);
        assert_eq!(exported.status.success(), status == 200, "{exported:?}");
        assert!(
            wait::eventually(
                "collector acknowledges the whole job profile",
                Duration::from_secs(10),
                || collector.try_status().unwrap()
            )
            .success()
        );
        let captured: Value = serde_json::from_slice(&std::fs::read(&captured).unwrap()).unwrap();
        assert_eq!(captured["path"], "/v1/metrics");
        let samples = link_measure_contract::client::read_otlp(&captured["body"]);
        let ids: std::collections::BTreeSet<_> =
            samples.iter().map(|s| s.unit.id.as_str()).collect();
        assert_eq!(ids, ["a1", "a1/build"].into_iter().collect());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
