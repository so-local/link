//! Who a run runs as on a box: `root`, outside any sandbox, or a user of
//! the box by name, in their own sandbox.

use crate::User;

impl User {
    /// `root`.
    pub fn root() -> Self {
        User("root".into())
    }

    pub fn is_root(&self) -> bool {
        self.0 == "root"
    }

    /// The user's name, when it isn't root.
    pub fn named(&self) -> Option<&str> {
        (!self.is_root()).then_some(self.0.as_str())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Eq for User {}

impl std::fmt::Display for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A name, checked as a declaration's `user` checks it.
impl TryFrom<String> for User {
    type Error = String;

    fn try_from(name: String) -> Result<User, String> {
        let user = User(name);
        link_actions::value::Validate::validate(&user, "").map_err(|r| r.why)?;
        Ok(user)
    }
}

impl std::str::FromStr for User {
    type Err = String;

    fn from_str(name: &str) -> Result<User, String> {
        User::try_from(name.to_string())
    }
}

impl From<User> for String {
    fn from(user: User) -> String {
        user.0
    }
}
