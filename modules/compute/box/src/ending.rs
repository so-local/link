//! End a disposable machine by the root-owned policy installed at first boot.

use anyhow::ensure;
use link_cloud_contract::ending::Policy;
use link_format::WithinExt;
use serde::Deserialize;
use std::{path::Path, time::Duration};

pub fn here() -> anyhow::Result<()> {
    ensure!(
        link_machine::machine::kind()?.is_box(),
        "only a disposable box can end itself"
    );
    let test = link_secretive::test_var("LNK_TEST_BOX_END");
    let default = link_files::layout::System::BoxEnd.path();
    let path = test.as_deref().map(Path::new).unwrap_or(&default);
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            use std::os::unix::fs::MetadataExt;
            let owner = if test.is_some() {
                unsafe { libc::geteuid() }
            } else {
                0
            };
            ensure!(
                meta.is_file() && meta.uid() == owner && meta.mode() & 0o022 == 0,
                "the box ending policy must be root-owned and not writable by others"
            );
            let policy = serde_json::from_slice(&std::fs::read(path)?)?;
            remove(&policy)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let sudo = link_secretive::test_var("LNK_SUDO").unwrap_or_else(|| "sudo".into());
            ensure!(
                link_secretive::process::command(sudo)
                    .args(["-n", "--", "/sbin/poweroff"])
                    .status()?
                    .success(),
                "powering off this box failed"
            );
            Ok(())
        }
        Err(err) => Err(err).within("reading the box ending policy"),
    }
}

pub fn remove(policy: &Policy) -> anyhow::Result<()> {
    #[derive(Deserialize)]
    struct Token {
        access_token: link_secretive::Secret,
        token_type: String,
    }
    // The cloud's own service: its token answer has fields Token doesn't
    // name (GCP's expires_in), read past as for any cloud API.
    let metadata = link_fetch::blocking::Client::builder()
        .metadata_server()
        .outside()
        .timeout(Duration::from_secs(10))
        .build()?;
    let mut request = metadata.get(&policy.credential_url);
    for (name, value) in &policy.credential_headers {
        request = request.header(name, value);
    }
    let token: Token = request.answer()?;
    ensure!(
        token.token_type.eq_ignore_ascii_case("bearer"),
        "metadata returned another token type"
    );
    let client = link_fetch::blocking::Client::builder()
        .outside()
        .timeout(Duration::from_secs(30))
        .build()?;
    client
        .delete(&policy.remove_url)
        .bearer(&token.access_token)
        .send()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    /// One HTTP request's first line and headers, answered with `body`.
    fn serve(listener: &TcpListener, body: &str) -> Vec<String> {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line.trim().is_empty() {
                break;
            }
            lines.push(line.trim().to_string());
        }
        write!(
            &stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        lines
    }

    #[test]
    fn a_gcp_box_deletes_itself_with_its_metadata_token() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let at = format!("http://{}", listener.local_addr().unwrap());
        let policy = Policy {
            credential_url: format!("{at}/token"),
            credential_headers: BTreeMap::from([("Metadata-Flavor".into(), "Google".into())]),
            remove_url: format!("{at}/instances/lnk-job"),
        };
        let server = std::thread::spawn(move || {
            // GCP's metadata server answers with expires_in as well.
            let token = serve(
                &listener,
                r#"{"access_token":"ya29.token","expires_in":3599,"token_type":"Bearer"}"#,
            );
            let delete = serve(&listener, r#"{"kind":"compute#operation"}"#);
            (token, delete)
        });
        remove(&policy).unwrap();
        let (token, delete) = server.join().unwrap();
        assert_eq!(token[0], "GET /token HTTP/1.1");
        assert!(
            token
                .iter()
                .any(|h| h.eq_ignore_ascii_case("metadata-flavor: Google")),
            "{token:?}"
        );
        assert_eq!(delete[0], "DELETE /instances/lnk-job HTTP/1.1");
        assert!(
            delete
                .iter()
                .any(|h| h.eq_ignore_ascii_case("authorization: Bearer ya29.token")),
            "{delete:?}"
        );
    }
}
