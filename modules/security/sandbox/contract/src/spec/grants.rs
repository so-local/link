//! Grant algebra shared by policy editors.
use super::*;

/// A section's grants added to, and taken from, another's: each list
/// item by item, each value whole, and its OS's tables alike.
macro_rules! grants {
    ($ty:ident, [$($list:ident),*], [$($value:ident),*], [$($opens:ident),*]) => {
        impl $ty {
            fn join(&self, more: &$ty) -> $ty {
                let os = |a: &Option<Box<$ty>>, b: &Option<Box<$ty>>| match (a, b) {
                    (Some(a), Some(b)) => Some(Box::new(a.join(b))),
                    (a, b) => a.clone().or_else(|| b.clone()),
                };
                $ty {
                    $($list: union(&self.$list, &more.$list),)*
                    $($value: more.$value.clone().or_else(|| self.$value.clone()),)*
                    macos: os(&self.macos, &more.macos),
                    linux: os(&self.linux, &more.linux),
                }
            }

            fn without(&self, less: &$ty) -> $ty {
                let os = |a: &Option<Box<$ty>>| {
                    a.as_ref()
                        .map(|a| a.without(less))
                        .filter(|a| a.opens())
                        .map(Box::new)
                };
                $ty {
                    $($list: minus(&self.$list, &less.$list),)*
                    $($value: match less.$value {
                        Some(_) => None,
                        None => self.$value.clone(),
                    },)*
                    macos: os(&self.macos),
                    linux: os(&self.linux),
                }
            }

            /// Whether it opens anything, here or in an OS's table.
            fn opens(&self) -> bool {
                false $(|| self.$opens.as_ref().is_some_and(Opens::opens))*
                    || self.macos.as_ref().is_some_and(|t| t.opens())
                    || self.linux.as_ref().is_some_and(|t| t.opens())
            }
        }
    };
}

/// A key's value that opens something: a list with an item in it,
/// `"host"`, `true`.
trait Opens {
    fn opens(&self) -> bool;
}

impl<T> Opens for Vec<T> {
    fn opens(&self) -> bool {
        !self.is_empty()
    }
}

impl Opens for NetworkMode {
    fn opens(&self) -> bool {
        *self == NetworkMode::Host
    }
}

impl Opens for u32 {
    fn opens(&self) -> bool {
        true
    }
}

impl Opens for bool {
    fn opens(&self) -> bool {
        *self
    }
}

/// A list's item, which may be `"*"`: every item of its kind.
pub trait Item: Clone + PartialEq {
    fn any(&self) -> bool;
}

impl Item for String {
    fn any(&self) -> bool {
        self == "*"
    }
}

impl Item for Port {
    fn any(&self) -> bool {
        matches!(self, Port::Word(t) if t == "*")
    }
}

impl Item for u16 {
    fn any(&self) -> bool {
        false
    }
}

impl Item for Toolchain {
    fn any(&self) -> bool {
        false
    }
}

/// Both lists, each item once, `a`'s first; `"*"` in either is the
/// whole of it, since `"*"` beside others isn't a list a sandbox takes.
pub fn union<T: Item>(a: &Option<Vec<T>>, b: &Option<Vec<T>>) -> Option<Vec<T>> {
    let all = match (a, b) {
        (Some(a), Some(b)) => {
            let mut all = a.clone();
            all.extend(b.iter().filter(|i| !a.contains(i)).cloned());
            all
        }
        (a, b) => a.clone().or_else(|| b.clone())?,
    };
    Some(match all.iter().find(|i| i.any()) {
        Some(any) => vec![any.clone()],
        None => all,
    })
}

/// `a` without `b`'s items; none left, none.
fn minus<T: Clone + PartialEq>(a: &Option<Vec<T>>, b: &Option<Vec<T>>) -> Option<Vec<T>> {
    let left: Vec<T> = a
        .iter()
        .flatten()
        .filter(|i| !b.iter().flatten().any(|j| j == *i))
        .cloned()
        .collect();
    (!left.is_empty()).then_some(left)
}

grants!(
    Files,
    [write, read, write_dot_files, hide],
    [],
    [write, read]
);
grants!(Network, [], [mode], [mode]);
grants!(Internet, [hosts, tcp], [per_minute], [hosts]);
grants!(Lan, [hosts], [], [hosts]);
grants!(Localhost, [ports, models], [], [ports, models]);
grants!(Tools, [names], [], [names]);
grants!(Browser, [hosts], [], [hosts]);
grants!(
    Commands,
    [allow, toolchains],
    [developer_tools],
    [allow, toolchains, developer_tools]
);

