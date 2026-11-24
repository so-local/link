//! Asking this machine's resolvers, for what the rules let through.

use super::*;

/// How many lookups that wait on someone else (a name's public
/// addresses, SRV and TXT) run at once, for every query of the sandbox:
/// past them a query waits its turn, and the system resolver's threads,
/// which the proxy's own connections need too, stay free.
pub(super) const MAX_LOOKUPS: usize = 16;
pub(super) static LOOKUPS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(MAX_LOOKUPS);
/// How long a lookup may take, its wait for a turn included.
pub(super) const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);
/// How long SRV and TXT may take, from the nameservers in turn, its wait
/// for a turn included.
pub(super) const ASK_TIMEOUT: Duration = Duration::from_secs(10);

/// `name`'s public addresses of `qtype` (A or AAAA), as the proxy itself
/// would dial them: never this machine's, its network's or a metadata
/// service's. Asked of the system's resolver as the proxy's connections
/// are, so a lookup and the connection after it ask once. A name whose
/// only refused address is a metadata service's is refused as a
/// connection to it is ([`crate::wire::Metadata`]), which pauses the
/// wire.
pub(super) async fn public(
    name: &str,
    qtype: u16,
    rules: &Filter,
) -> anyhow::Result<(u8, Vec<(Record, u32)>)> {
    let found = tokio::time::timeout(LOOKUP_TIMEOUT, async {
        let permit = LOOKUPS.acquire().await.ok();
        link_sandbox_route::resolve::resolve_held(name, 0, permit).await
    })
    .await;
    let addrs = match found {
        Ok(Ok(addrs)) => addrs,
        Ok(Err(_)) => return Ok((NXDOMAIN, Vec::new())),
        Err(_) => anyhow::bail!("{name}: no answer in time"),
    };
    let ips: Vec<IpAddr> = addrs.iter().map(SocketAddr::ip).collect();
    let allowed: Vec<IpAddr> = ips
        .iter()
        .copied()
        .filter(|ip| (rules.allowed)(*ip))
        .collect();
    if allowed.is_empty() && ips.iter().any(|ip| crate::wire::is_metadata(*ip)) {
        return Err(crate::wire::Metadata(name.to_string()).into());
    }
    let records = allowed
        .into_iter()
        .filter_map(|ip| match (ip, qtype) {
            (IpAddr::V4(v4), A) => Some((Record::A(v4), 30)),
            (IpAddr::V6(v6), AAAA) => Some((Record::Aaaa(v6), 30)),
            _ => None,
        })
        .collect();
    Ok((NOERROR, records))
}

/// The nameservers this machine asks (`/etc/resolv.conf`), at most three.
pub fn nameservers() -> Vec<SocketAddr> {
    let text = std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default();
    nameservers_in(&text)
}

pub(super) fn nameservers_in(text: &str) -> Vec<SocketAddr> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("nameserver"))
        .filter_map(|rest| rest.trim().split('%').next()?.parse::<IpAddr>().ok())
        .map(|ip| SocketAddr::new(ip, 53))
        .take(3)
        .collect()
}

/// A query for `name` of `qtype`, with id `id`.
pub(super) fn query(id: u16, name: &str, qtype: u16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&0x0100u16.to_be_bytes());
    out.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);
    write_name(&mut out, name);
    out.extend_from_slice(&qtype.to_be_bytes());
    out.extend_from_slice(&CLASS_IN.to_be_bytes());
    out
}

/// A random query id.
pub(super) fn random_id() -> u16 {
    let mut bytes = [0u8; 2];
    if let Ok(mut random) = std::fs::File::open("/dev/urandom") {
        let _ = std::io::Read::read_exact(&mut random, &mut bytes);
    }
    u16::from_ne_bytes(bytes)
}

/// `name`'s records of `qtype`, from `servers` in turn (over UDP, again
/// over TCP when cut short), as the response code and records.
pub async fn ask(
    servers: &[SocketAddr],
    name: &str,
    qtype: u16,
) -> anyhow::Result<(u8, Vec<(Record, u32)>)> {
    anyhow::ensure!(!servers.is_empty(), "no nameserver here (/etc/resolv.conf)");
    let mut last = anyhow::anyhow!("no answer for {name}");
    for server in servers {
        let id = random_id();
        let asked = query(id, name, qtype);
        let got = match ask_udp(*server, &asked).await {
            Ok(got) if got.len() > 3 && got[2] & 0x02 != 0 => ask_tcp(*server, &asked).await,
            other => other,
        };
        match got.and_then(|got| read_answer(&got, id, name, qtype)) {
            Ok((rcode, records)) if rcode == NOERROR || rcode == NXDOMAIN => {
                return Ok((rcode, records));
            }
            Ok((rcode, _)) => last = anyhow::anyhow!("{name}: the nameserver answered {rcode}"),
            Err(err) => last = err,
        }
    }
    Err(last)
}

