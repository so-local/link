//! The hooks the example's declarations name. The example's types are
//! generated three times over, so a hook reads what it checks through a
//! trait each of them has.

/// What `probe_count` reads of `env probe`'s arguments.
pub trait Counted {
    fn count(&self) -> Option<i64>;
}

impl Counted for crate::module::EnvProbeArgs {
    fn count(&self) -> Option<i64> {
        self.count.map(i64::from)
    }
}

impl Counted for crate::args::EnvProbeArgs {
    fn count(&self) -> Option<i64> {
        self.count.map(i64::from)
    }
}

impl Counted for crate::handler::EnvProbeArgs {
    fn count(&self) -> Option<i64> {
        self.count.map(i64::from)
    }
}

/// `env probe` refuses four, to prove a hook runs after the declared checks.
pub fn probe_count(args: &impl Counted) -> Result<(), String> {
    match args.count() {
        Some(4) => Err("four is refused by its hook".into()),
        _ => Ok(()),
    }
}

/// An environment's name is never `reserved`, a rule across the spec a
/// hook on `env create` keeps on every read of the spec.
pub fn env_named(env: &impl Named) -> Result<(), String> {
    match env.name() {
        "reserved" => Err("the name reserved is kept for Link".into()),
        _ => Ok(()),
    }
}

/// What `env_named` reads of the env spec.
pub trait Named {
    fn name(&self) -> &str;
}

impl Named for crate::module::Env {
    fn name(&self) -> &str {
        &self.name
    }
}

impl Named for crate::types::Env {
    fn name(&self) -> &str {
        &self.name
    }
}

impl Named for crate::args::Env {
    fn name(&self) -> &str {
        &self.name
    }
}

impl Named for crate::handler::Env {
    fn name(&self) -> &str {
        &self.name
    }
}

/// What `domain` reads of a domain's name, in each place it's generated.
pub trait Spelled {
    fn spelled(&self) -> &str;
}

macro_rules! spelled {
    ($($module:ident),*) => {$(
        impl Spelled for crate::$module::Domain {
            fn spelled(&self) -> &str {
                &self.0
            }
        }
    )*};
}

spelled!(module, types, handler, args);

/// A domain refuses `localhost`, which its pattern allows, to prove a
/// shape's own hook runs wherever it's read.
pub fn domain(domain: &impl Spelled) -> Result<(), String> {
    match domain.spelled() {
        "localhost" => Err("localhost isn't a domain".into()),
        _ => Ok(()),
    }
}
