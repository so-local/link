//! DNS messages: queries read, answers written, answers read back.

use super::*;

pub const A: u16 = 1;
pub const CNAME: u16 = 5;
pub const TXT: u16 = 16;
pub const AAAA: u16 = 28;
pub const SRV: u16 = 33;
pub(super) const CLASS_IN: u16 = 1;

/// Response codes.
pub const NOERROR: u8 = 0;
pub const FORMERR: u8 = 1;
pub const SERVFAIL: u8 = 2;
pub const NXDOMAIN: u8 = 3;
pub const NOTIMP: u8 = 4;
pub const REFUSED: u8 = 5;

/// The most a message is read: a query is far smaller.
pub const MAX_MESSAGE: usize = 4096;
/// The most an answer over UDP is without EDNS, past which it's cut
/// (truncated) for the client to ask again over TCP.
pub(super) const MAX_UDP: usize = 512;
/// How long an answer is kept by whoever asked, at most.
pub(super) const MAX_TTL: u32 = 300;
/// The most records of one answer.
pub(super) const MAX_RECORDS: usize = 32;

/// A query's question, as read.
#[derive(Debug, PartialEq)]
pub struct Query {
    pub id: u16,
    /// Recursion desired, copied to the answer.
    pub rd: bool,
    /// The name asked for, lowercase, without a final dot.
    pub name: String,
    pub qtype: u16,
    /// The question as it came, its case too (some resolvers check it).
    pub question: Vec<u8>,
}

/// An answer's record, for the question's name.
#[derive(Clone, Debug, PartialEq)]
pub enum Record {
    A(Ipv4Addr),
    Aaaa(Ipv6Addr),
    Srv {
        priority: u16,
        weight: u16,
        port: u16,
        target: String,
    },
    Txt(Vec<Vec<u8>>),
}

impl Record {
    fn rtype(&self) -> u16 {
        match self {
            Record::A(_) => A,
            Record::Aaaa(_) => AAAA,
            Record::Srv { .. } => SRV,
            Record::Txt(_) => TXT,
        }
    }
}

/// The type's name, for the log.
pub fn type_name(qtype: u16) -> String {
    match qtype {
        A => "A".into(),
        AAAA => "AAAA".into(),
        SRV => "SRV".into(),
        TXT => "TXT".into(),
        CNAME => "CNAME".into(),
        other => format!("TYPE{other}"),
    }
}

/// `msg`'s question, if it's a plain query of one; else the response code
/// to refuse it with (None: not even a header, so no answer).
pub fn parse_query(msg: &[u8]) -> Result<Query, Option<(u16, u8)>> {
    if msg.len() < 12 {
        return Err(None);
    }
    let id = u16::from_be_bytes([msg[0], msg[1]]);
    let flags = u16::from_be_bytes([msg[2], msg[3]]);
    let count = |at: usize| u16::from_be_bytes([msg[at], msg[at + 1]]);
    // An answer, not a query: never answered.
    if flags & 0x8000 != 0 {
        return Err(None);
    }
    if (flags >> 11) & 0xf != 0 {
        return Err(Some((id, NOTIMP)));
    }
    if count(4) != 1 || count(6) != 0 || count(8) != 0 {
        return Err(Some((id, FORMERR)));
    }
    let (labels, end) = read_name(msg, 12, false).ok_or(Some((id, FORMERR)))?;
    if end + 4 > msg.len() {
        return Err(Some((id, FORMERR)));
    }
    let qtype = u16::from_be_bytes([msg[end], msg[end + 1]]);
    let qclass = u16::from_be_bytes([msg[end + 2], msg[end + 3]]);
    if qclass != CLASS_IN {
        return Err(Some((id, NOTIMP)));
    }
    let name = name_text(&labels).ok_or(Some((id, REFUSED)))?;
    Ok(Query {
        id,
        rd: flags & 0x0100 != 0,
        name,
        qtype,
        question: msg[12..end + 4].to_vec(),
    })
}

