//! A recorded stdio MCP session over the real file-tool server.

use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Stdio};
use std::time::Duration;

use link_test_kit::{ChildGroup, env, wait};
use serde_json::{Value, json};

pub struct Session {
    child: ChildGroup,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    id: u64,
}

impl Session {
    pub fn new(home: &std::path::Path, apply: &[String]) -> Self {
        let mut command = env::command(env!("CARGO_BIN_EXE_lnk-file-tool"), home);
        command
            .args(["serve", "--"])
            .args(apply)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child = ChildGroup::spawn(&mut command).unwrap();
        let input = child.take_stdin();
        let output = BufReader::new(child.take_stdout().unwrap());
        Self {
            child,
            input,
            output,
            id: 0,
        }
    }

    pub fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.id += 1;
        let message = json!({"jsonrpc":"2.0", "id":self.id, "method":"tools/call",
            "params":{"name":name,"arguments":arguments}});
        writeln!(self.input.as_mut().unwrap(), "{message}").unwrap();
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["jsonrpc"], "2.0");
        assert_eq!(reply["id"], self.id);
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    }

    pub fn finish(mut self) {
        drop(self.input.take());
        let status = wait::eventually("file server exits at EOF", Duration::from_secs(5), || {
            self.child.try_status().unwrap()
        });
        assert!(status.success());
    }
}
