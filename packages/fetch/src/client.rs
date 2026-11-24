//! The client calls go out on ([`Client`]), what it was built to allow
//! ([`Builder`]), and the rules it carries to each call.

use super::*;

/// The client calls go out on: one for as long as its caller calls, so
/// its connections are kept. Cheap to clone.
#[derive(Clone, Debug)]
pub struct Client {
    /// For `https://`: this machine's proxies and DNS, as any program.
    http: reqwest::Client,
    /// For `http://` to this machine: no proxy, and loopback alone
    /// ([`Loopback`]).
    here: reqwest::Client,
    rules: Rules,
}

/// What a client was built to allow, beyond Link's rules.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Rules {
    /// Answers from a service that isn't Link's: read open.
    pub(super) outside: bool,
    /// Where plain `http://` may go beyond this machine.
    pub(super) plain: Plain,
    /// A stream given up when nothing moves for this long
    /// ([`Builder::streaming`]).
    pub(super) stall: Option<Duration>,
    /// Redirects followed ([`Builder::follow_redirects`]).
    pub(super) redirects: usize,
    /// To public addresses alone ([`Builder::public_only`]).
    pub(super) public: bool,
    /// `https://` alone, this machine too ([`Builder::https_only`]).
    pub(super) https: bool,
}

/// Where a client's plain `http://` may go beyond this machine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Plain {
    /// Nowhere.
    #[default]
    Here,
    /// A cloud's metadata server alone, carrying no credential
    /// ([`Builder::metadata_server`]).
    Metadata,
    /// Anywhere, the caller's user having said so ([`Builder::plain_http`]).
    Said,
}

impl Client {
    /// A client by Link's rules, its calls stopped past [`TIMEOUT`].
    pub fn new() -> Result<Self, Error> {
        Self::builder().build()
    }

    /// A client by Link's rules, its calls stopped past `timeout`.
    pub fn with_timeout(timeout: Duration) -> Result<Self, Error> {
        Self::builder().timeout(timeout).build()
    }

    /// A client by Link's rules but for what its caller names
    /// ([`Builder`]).
    pub fn builder() -> Builder {
        Builder::default()
    }

    /// A `GET` of `url`.
    pub fn get(&self, url: &str) -> Request<'_> {
        self.request(Method::GET, url)
    }

    /// A `POST` to `url`.
    pub fn post(&self, url: &str) -> Request<'_> {
        self.request(Method::POST, url)
    }

    /// A `PUT` to `url`.
    pub fn put(&self, url: &str) -> Request<'_> {
        self.request(Method::PUT, url)
    }

    /// A `PATCH` of `url`.
    pub fn patch(&self, url: &str) -> Request<'_> {
        self.request(Method::PATCH, url)
    }

    /// A `DELETE` of `url`.
    pub fn delete(&self, url: &str) -> Request<'_> {
        self.request(Method::DELETE, url)
    }

    /// A call to `url` that holds a secret (a bot's token in its path),
    /// named `shown` wherever it's named ("Telegram's getMe"), never by
    /// its URL.
    pub fn secret_url<'a>(&'a self, method: Method, url: &'a Secret, shown: &str) -> Request<'a> {
        let route = Route(format!("{method} {}", printable(shown)));
        self.make(method, Target::Secret(url), route)
    }

    /// A call of `method` to `url`.
    pub fn request(&self, method: Method, url: &str) -> Request<'_> {
        let route = Route::new(&method, url);
        self.make(method, Target::Plain(url.to_string()), route)
    }

    fn make<'a>(&'a self, method: Method, url: Target<'a>, route: Route) -> Request<'a> {
        let plain = reqwest::Url::parse(url.as_str()).is_ok_and(|url| url.scheme() == "http");
        Request {
            http: if plain { &self.here } else { &self.http },
            rules: self.rules,
            route,
            method,
            url,
            credential: None,
            headers: Vec::new(),
            query: None,
            body: None,
            limit: MAX_ANSWER,
            timeout: None,
        }
    }
}

/// A client by Link's rules, but for what its caller names: each a call
/// that needs it, said where the client is made, so a review sees it.
#[derive(Debug)]
#[must_use = "a builder does nothing until it's built"]
pub struct Builder {
    timeout: Option<Duration>,
    connect: Duration,
    trust: Option<Vec<u8>>,
    direct: bool,
    #[cfg(unix)]
    socket: Option<std::path::PathBuf>,
    rules: Rules,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            timeout: Some(TIMEOUT),
            connect: CONNECT_TIMEOUT,
            trust: None,
            direct: false,
            #[cfg(unix)]
            socket: None,
            rules: Rules::default(),
        }
    }
}

impl Builder {
    /// Each call stopped past `timeout`, not [`TIMEOUT`].
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// No call stopped for its length, for one that streams for as long
    /// as it takes (an agent's files): it's given up only once nothing
    /// moves for `stall`. A body sent is given up when none of it goes for
    /// that long, and the answer then waited for as long as it takes; an
    /// answer read, when none of it comes for that long; a call with no
    /// body to send, when its answer doesn't start within it.
    pub fn streaming(mut self, stall: Duration) -> Self {
        self.timeout = None;
        self.rules.stall = Some(stall);
        self
    }

    /// A connection given up past `connect`, not [`CONNECT_TIMEOUT`].
    pub fn connect_timeout(mut self, connect: Duration) -> Self {
        self.connect = connect;
        self
    }

    /// For a service that isn't Link's (a cloud's API, a chat's): its
    /// answers are read open, a field their type doesn't name passed over,
    /// since such a service adds fields without a new version. A value of
    /// the wrong type is still refused.
    pub fn outside(mut self) -> Self {
        self.rules.outside = true;
        self
    }