/// The labels of the name at `at` in `msg`, and where what follows it
/// starts; following compression pointers only if `pointers` (an
/// answer's), each to earlier in the message, so it ends.
pub(super) fn read_name(msg: &[u8], at: usize, pointers: bool) -> Option<(Vec<Vec<u8>>, usize)> {
    let mut labels = Vec::new();
    let mut at = at;
    let mut end = None;
    let mut length = 0;
    loop {
        let len = *msg.get(at)? as usize;
        match len {
            0 => {
                return Some((labels, end.unwrap_or(at + 1)));
            }
            1..=63 => {
                let label = msg.get(at + 1..at + 1 + len)?;
                length += len + 1;
                if length > 254 {
                    return None;
                }
                labels.push(label.to_vec());
                at += 1 + len;
            }
            l if l & 0xc0 == 0xc0 && pointers => {
                let to = ((l & 0x3f) << 8) | *msg.get(at + 1)? as usize;
                if to >= at {
                    return None;
                }
                end.get_or_insert(at + 2);
                at = to;
            }
            _ => return None,
        }
    }
}

/// `labels` as a name, lowercase: letters, digits, `-` and `_` only (a
/// service's `_mongodb._tcp`), or None.
pub(super) fn name_text(labels: &[Vec<u8>]) -> Option<String> {
    if labels.is_empty() {
        return None;
    }
    let mut parts = Vec::with_capacity(labels.len());
    for label in labels {
        let plain = label
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_');
        if !plain {
            return None;
        }
        parts.push(String::from_utf8_lossy(label).to_ascii_lowercase());
    }
    Some(parts.join("."))
}

/// `name` as a message's labels.
pub(super) fn write_name(out: &mut Vec<u8>, name: &str) {
    for label in name.split('.').filter(|l| !l.is_empty()) {
        out.push(label.len().min(63) as u8);
        out.extend_from_slice(&label.as_bytes()[..label.len().min(63)]);
    }
    out.push(0);
}

/// The answer to `query`: `rcode`, and `records` for its name, each kept
/// `ttl` seconds at most. Over UDP (`udp`), one that wouldn't fit
/// [`MAX_UDP`] is cut, for the client to ask again over TCP.
pub fn answer(query: &Query, rcode: u8, records: &[(Record, u32)], udp: bool) -> Vec<u8> {
    let mut body = Vec::new();
    for (record, ttl) in records.iter().take(MAX_RECORDS) {
        // The question's name, at offset 12.
        body.extend_from_slice(&[0xc0, 12]);
        body.extend_from_slice(&record.rtype().to_be_bytes());
        body.extend_from_slice(&CLASS_IN.to_be_bytes());
        body.extend_from_slice(&(*ttl).min(MAX_TTL).to_be_bytes());
        let mut data = Vec::new();
        match record {
            Record::A(ip) => data.extend_from_slice(&ip.octets()),
            Record::Aaaa(ip) => data.extend_from_slice(&ip.octets()),
            Record::Srv {
                priority,
                weight,
                port,
                target,
            } => {
                data.extend_from_slice(&priority.to_be_bytes());
                data.extend_from_slice(&weight.to_be_bytes());
                data.extend_from_slice(&port.to_be_bytes());
                write_name(&mut data, target);
            }
            Record::Txt(strings) => {
                for s in strings {
                    data.push(s.len().min(255) as u8);
                    data.extend_from_slice(&s[..s.len().min(255)]);
                }
            }
        }
        body.extend_from_slice(&(data.len() as u16).to_be_bytes());
        body.extend_from_slice(&data);
    }
    let count = records.len().min(MAX_RECORDS) as u16;
    let cut = udp && 12 + query.question.len() + body.len() > MAX_UDP;
    let mut flags: u16 = 0x8000 | 0x0080 | rcode as u16;
    if query.rd {
        flags |= 0x0100;
    }
    if cut {
        flags |= 0x0200;
    }
    let mut out = Vec::with_capacity(12 + query.question.len() + body.len());
    out.extend_from_slice(&query.id.to_be_bytes());
    out.extend_from_slice(&flags.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&(if cut { 0 } else { count }).to_be_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&query.question);
    if !cut {
        out.extend_from_slice(&body);
    }
    out
}

/// A bare error answer, for a query whose question can't be read.
pub(super) fn error(id: u16, rcode: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(12);
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&(0x8080u16 | rcode as u16).to_be_bytes());
    out.extend_from_slice(&[0; 8]);
    out
}