impl Spec {
    /// This spec's grants and `more`'s together: each list both's, each
    /// value `more`'s where it gives one, OS tables alike. What `lnk agent
    /// permission add` does to an agent's spec.
    pub fn join(&self, more: &Spec) -> Spec {
        fn section<T: Clone>(
            a: &Option<T>,
            b: &Option<T>,
            join: impl Fn(&T, &T) -> T,
        ) -> Option<T> {
            match (a, b) {
                (Some(a), Some(b)) => Some(join(a, b)),
                (a, b) => a.clone().or_else(|| b.clone()),
            }
        }
        Spec {
            name: more.name.clone().or_else(|| self.name.clone()),
            version: more.version.or(self.version),
            files: section(&self.files, &more.files, Files::join),
            network: section(&self.network, &more.network, Network::join),
            internet: section(&self.internet, &more.internet, Internet::join),
            lan: section(&self.lan, &more.lan, Lan::join),
            localhost: section(&self.localhost, &more.localhost, Localhost::join),
            tools: section(&self.tools, &more.tools, Tools::join),
            browser: section(&self.browser, &more.browser, Browser::join),
            commands: section(&self.commands, &more.commands, Commands::join),
            teams: {
                let mut teams = self.teams.clone();
                for (team, more) in &more.teams {
                    let joined = match teams.get(team) {
                        Some(have) => have.join(more),
                        None => more.clone(),
                    };
                    teams.insert(team.clone(), joined);
                }
                teams
            },
        }
    }

    /// This spec without what `less` names, in its OS tables too: each
    /// item of its lists, and each value it gives (`[network] mode`,
    /// `developer_tools`, `per_minute`) back to none. A section left
    /// opening nothing goes whole: `[internet]` without hosts takes its
    /// `tcp` with it. What `lnk agent permission remove` does to an agent's spec.
    pub fn without(&self, less: &Spec) -> Spec {
        fn section<T>(
            a: &Option<T>,
            b: &Option<T>,
            without: impl Fn(&T, &T) -> T,
            opens: impl Fn(&T) -> bool,
        ) -> Option<T>
        where
            T: Clone,
        {
            match (a, b) {
                (Some(a), Some(b)) => Some(without(a, b)).filter(|s| opens(s)),
                (a, _) => a.clone(),
            }
        }
        Spec {
            name: self.name.clone(),
            version: self.version,
            files: section(&self.files, &less.files, Files::without, Files::opens),
            network: section(
                &self.network,
                &less.network,
                Network::without,
                Network::opens,
            ),
            internet: section(
                &self.internet,
                &less.internet,
                Internet::without,
                Internet::opens,
            ),
            lan: section(&self.lan, &less.lan, Lan::without, Lan::opens),
            localhost: section(
                &self.localhost,
                &less.localhost,
                Localhost::without,
                Localhost::opens,
            ),
            tools: section(&self.tools, &less.tools, Tools::without, Tools::opens),
            browser: section(
                &self.browser,
                &less.browser,
                Browser::without,
                Browser::opens,
            ),
            commands: section(
                &self.commands,
                &less.commands,
                Commands::without,
                Commands::opens,
            ),
            // A team left reaching nothing goes whole.
            teams: self
                .teams
                .iter()
                .map(|(team, have)| match less.teams.get(team) {
                    Some(less) => (team.clone(), have.without(less)),
                    None => (team.clone(), have.clone()),
                })
                .filter(|(_, folders)| !folders.is_empty())
                .collect(),
        }
    }

