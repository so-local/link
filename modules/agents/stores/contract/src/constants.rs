#[cfg(feature = "files-bridge")]
pub(crate) const CAPACITY_FILE: &str = "capacity.json";

/// The route a team's folder service answers a member's calls on, under
/// its URL, as a router names it ([`crate::files::team_route`]).
#[cfg(feature = "files-client")]
pub const TEAM_FILES_ROUTE: &str = "/v1/teams/{owner}/{team}/files";

/// What a member's token for a team's folder service grants, before the
/// team's ID: membership of the team (`team.member@team:<id>`), checked
/// against the roster at each call ([`crate::files::member_grants`]).
#[cfg(feature = "files-client")]
pub const TEAM_MEMBER_GRANT: &str = "team.member@team:";

/// How long a call to a team's folder service may take, all of it: a
/// file at its largest each way.
#[cfg(feature = "files-bridge")]
pub const TEAM_FILES_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// How long before its end a member's kept token for a team's folder
/// service is made again: a call made with one never meets its end on the
/// way.
#[cfg(feature = "files-bridge")]
pub const TEAM_TOKEN_MARGIN_SECS: u64 = 60;
