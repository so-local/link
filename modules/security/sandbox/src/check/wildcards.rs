//! The endpoints a check warns of: a `*.` over a domain a provider shares
//! among its customers.

/// Domains a provider shares among its customers, each a database or a
/// cache under it: a `*.` endpoint over one reaches anyone's.
const SHARED: &[&str] = &[
    "mongodb.net",
    "mongodb.com",
    "amazonaws.com",
    "rds.amazonaws.com",
    "cache.amazonaws.com",
    "cloudfront.net",
    "database.windows.net",
    "postgres.database.azure.com",
    "mysql.database.azure.com",
    "redis.cache.windows.net",
    "documents.azure.com",
    "neon.tech",
    "supabase.co",
    "supabase.com",
    "pooler.supabase.com",
    "aivencloud.com",
    "redislabs.com",
    "redis-cloud.com",
    "upstash.io",
    "cockroachlabs.cloud",
    "psdb.cloud",
    "elephantsql.com",
    "herokuapp.com",
    "railway.app",
    "render.com",
    "googleapis.com",
];

/// Of `tcp` (a policy's endpoints), each `*.` one over a domain a
/// provider shares among its customers, and that domain: `*.mongodb.net`,
/// or one of its regions' (`*.us-east-1.rds.amazonaws.com`).
pub fn shared_wildcards(tcp: &[String]) -> Vec<(String, String)> {
    let region = |label: &str| {
        let parts: Vec<&str> = label.split('-').collect();
        parts.len() >= 3
            && parts.iter().all(|p| !p.is_empty())
            && parts
                .last()
                .is_some_and(|p| p.bytes().all(|b| b.is_ascii_digit()))
    };
    tcp.iter()
        .filter_map(|e| link_sandbox_contract::net::check_endpoint(e).ok())
        .filter_map(|(host, port)| {
            let parent = host.strip_prefix("*.")?.to_string();
            let shared = SHARED.iter().find(|domain| {
                parent == **domain
                    || parent
                        .strip_suffix(&format!(".{domain}"))
                        .is_some_and(|rest| !rest.contains('.') && region(rest))
            })?;
            Some((format!("{host}:{port}"), shared.to_string()))
        })
        .collect()
}