    /// Whether this spec opens on `os` everything `need` opens there: each
    /// folder, host, endpoint, port, tool and program it names (a `"*"`
    /// covering any host, port or program), `[network] mode = "host"` and
    /// the developer tools. What only narrows (`hide`, `per_minute`) asks
    /// for nothing. Paths are compared as written: both absolute.
    pub fn covers(&self, need: &Spec, os: Os) -> anyhow::Result<bool> {
        let (have, need) = (self.on(os)?, need.on(os)?);
        fn all<T: PartialEq>(
            have: &Option<Vec<T>>,
            need: &Option<Vec<T>>,
            any: Option<&T>,
        ) -> bool {
            let have = have.as_deref().unwrap_or_default();
            need.iter()
                .flatten()
                .all(|n| have.contains(n) || any.is_some_and(|a| have.contains(a)))
        }
        let star = "*".to_string();
        let every = Port::Word("*".into());
        let files = need.files.as_ref().is_none_or(|n| {
            let h = have.files.clone().unwrap_or_default();
            all(&h.write, &n.write, None)
                && all(&union(&h.read, &h.write), &n.read, None)
                && all(&h.write_dot_files, &n.write_dot_files, None)
        });
        let network = need.network.as_ref().is_none_or(|n| {
            n.mode != Some(NetworkMode::Host)
                || have.network.as_ref().and_then(|h| h.mode) == Some(NetworkMode::Host)
        });
        let internet = need.internet.as_ref().is_none_or(|n| {
            have.internet
                .as_ref()
                .is_some_and(|h| all(&h.hosts, &n.hosts, Some(&star)) && all(&h.tcp, &n.tcp, None))
        });
        let lan = need.lan.as_ref().is_none_or(|n| {
            have.lan
                .as_ref()
                .is_some_and(|h| all(&h.hosts, &n.hosts, Some(&star)))
        });
        let localhost = need.localhost.as_ref().is_none_or(|n| {
            have.localhost.as_ref().is_some_and(|h| {
                all(&h.ports, &n.ports, Some(&every)) && all(&h.models, &n.models, None)
            })
        });
        let tools = need.tools.as_ref().is_none_or(|n| {
            have.tools
                .as_ref()
                .is_some_and(|h| all(&h.names, &n.names, None))
        });
        let browser = need.browser.as_ref().is_none_or(|n| {
            have.browser
                .as_ref()
                .is_some_and(|h| all(&h.hosts, &n.hosts, Some(&star)))
        });
        let commands = need.commands.as_ref().is_none_or(|n| {
            let h = have.commands.clone().unwrap_or_default();
            all(&h.allow, &n.allow, Some(&star))
                && all(&h.toolchains, &n.toolchains, None)
                && (n.developer_tools != Some(true) || h.developer_tools == Some(true))
        });
        let teams = need
            .teams
            .iter()
            .all(|(team, need)| have.teams.get(team).is_some_and(|have| have.covers(need)));
        Ok(files
            && network
            && internet
            && lan
            && localhost
            && tools
            && browser
            && commands
            && teams)
    }
}

impl Spec {
    /// Each thing it opens, as a spec of its own: one folder, host,
    /// endpoint, port, tool or program, `[network] mode = "host"`, the
    /// developer tools, a limit of calls. Of a spec already [`Spec::on`]
    /// one OS: an OS's tables aren't here.
    pub fn items(&self) -> Vec<Spec> {
        let mut items = Vec::new();
        macro_rules! each {
            ($section:ident, $ty:ident, [$($list:ident),*], [$($value:ident),*]) => {
                if let Some(section) = &self.$section {
                    $(for item in section.$list.iter().flatten() {
                        items.push(Spec {
                            $section: Some($ty {
                                $list: Some(vec![item.clone()]),
                                ..Default::default()
                            }),
                            ..Default::default()
                        });
                    })*
                    $(if let Some(value) = &section.$value
                        && Opens::opens(value)
                    {
                        items.push(Spec {
                            $section: Some($ty {
                                $value: Some(value.clone()),
                                ..Default::default()
                            }),
                            ..Default::default()
                        });
                    })*
                }
            };
        }
        each!(files, Files, [write, read, write_dot_files, hide], []);
        each!(network, Network, [], [mode]);
        each!(internet, Internet, [hosts, tcp], [per_minute]);
        each!(lan, Lan, [hosts], []);
        each!(localhost, Localhost, [ports, models], []);
        each!(tools, Tools, [names], []);
        each!(browser, Browser, [hosts], []);
        each!(commands, Commands, [allow, toolchains], [developer_tools]);
        for (team, folders) in &self.teams {
            for (name, write) in folders
                .written()
                .iter()
                .map(|n| (n, true))
                .chain(folders.only_read().iter().map(|n| (n, false)))
            {
                let one = vec![name.clone()];
                let folders = match write {
                    true => TeamFolders {
                        write: one,
                        read: Vec::new(),
                    },
                    false => TeamFolders {
                        write: Vec::new(),
                        read: one,
                    },
                };
                items.push(Spec {
                    teams: [(team.clone(), folders)].into(),
                    ..Default::default()
                });
            }
        }
        items
    }
}

