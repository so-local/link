//! The worked example's module, generated from its declarations, and a
//! handler that keeps what it's given in memory.

pub mod checks;

#[cfg(test)]
link_test_kit::program!(unit);

pub mod module {
    include!(concat!(env!("OUT_DIR"), "/module.rs"));
}

/// The same declarations' types alone, as a contract crate shares them.
pub mod types {
    include!(concat!(env!("OUT_DIR"), "/types.rs"));
}

/// The same declarations' handler without a command line, as a served
/// module has it.
pub mod handler {
    include!(concat!(env!("OUT_DIR"), "/handler.rs"));
}

/// The same declarations' shapes and arguments with their `argv()`, as a
/// tool's contract crate shares them with the agent that runs it.
pub mod args {
    include!(concat!(env!("OUT_DIR"), "/args.rs"));
}

/// The program again, over [`args`]: its groups and each action's
/// arguments are that module's types, as a tool's program reads the
/// lines its contract's caller builds.
pub mod over {
    include!(concat!(env!("OUT_DIR"), "/over.rs"));
}

/// The store contract's shapes, as its contract crate shares them.
pub mod contract {
    include!(concat!(env!("OUT_DIR"), "/contract.rs"));
}

/// A store implementing the contract's protocol over the contract's shapes.
pub mod file_store {
    include!(concat!(env!("OUT_DIR"), "/file_store.rs"));
}

/// Core's own program, `lnk`, generated as every module's is.
pub mod core {
    include!(concat!(env!("OUT_DIR"), "/core.rs"));
}

use link_actions::call::{Call, Outcome};
use module::{Approval, ApprovalAddArgs, Created, Env, EnvCreateArgs, EnvProbeArgs, Probed};

/// Makes a box ID from the env's name, and keeps each approval asked for.
#[derive(Default)]
pub struct Handler {
    pub approvals: std::sync::Mutex<Vec<Approval>>,
}

impl module::EnvActions for Handler {
    fn env_create(&self, call: Call<EnvCreateArgs, Env>) -> Outcome<Created> {
        Outcome::Completed {
            output: Created {
                r#box: module::BoxId("box:k3m4n5p6q7r8".into()),
                cloud: format!("{} for {}", call.args.cloud, call.spec.name),
            },
        }
    }

    fn env_probe(&self, call: Call<EnvProbeArgs>) -> Outcome<Probed> {
        Outcome::Completed {
            output: Probed {
                count: call.args.count.map(i64::from).unwrap_or_default(),
                said: None,
            },
        }
    }

    fn profile_set(
        &self,
        call: Call<module::ProfileSetArgs, module::Profile>,
    ) -> Outcome<module::Profile> {
        Outcome::Completed { output: call.spec }
    }

    fn env_push(&self, call: Call<module::EnvPushArgs>) -> Outcome<Probed> {
        Outcome::Completed {
            output: Probed {
                count: call.args.files.len() as i64,
                said: None,
            },
        }
    }

    fn env_steer(&self, _: Call<module::EnvSteerArgs>) -> Outcome<Probed> {
        Outcome::Completed {
            output: Probed {
                count: 3,
                said: None,
            },
        }
    }

    fn env_compact(&self, _: Call<module::EnvCompactArgs>) -> Outcome<Probed> {
        Outcome::Completed {
            output: Probed {
                count: 0,
                said: None,
            },
        }
    }

    fn env_setup(&self, _: Call<module::EnvSetupArgs>) -> Outcome<Probed> {
        Outcome::Completed {
            output: Probed {
                count: 0,
                said: None,
            },
        }
    }

    fn approval_show(&self, call: Call<module::ApprovalShowArgs>) -> Outcome<Approval> {
        Outcome::Completed {
            output: Approval {
                secret: module::SecretId(call.args.secret),
                file: "env.toml".into(),
            },
        }
    }

    fn env_status(&self, _: Call<module::EnvStatusArgs>) -> Outcome<module::Status> {
        Outcome::Completed {
            output: module::Status("ready".into()),
        }
    }

    fn env_delete(&self, _: Call<module::EnvDeleteArgs>) -> Outcome<::link_types::Report> {
        Outcome::Completed {
            output: ::link_types::Report {
                blocks: Vec::new(),
                exit: 0,
            },
        }
    }

    fn state_show(&self, _: Call<module::StateShowArgs>) -> Outcome<Probed> {
        Outcome::Completed {
            output: Probed {
                count: 0,
                said: None,
            },
        }
    }

    fn env_inspect(&self, _: Call<module::EnvInspectArgs>) -> Outcome<Probed> {
        Outcome::Completed {
            output: Probed {
                count: 2,
                said: None,
            },
        }
    }

    fn state_export(&self, _: Call<module::StateExportArgs>) -> Outcome<Probed> {
        Outcome::Completed {
            output: Probed {
                count: 1,
                said: None,
            },
        }
    }

    fn env_wrap(&self, call: Call<module::EnvWrapArgs>) -> Outcome<Probed> {
        Outcome::Completed {
            output: Probed {
                count: call.args.program.len() as i64,
                said: None,
            },
        }
    }

    fn approval_add(&self, call: Call<ApprovalAddArgs>) -> Outcome<Approval> {
        let approval = Approval {
            secret: module::SecretId(call.args.secret),
            file: call.args.file,
        };
        self.approvals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(approval.clone());
        Outcome::Completed { output: approval }
    }
}