pub(super) async fn ask_udp(server: SocketAddr, asked: &[u8]) -> anyhow::Result<Vec<u8>> {
    let bind: SocketAddr = match server {
        SocketAddr::V4(_) => (Ipv4Addr::UNSPECIFIED, 0).into(),
        SocketAddr::V6(_) => (Ipv6Addr::UNSPECIFIED, 0).into(),
    };
    let socket = tokio::net::UdpSocket::bind(bind).await?;
    socket.connect(server).await?;
    socket.send(asked).await?;
    let mut buf = vec![0u8; MAX_MESSAGE];
    let n = tokio::time::timeout(Duration::from_secs(2), socket.recv(&mut buf)).await??;
    buf.truncate(n);
    Ok(buf)
}

pub(super) async fn ask_tcp(server: SocketAddr, asked: &[u8]) -> anyhow::Result<Vec<u8>> {
    let talk = async {
        let mut stream = tokio::net::TcpStream::connect(server).await?;
        // Its length and the message in one write: a second small one
        // could wait for the first's ACK.
        let mut message = (asked.len() as u16).to_be_bytes().to_vec();
        message.extend_from_slice(asked);
        stream.write_all(&message).await?;
        let len = stream.read_u16().await? as usize;
        anyhow::ensure!(len <= 65535, "too long");
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf).await?;
        Ok::<_, anyhow::Error>(buf)
    };
    tokio::time::timeout(Duration::from_secs(4), talk).await?
}

/// The records of `qtype` for `name` (and the names it's an alias of) in
/// `msg`, the answer to query `id`; only what was asked, never another
/// name's.
pub fn read_answer(
    msg: &[u8],
    id: u16,
    name: &str,
    qtype: u16,
) -> anyhow::Result<(u8, Vec<(Record, u32)>)> {
    anyhow::ensure!(msg.len() >= 12, "a short answer");
    let flags = u16::from_be_bytes([msg[2], msg[3]]);
    anyhow::ensure!(
        u16::from_be_bytes([msg[0], msg[1]]) == id && flags & 0x8000 != 0,
        "not the answer asked for"
    );
    let rcode = (flags & 0xf) as u8;
    let count = |at: usize| u16::from_be_bytes([msg[at], msg[at + 1]]) as usize;
    anyhow::ensure!(count(4) == 1, "not the question asked");
    let (labels, mut at) = read_name(msg, 12, true).ok_or_else(|| anyhow::anyhow!("a bad name"))?;
    anyhow::ensure!(
        name_text(&labels).as_deref() == Some(name)
            && msg.get(at..at + 4) == Some(&[(qtype >> 8) as u8, qtype as u8, 0, 1][..]),
        "not the question asked"
    );
    at += 4;
    let mut owners = vec![name.to_string()];
    let mut records = Vec::new();
    for _ in 0..count(6) {
        let (labels, end) =
            read_name(msg, at, true).ok_or_else(|| anyhow::anyhow!("a bad name"))?;
        let fixed = msg
            .get(end..end + 10)
            .ok_or_else(|| anyhow::anyhow!("a short record"))?;
        let rtype = u16::from_be_bytes([fixed[0], fixed[1]]);
        let ttl = u32::from_be_bytes([fixed[4], fixed[5], fixed[6], fixed[7]]);
        let len = u16::from_be_bytes([fixed[8], fixed[9]]) as usize;
        let data_at = end + 10;
        let data = msg
            .get(data_at..data_at + len)
            .ok_or_else(|| anyhow::anyhow!("a short record"))?;
        at = data_at + len;
        let owner = name_text(&labels);
        if !owner.as_ref().is_some_and(|o| owners.contains(o)) {
            continue;
        }
        let record = match rtype {
            CNAME if owners.len() < 8 => {
                if let Some(alias) = read_name(msg, data_at, true).and_then(|(l, _)| name_text(&l))
                {
                    owners.push(alias);
                }
                continue;
            }
            t if t != qtype => continue,
            A if len == 4 => Record::A(Ipv4Addr::new(data[0], data[1], data[2], data[3])),
            AAAA if len == 16 => Record::Aaaa(Ipv6Addr::from(
                <[u8; 16]>::try_from(data).unwrap_or_default(),
            )),
            SRV if len > 6 => {
                let Some(target) =
                    read_name(msg, data_at + 6, true).and_then(|(l, _)| name_text(&l))
                else {
                    continue;
                };
                Record::Srv {
                    priority: u16::from_be_bytes([data[0], data[1]]),
                    weight: u16::from_be_bytes([data[2], data[3]]),
                    port: u16::from_be_bytes([data[4], data[5]]),
                    target,
                }
            }
            TXT => {
                let mut strings = Vec::new();
                let mut i = 0;
                while i < data.len() {
                    let n = data[i] as usize;
                    let Some(s) = data.get(i + 1..i + 1 + n) else {
                        break;
                    };
                    strings.push(s.to_vec());
                    i += 1 + n;
                }
                Record::Txt(strings)
            }
            _ => continue,
        };
        if records.len() < MAX_RECORDS {
            records.push((record, ttl));
        }
    }
    Ok((rcode, records))
}