    /// `https://` trusting only the certificates in `pem` (a private
    /// certificate authority), not the machine's.
    pub fn trusting(mut self, pem: &[u8]) -> Self {
        self.trust = Some(pem.to_vec());
        self
    }

    /// Through no proxy, `https://` too: a service on the caller's own
    /// network.
    pub fn direct(mut self) -> Self {
        self.direct = true;
        self
    }

    /// Reach a local HTTP service through its Unix socket, preserving the
    /// URL, typed protocol and request limits. No network proxy is used.
    #[cfg(unix)]
    pub fn unix_socket(mut self, socket: impl Into<std::path::PathBuf>) -> Self {
        self.socket = Some(socket.into());
        self.direct = true;
        self
    }

    /// Plain `http://` beyond this machine too, where its user said so
    /// (`lnk measure export set --url http://...`, `--allow-http`): what such a call
    /// carries, its credential too, crosses that network in the clear, as
    /// they chose. Through no proxy. Names under `localhost` stay this
    /// machine's.
    pub fn plain_http(mut self) -> Self {
        self.rules.plain = Plain::Said;
        self
    }

    /// Plain `http://` to a cloud's metadata server too, which has no TLS:
    /// `169.254.169.254`, on the machine's own link, or
    /// `metadata.google.internal`, found there alone, never by DNS. Such a
    /// call carries no credential, and goes through no proxy; any other
    /// plain `http://` beyond this machine is refused.
    pub fn metadata_server(mut self) -> Self {
        self.rules.plain = Plain::Metadata;
        self
    }

    /// `https://` alone, even to this machine: for a call whose
    /// credential goes nowhere in the clear, whatever address it was given.
    pub fn https_only(mut self) -> Self {
        self.rules.https = true;
        self
    }

    /// To public addresses alone, `https://` alone, and through no proxy
    /// (which would find the name itself): for an address someone else
    /// gave (a share's file), which mustn't make this machine reach
    /// itself or its own network. A name is found as usual, and only its
    /// public addresses kept ([`is_public`]).
    pub fn public_only(mut self) -> Self {
        self.rules.public = true;
        self.direct = true;
        self
    }

    /// Up to `hops` redirects followed, each to `https://` alone: for a
    /// public file whose address moves (a release's asset). A call on such
    /// a client carries no credential, which a redirect would carry
    /// elsewhere.
    pub fn follow_redirects(mut self, hops: usize) -> Self {
        self.rules.redirects = hops;
        self
    }

    /// The client.
    pub fn build(self) -> Result<Client, Error> {
        #[cfg(unix)]
        if self.socket.is_some() && self.rules.public {
            return Err(Error::Setup(
                "a local socket cannot serve a public-only client".into(),
            ));
        }
        // reqwest's TLS needs it. Errors only if one is already installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let trust = match &self.trust {
            Some(pem) => {
                let certs = reqwest::Certificate::from_pem_bundle(pem)
                    .map_err(|e| Error::Setup(format!("its certificates: {e}")))?;
                if certs.is_empty() {
                    return Err(Error::Setup("its certificates: none given".into()));
                }
                Some(certs)
            }
            None => None,
        };
        let hops = self.rules.redirects;
        let builder = || {
            let mut builder = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .connect_timeout(match self.timeout {
                    Some(timeout) => self.connect.min(timeout),
                    None => self.connect,
                })
                .redirect(match hops {
                    0 => reqwest::redirect::Policy::none(),
                    hops => reqwest::redirect::Policy::custom(move |attempt| {
                        if attempt.previous().len() > hops {
                            attempt.error(format!("more than {hops} redirects"))
                        } else if attempt.url().scheme() != "https" {
                            attempt.error("a redirect to other than https://")
                        } else {
                            attempt.follow()
                        }
                    }),
                });
            if let Some(timeout) = self.timeout {
                builder = builder.timeout(timeout);
            }
            if let Some(certs) = &trust {
                builder = builder.tls_certs_only(certs.clone());
            }
            #[cfg(unix)]
            if let Some(socket) = &self.socket {
                builder = builder.unix_socket(socket.clone()).no_proxy();
            }
            builder
        };
        let built = |builder: reqwest::ClientBuilder| {
            builder
                .build()
                .map_err(|e| Error::Setup(e.without_url().to_string()))
        };
        let mut http = match self.direct {
            true => builder().no_proxy(),
            false => builder(),
        };
        if self.rules.public {
            http = http.dns_resolver(Arc::new(PublicOnly));
        }
        Ok(Client {
            http: built(http)?,
            // A proxy (`HTTP_PROXY`, `ALL_PROXY`) would carry it, in the
            // clear, off this machine.
            here: built(builder().no_proxy().dns_resolver(Arc::new(Loopback {
                beyond: self.rules.plain,
            })))?,
            rules: self.rules,
        })
    }
}

impl Rules {
    /// Where its calls may go, as [`Error::Insecure`] says.
    pub(super) fn only(&self) -> &'static str {
        if self.public {
            "https:// to a public address"
        } else if self.https {
            "https://"
        } else {
            match self.plain {
                Plain::Here => "https://, or http:// on this machine",
                Plain::Metadata => "https://, or http:// on this machine or to its metadata server",
                Plain::Said => "https://, or http://",
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_local_socket_cannot_bypass_public_address_restrictions() {
        for builder in [
            Client::builder()
                .public_only()
                .unix_socket("/fixture/socket"),
            Client::builder()
                .unix_socket("/fixture/socket")
                .public_only(),
        ] {
            assert!(matches!(builder.build(), Err(Error::Setup(_))));
        }
    }
}
