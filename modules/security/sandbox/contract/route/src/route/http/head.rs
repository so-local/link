//! A head, read once: a request's from a sandbox, or an answer's from
//! where a route sent it. Every reader of a head reads it here, so no two
//! readers can take one head two ways.

/// One header line of a head: its name lowercased, its value trimmed,
/// and the line as it came.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header<'a> {
    pub name: String,
    pub value: &'a str,
    pub line: &'a str,
}

/// A request's head, as [`RequestHead::parse`] reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestHead<'a> {
    pub method: &'a str,
    pub target: &'a str,
    pub version: &'a str,
    pub headers: Vec<Header<'a>>,
}

impl<'a> RequestHead<'a> {
    /// `head` (through its blank line), or why it's refused: a head that
    /// isn't [`super::plain`] or text, a request line not of three parts
    /// split by one space, a version not HTTP/1.x, a header folded over
    /// lines or without a colon.
    pub fn parse(head: &'a [u8]) -> Result<Self, String> {
        if !super::plain(head) {
            return Err("a request head with a bare line end or a control character".into());
        }
        let text = std::str::from_utf8(head).map_err(|_| "a request head that isn't text")?;
        let mut lines = text.split("\r\n");
        let mut parts = lines.next().unwrap_or_default().split(' ');
        let (Some(method), Some(target), Some(version), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err("not an HTTP request".into());
        };
        if method.is_empty() || target.is_empty() {
            return Err("not an HTTP request".into());
        }
        if version != "HTTP/1.1" && version != "HTTP/1.0" {
            return Err(format!("{version} isn't HTTP/1.x"));
        }
        let mut headers = Vec::new();
        for line in lines.take_while(|l| !l.is_empty()) {
            if line.starts_with([' ', '\t']) {
                return Err("a header folded over lines".into());
            }
            let (name, value) = line.split_once(':').ok_or("a header without a colon")?;
            // A name as every reader takes it: a token, nothing around it,
            // so no server reads `Content-Length :` as the length checked
            // here, or as none.
            let token = |b: u8| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_|~".contains(&b) || b == 0x60 /* the backtick */;
            if name.is_empty() || !name.bytes().all(token) {
                return Err(format!("{name:?} isn't a header's name"));
            }
            let name = name.to_ascii_lowercase();
            let value = value.trim();
            // A length of digits alone, read one way by every reader.
            if name == "content-length"
                && (value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()))
            {
                return Err("a bad content-length".into());
            }
            headers.push(Header { name, value, line });
        }
        Ok(Self {
            method,
            target,
            version,
            headers,
        })
    }

    /// Its target's path, without a query.
    pub fn path(&self) -> &'a str {
        self.target.split_once('?').map_or(self.target, |(p, _)| p)
    }

    /// Its first header `name` (lowercase)'s value.
    pub fn header(&self, name: &str) -> Option<&'a str> {
        self.headers
            .iter()
            .find(|h| h.name == name)
            .map(|h| h.value)
    }

    /// Whether it asks for a `100 Continue` before its body.
    pub fn expects_continue(&self) -> bool {
        self.headers
            .iter()
            .any(|h| h.name == "expect" && h.value.eq_ignore_ascii_case("100-continue"))
    }

    /// Its body's length, if the head says it one way: none said is 0;
    /// a body in chunks, a length that isn't a number, or two that differ
    /// is refused.
    pub fn length(&self) -> Result<u64, String> {
        let mut length = None;
        for header in &self.headers {
            match header.name.as_str() {
                "transfer-encoding" => return Err("a body without a length (chunked)".into()),
                "content-length" => {
                    let value = header
                        .value
                        .parse()
                        .map_err(|_| "a bad content-length".to_string())?;
                    if length.is_some_and(|seen| seen != value) {
                        return Err("two lengths".into());
                    }
                    length = Some(value);
                }
                _ => {}
            }
        }
        Ok(length.unwrap_or(0))
    }
}

/// An answer's head, as [`AnswerHead::parse`] reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnswerHead<'a> {
    /// Its status line, whole.
    pub status: &'a str,
    pub version: &'a str,
    pub code: u16,
    /// Its headers; one without a colon is kept as a name alone: an
    /// origin's head, passed on, never a sandbox's.
    pub headers: Vec<Header<'a>>,
}

impl<'a> AnswerHead<'a> {
    /// `head` (through its blank line, or its status line alone), or why
    /// not: not text, not HTTP/1, no status code.
    pub fn parse(head: &'a [u8]) -> Result<Self, String> {
        let text = std::str::from_utf8(head).map_err(|_| "an answer head that isn't text")?;
        let mut lines = text.split("\r\n");
        let status = lines.next().unwrap_or_default();
        let mut parts = status.split(' ');
        let version = parts.next().unwrap_or_default();
        let code = parts
            .next()
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| format!("not an HTTP answer: {}", link_format::clean(status)))?;
        if !version.starts_with("HTTP/1.") {
            return Err("not an HTTP/1 answer".into());
        }
        let headers = lines
            .take_while(|l| !l.is_empty())
            .map(|line| {
                let (name, value) = line.split_once(':').unwrap_or((line, ""));
                Header {
                    name: name.trim().to_ascii_lowercase(),
                    value: value.trim(),
                    line,
                }
            })
            .collect();
        Ok(Self {
            status,
            version,
            code,
            headers,
        })
    }

    /// Its first header `name` (lowercase)'s value.
    pub fn header(&self, name: &str) -> Option<&'a str> {
        self.headers
            .iter()
            .find(|h| h.name == name)
            .map(|h| h.value)
    }

    /// A 1xx, which another head follows.
    pub fn informational(&self) -> bool {
        (100..200).contains(&self.code)
    }
}