impl Spec {
    /// Why taking `less` from this spec ([`Spec::without`]) would widen
    /// it on either OS, if it would: a limit of calls lifted, a folder no
    /// longer hidden, one host, port or program taken from a list that
    /// holds `"*"`, which leaves any, or an OS's table left without a key
    /// it gave, which falls back to the section's own. A `deny` is
    /// refused for it, never done.
    pub fn widened_by_taking(&self, less: &Spec) -> anyhow::Result<Option<String>> {
        let after = self.without(less);
        for os in [Os::Linux, Os::MacOs] {
            if let Some(why) = self.widened_on(less, &after, os)? {
                return Ok(Some(why));
            }
        }
        Ok(None)
    }

    fn widened_on(&self, less: &Spec, after: &Spec, os: Os) -> anyhow::Result<Option<String>> {
        let (have, less, after) = (self.on(os)?, less.on(os)?, after.on(os)?);
        if less
            .internet
            .as_ref()
            .is_some_and(|i| i.per_minute.is_some())
        {
            return Ok(Some(
                "taking a limit of calls away would lift it: a limit is set with allow \
                 --internet.per_minute N, and changed only so"
                    .into(),
            ));
        }
        let hidden = |s: &Spec| s.files.as_ref().and_then(|f| f.hide.clone());
        if hidden(&have)
            .iter()
            .flatten()
            .any(|h| !hidden(&after).iter().flatten().any(|a| a == h))
        {
            return Ok(Some(
                "taking a folder from hide would show what it hides".into(),
            ));
        }
        fn from_any<T: PartialEq>(have: &Option<Vec<T>>, less: &Option<Vec<T>>, any: &T) -> bool {
            have.as_ref().is_some_and(|h| h.contains(any))
                && less.iter().flatten().any(|l| l != any)
        }
        let star = "*".to_string();
        let every = Port::Word("*".into());
        let any = [
            from_any(
                &have.internet.as_ref().and_then(|i| i.hosts.clone()),
                &less.internet.as_ref().and_then(|i| i.hosts.clone()),
                &star,
            ),
            from_any(
                &have.lan.as_ref().and_then(|l| l.hosts.clone()),
                &less.lan.as_ref().and_then(|l| l.hosts.clone()),
                &star,
            ),
            from_any(
                &have.browser.as_ref().and_then(|b| b.hosts.clone()),
                &less.browser.as_ref().and_then(|b| b.hosts.clone()),
                &star,
            ),
            from_any(
                &have.localhost.as_ref().and_then(|l| l.ports.clone()),
                &less.localhost.as_ref().and_then(|l| l.ports.clone()),
                &every,
            ),
            from_any(
                &have.commands.as_ref().and_then(|c| c.allow.clone()),
                &less.commands.as_ref().and_then(|c| c.allow.clone()),
                &star,
            ),
        ];
        if any.contains(&true) {
            return Ok(Some(
                "it can't deny one of any (\"*\"): deny \"*\", then allow each it may reach".into(),
            ));
        }
        // An OS's table left without a key it gave falls back to the
        // section's own, which may open more: never written.
        if let Some(wider) = after
            .items()
            .into_iter()
            .find(|item| !have.covers(item, os).unwrap_or(false))
        {
            let os = match os {
                Os::Linux => "linux",
                Os::MacOs => "macos",
            };
            return Ok(Some(format!(
                "on {os} it would then have {}, which it hasn't now: an OS's table left without \
                 a key falls back to the section's own; change the [<section>.{os}] table in the \
                 file itself",
                link_spec::print(&wider)?
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            )));
        }
        Ok(None)
    }

    /// An error for a list, in any table, holding `"*"` beside other
    /// items: `"*"` is every item of its kind already.
    pub fn check_any_alone(&self) -> anyhow::Result<()> {
        fn walk(value: &serde_json::Value, at: &str) -> anyhow::Result<()> {
            match value {
                serde_json::Value::Array(items) => ensure!(
                    items.len() == 1 || !items.iter().any(|i| i == "*"),
                    "{at}: \"*\" is everything already: leave out the rest"
                ),
                serde_json::Value::Object(keys) => {
                    for (key, value) in keys {
                        let at = match at {
                            "" => key.clone(),
                            at => format!("{at}.{key}"),
                        };
                        walk(value, &at)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        walk(&serde_json::to_value(self)?, "")
    }
}
