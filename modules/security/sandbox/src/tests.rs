//! The confinements the crate's tests start from.

use std::path::PathBuf;

use link_sandbox_client::{Confinement, Net, Proxy};

pub(crate) fn bare(write: &[&str], read: &[&str]) -> Confinement {
    Confinement {
        name: "x".into(),
        write: write.iter().map(PathBuf::from).collect(),
        write_dot_files: Vec::new(),
        read: read.iter().map(PathBuf::from).collect(),
        read_home: false,
        hide: Vec::new(),
        tmp_prefix: None,
        terminal: false,
        commands: link_sandbox_client::Programs::Any,
        developer_tools: false,
        builds: Vec::new(),
        net: Net::Closed,
    }
}

pub(crate) fn proxied() -> Confinement {
    Confinement {
        name: "otter".into(),
        net: Net::Proxied {
            ports: [18789].into(),
            proxy: Proxy {
                hold: false,
                upstream: "direct".into(),
                dir: "/Users/me/.lnk/safe/agents/main/net/otter".into(),
                serve: [18789].into(),
                hosts: Vec::new(),
                tcp: Vec::new(),
                listen: None,
                per_minute: None,
                routes: vec![
                    link_sandbox_route::local_model::local(
                        link_sandbox_route::Bridge {
                            socket: "/nowhere/bridges/models.sock".into(),
                            start: Vec::new(),
                        },
                        [11434].into(),
                    )
                    .unwrap(),
                ],
                caller: None,
                secret: None,
                lan: Vec::new(),
                no_internet: false,
            },
        },
        ..bare(&["/Users/me/.lnk/data/agents/main/harnesses/otter"], &[])
    }
}
