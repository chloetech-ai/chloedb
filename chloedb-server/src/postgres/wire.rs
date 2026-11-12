//! A Postgres client: the frontend/backend protocol (3.0), as much of it as
//! following a database takes. It connects over TCP, with TLS as libpq's
//! `sslmode` asks; signs in with SCRAM-SHA-256, MD5, a password in the
//! clear, or none (trust); runs statements by the simple query protocol,
//! reads `COPY ... TO STDOUT`, and, on a connection opened for logical
//! replication (`replication=database`), runs the replication commands and
//! streams a slot's changes ([`Conn::start_replication`]).
//!
//! Values come back as text, as Postgres writes them with `DateStyle` ISO,
//! `TimeZone` UTC and `extra_float_digits` 3, which the connection asks for
//! when it starts (and the walsender's output plugin then writes its values
//! with too).

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
use tokio::net::TcpStream;

/// A failure of a Postgres connection or statement.
#[derive(Clone, Debug)]
pub struct Error {
    pub message: String,
    /// The SQLSTATE of an error the server sent (`42704`, ...).
    pub code: Option<String>,
    /// The connection failed (or was refused at sign-in), rather than a
    /// statement on it: connecting again may help.
    pub io: bool,
}

impl Error {
    fn new(m: impl Into<String>) -> Error {
        Error { message: m.into(), code: None, io: false }
    }

    fn io(m: impl Into<String>) -> Error {
        Error { message: m.into(), code: None, io: true }
    }

    /// Whether the server refused with SQLSTATE `code`.
    pub fn is(&self, code: &str) -> bool {
        self.code.as_deref() == Some(code)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// libpq's `sslmode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SslMode {
    Disable,
    /// TLS when the server offers it, unchecked (libpq's default).
    Prefer,
    /// TLS, unchecked unless a root certificate is given (then as `VerifyCa`).
    Require,
    /// TLS with a certificate signed by the root (or a public CA's).
    VerifyCa,
    /// `VerifyCa`, for the host's name too.
    VerifyFull,
}

/// Where and how to connect: from a URL,
/// `postgres://user:password@host:port/database?sslmode=require`.
#[derive(Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub database: String,
    pub ssl: SslMode,
    /// PEM certificates the server's is checked against (`sslrootcert`).
    pub root_cert: Option<Vec<u8>>,
    pub application_name: String,
    pub connect_timeout: Duration,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.shown())
    }
}

/// `%XX` escapes decoded.
fn decode_part(s: &str) -> Result<String, Error> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3).ok_or_else(|| Error::new(format!("{s:?}: a % stands before two hex digits")))?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| Error::new(format!("{s:?}: a % stands before two hex digits")))?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| Error::new(format!("{s:?} is not UTF-8 once decoded")))
}

impl Config {
    /// A `postgres://` (or `postgresql://`) URL: user, password, host,
    /// port (5432), database (the user's name), and of its parameters
    /// `sslmode`, `sslrootcert` (a file), `application_name` and
    /// `connect_timeout` (seconds). Without a password in the URL,
    /// `PGPASSWORD` gives it.
    pub fn parse(url: &str) -> Result<Config, Error> {
        let rest = url
            .strip_prefix("postgres://")
            .or_else(|| url.strip_prefix("postgresql://"))
            .ok_or_else(|| Error::new("the source is a postgres:// URL: postgres://user:password@host:5432/database"))?;
        let (main, query) = match rest.split_once('?') {
            Some((m, q)) => (m, Some(q)),
            None => (rest, None),
        };
        let (auth, hostpart) = match main.rsplit_once('@') {
            Some((a, h)) => (Some(a), h),
            None => (None, main),
        };
        let (hostport, database) = match hostpart.split_once('/') {
            Some((h, d)) => (h, decode_part(d)?),
            None => (hostpart, String::new()),
        };
        let (host, port) = if let Some(h) = hostport.strip_prefix('[') {
            // [::1]:5432
            let (h, p) = h.split_once(']').ok_or_else(|| Error::new("an IPv6 host is written [address]"))?;
            (h.to_string(), p.strip_prefix(':').filter(|p| !p.is_empty()))
        } else {
            match hostport.rsplit_once(':') {
                Some((h, p)) => (h.to_string(), Some(p)),
                None => (hostport.to_string(), None),
            }
        };
        let port = match port {
            Some(p) => p.parse().map_err(|_| Error::new(format!("{p:?} is not a port")))?,
            None => 5432,
        };
        let (user, password) = match auth {
            Some(a) => match a.split_once(':') {
                Some((u, p)) => (decode_part(u)?, Some(decode_part(p)?)),
                None => (decode_part(a)?, None),
            },
            None => (std::env::var("PGUSER").unwrap_or_else(|_| "postgres".into()), None),
        };
        let mut c = Config {
            host: if host.is_empty() { "localhost".into() } else { decode_part(&host)? },
            port,
            database: if database.is_empty() { user.clone() } else { database },
            password: password.or_else(|| std::env::var("PGPASSWORD").ok()),
            user,
            // Set below, once the host is known, unless the URL names it.
            ssl: SslMode::Prefer,
            root_cert: None,
            application_name: "chloedb-server".into(),
            connect_timeout: Duration::from_secs(10),
        };
        let mut ssl_named = false;
        for kv in query.into_iter().flat_map(|q| q.split('&')).filter(|kv| !kv.is_empty()) {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            let v = decode_part(v)?;
            match k {
                "sslmode" => {
                    ssl_named = true;
                    c.ssl = match v.as_str() {
                        "disable" => SslMode::Disable,
                        "allow" | "prefer" => SslMode::Prefer,
                        "require" => SslMode::Require,
                        "verify-ca" => SslMode::VerifyCa,
                        "verify-full" => SslMode::VerifyFull,
                        other => return Err(Error::new(format!("sslmode {other:?} is disable, prefer, require, verify-ca or verify-full"))),
                    }
                }
                "sslrootcert" => c.root_cert = Some(std::fs::read(&v).map_err(|e| Error::new(format!("sslrootcert {v}: {e}")))?),
                "application_name" => c.application_name = v,
                "connect_timeout" => c.connect_timeout = Duration::from_secs(v.parse().map_err(|_| Error::new(format!("connect_timeout {v:?} is seconds")))?),
                "user" => c.user = v,
                "password" => c.password = Some(v),
                "host" => c.host = v,
                "port" => c.port = v.parse().map_err(|_| Error::new(format!("{v:?} is not a port")))?,
                "dbname" => c.database = v,
                // Others (target_session_attrs, ...) are libpq's choices
                // this client does not make.
                _ => tracing::warn!(parameter = k, "a parameter of the Postgres URL this client does not use"),
            }
        }
        // libpq's default, prefer, falls back to plain text when the server
        // (or someone on the path) answers that it takes no TLS: the
        // password's proof and every row would then cross the network in
        // the clear. Off this machine, TLS is required unless the URL says
        // otherwise.
        if !is_loopback(&c.host) {
            match c.ssl {
                _ if !ssl_named => c.ssl = SslMode::Require,
                SslMode::Disable | SslMode::Prefer => tracing::warn!(host = %c.host, "sslmode lets the connection to Postgres be in plain text: the password's proof and every row may be read on the way (require or verify-full would not)"),
                _ => {}
            }
        }
        Ok(c)
    }

    /// The URL without its password, to show.
    pub fn shown(&self) -> String {
        format!("postgres://{}@{}:{}/{}", self.user, self.host, self.port, self.database)
    }
}

/// Whether `host` is this machine: `localhost` or a loopback address.
fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send + Sync {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Sync> Io for T {}

type Stream = Box<dyn Io>;

/// Rows of a statement's answer, as text (`None` for NULL).
#[derive(Clone, Debug, Default)]
pub struct Rows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
}

impl Rows {
    /// Column `name` of row `r`.
    pub fn get(&self, r: usize, name: &str) -> Option<&str> {
        let i = self.columns.iter().position(|c| c == name)?;
        self.rows.get(r)?.get(i)?.as_deref()
    }
}

/// A connection.
pub struct Conn {
    rd: BufReader<ReadHalf<Stream>>,
    wr: WriteHalf<Stream>,
    /// `server_version_num`, as 160004.
    pub version: u32,
    /// `server_encoding`.
    pub encoding: String,
}

/// One message from the server: its type and its body.
type Msg = (u8, Vec<u8>);

/// The most a message from the server may hold (a row of a COPY, an error).
const MAX_MESSAGE: usize = 1 << 30;

impl Conn {
    /// Connects and signs in; for `replication`, a connection of logical
    /// replication (`replication=database`), which runs the replication
    /// commands as well as SQL.
    pub async fn connect(cfg: &Config, replication: bool) -> Result<Conn, Error> {
        let addr = format!("{}:{}", cfg.host, cfg.port);
        let tcp = tokio::time::timeout(cfg.connect_timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| Error::io(format!("{}: no answer in {} s", cfg.shown(), cfg.connect_timeout.as_secs())))?
            .map_err(|e| Error::io(format!("{}: {e}", cfg.shown())))?;
        let _ = tcp.set_nodelay(true);
        let stream = tls(tcp, cfg).await?;
        let (r, w) = tokio::io::split(stream);
        let mut c = Conn { rd: BufReader::with_capacity(1 << 16, r), wr: w, version: 0, encoding: String::new() };
        let mut params = vec![
            ("user", cfg.user.as_str()),
            ("database", cfg.database.as_str()),
            ("application_name", cfg.application_name.as_str()),
            ("client_encoding", "UTF8"),
            ("DateStyle", "ISO, YMD"),
            ("TimeZone", "UTC"),
            ("extra_float_digits", "3"),
        ];
        if replication {
            params.push(("replication", "database"));
        }
        let mut body = Vec::new();
        body.extend_from_slice(&196608u32.to_be_bytes());
        for (k, v) in params {
            body.extend_from_slice(k.as_bytes());
            body.push(0);
            body.extend_from_slice(v.as_bytes());
            body.push(0);
        }
        body.push(0);
        let mut m = ((body.len() + 4) as u32).to_be_bytes().to_vec();
        m.extend(body);
        c.write_raw(&m).await?;
        c.sign_in(cfg).await?;
        loop {
            let (t, b) = c.recv().await?;
            match t {
                b'Z' => break,
                b'S' => c.parameter(&b),
                b'K' | b'N' | b'v' => {}
                b'E' => return Err(Error { io: true, ..server_error(&b) }),
                other => return Err(Error::io(format!("{}: message {:?} while starting", cfg.shown(), other as char))),
            }
        }
        Ok(c)
    }

    fn parameter(&mut self, b: &[u8]) {
        let mut r = Reader::new(b);
        let (Ok(k), Ok(v)) = (r.cstr(), r.cstr()) else { return };
        match k.as_str() {
            "server_version" => {
                // "16.4 (Ubuntu ...)", "10.23": major * 10000 + minor.
                let mut parts = v.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).map(|p| p.parse::<u32>().unwrap_or(0));
                let major = parts.next().unwrap_or(0);
                let minor = parts.next().unwrap_or(0);
                self.version = if major >= 10 { major * 10000 + minor } else { major * 10000 + minor * 100 };
            }
            "server_encoding" => self.encoding = v,
            _ => {}
        }
    }

    async fn sign_in(&mut self, cfg: &Config) -> Result<(), Error> {
        let password = || cfg.password.clone().ok_or_else(|| Error::io(format!("{}: the server asks for a password: give it in the URL or PGPASSWORD", cfg.shown())));
        loop {
            let (t, b) = self.recv().await?;
            if t == b'E' {
                return Err(Error { io: true, ..server_error(&b) });
            }
            if t != b'R' {
                return Err(Error::io(format!("{}: message {:?} while signing in", cfg.shown(), t as char)));
            }
            let mut r = Reader::new(&b);
            match r.i32()? {
                0 => return Ok(()),
                3 => {
                    let p = password()?;
                    self.send(b'p', &cstring(&p)).await?;
                }
                5 => {
                    let salt = r.bytes(4)?;
                    let p = password()?;
                    let inner = hex(&md5::Md5::digest(format!("{p}{}", cfg.user).as_bytes()));
                    let mut h = md5::Md5::new();
                    h.update(inner.as_bytes());
                    h.update(salt);
                    self.send(b'p', &cstring(&format!("md5{}", hex(&h.finalize())))).await?;
                }
                10 => {
                    let mut mechanisms = Vec::new();
                    while let Ok(m) = r.cstr() {
                        if m.is_empty() {
                            break;
                        }
                        mechanisms.push(m);
                    }
                    if !mechanisms.iter().any(|m| m == "SCRAM-SHA-256") {
                        return Err(Error::io(format!("{}: the server signs in by {} only, and this client speaks SCRAM-SHA-256", cfg.shown(), mechanisms.join(", "))));
                    }
                    self.scram(&password()?).await?;
                }
                other => return Err(Error::io(format!("{}: the server signs in by a method this client does not speak (AuthenticationRequest {other}): use scram-sha-256, md5 or password in pg_hba.conf", cfg.shown()))),
            }
        }
    }

    /// SCRAM-SHA-256 (RFC 5802 and 7677), without channel binding.
    async fn scram(&mut self, password: &str) -> Result<(), Error> {
        use rand::RngCore;
        let mut n = [0u8; 18];
        rand::thread_rng().fill_bytes(&mut n);
        let b64 = base64::engine::general_purpose::STANDARD;
        let nonce = b64.encode(n);
        let first_bare = format!("n=,r={nonce}");
        let first = format!("n,,{first_bare}");
        let mut m = cstring("SCRAM-SHA-256");
        m.extend_from_slice(&(first.len() as i32).to_be_bytes());
        m.extend_from_slice(first.as_bytes());
        self.send(b'p', &m).await?;
        let server_first = self.sasl(11).await?;
        let mut snonce = None;
        let mut salt = None;
        let mut iterations = None;
        for part in server_first.split(',') {
            match part.split_once('=') {
                Some(("r", v)) => snonce = Some(v.to_string()),
                Some(("s", v)) => salt = b64.decode(v).ok(),
                Some(("i", v)) => iterations = v.parse::<u32>().ok(),
                _ => {}
            }
        }
        let (Some(snonce), Some(salt), Some(iterations)) = (snonce, salt, iterations) else {
            return Err(Error::io("SCRAM: the server's first message lacks its nonce, salt or iterations"));
        };
        if !snonce.starts_with(&nonce) || iterations == 0 {
            return Err(Error::io("SCRAM: the server's nonce does not extend this client's"));
        }
        let salted = hi(password.as_bytes(), &salt, iterations);
        let client_key = hmac256(&salted, b"Client Key");
        let stored_key = Sha256::digest(client_key);
        let without_proof = format!("c=biws,r={snonce}");
        let auth_message = format!("{first_bare},{server_first},{without_proof}");
        let signature = hmac256(&stored_key, auth_message.as_bytes());
        let proof: Vec<u8> = client_key.iter().zip(signature.iter()).map(|(a, b)| a ^ b).collect();
        let last = format!("{without_proof},p={}", b64.encode(proof));
        self.send(b'p', last.as_bytes()).await?;
        let server_final = self.sasl(12).await?;
        let server_key = hmac256(&salted, b"Server Key");
        let expect = b64.encode(hmac256(&server_key, auth_message.as_bytes()));
        match server_final.strip_prefix("v=") {
            Some(v) if v == expect => Ok(()),
            _ => Err(Error::io(format!("SCRAM: the server did not prove it knows the password ({server_final})"))),
        }
    }

    /// The text of an AuthenticationSASLContinue (11) or SASLFinal (12).
    async fn sasl(&mut self, want: i32) -> Result<String, Error> {
        let (t, b) = self.recv().await?;
        if t == b'E' {
            return Err(Error { io: true, ..server_error(&b) });
        }
        let mut r = Reader::new(&b);
        if t != b'R' || r.i32()? != want {
            return Err(Error::io("SCRAM: an unexpected message from the server"));
        }
        Ok(String::from_utf8_lossy(r.rest()).into_owned())
    }

    async fn write_raw(&mut self, b: &[u8]) -> Result<(), Error> {
        self.wr.write_all(b).await.map_err(|e| Error::io(format!("writing to Postgres: {e}")))?;
        self.wr.flush().await.map_err(|e| Error::io(format!("writing to Postgres: {e}")))
    }

    async fn send(&mut self, t: u8, body: &[u8]) -> Result<(), Error> {
        self.write_raw(&frame(t, body)).await
    }

    /// The next message from the server.
    async fn recv(&mut self) -> Result<Msg, Error> {
        read_msg(&mut self.rd).await
    }

    /// Runs `sql` (statements separated by `;` too), answering the rows of
    /// the last that returns rows.
    pub async fn query(&mut self, sql: &str) -> Result<Rows, Error> {
        self.send(b'Q', &cstring(sql)).await?;
        let mut out = Rows::default();
        let mut cur = Rows::default();
        let mut err = None;
        loop {
            let (t, b) = self.recv().await?;
            match t {
                b'T' => {
                    let mut r = Reader::new(&b);
                    let n = r.i16()? as usize;
                    cur = Rows::default();
                    for _ in 0..n {
                        cur.columns.push(r.cstr()?);
                        r.bytes(18)?;
                    }
                }
                b'D' => cur.rows.push(data_row(&b)?),
                b'C' => {
                    if !cur.columns.is_empty() {
                        out = std::mem::take(&mut cur);
                    }
                }
                b'E' => err = Some(server_error(&b)),
                b'Z' => break,
                b'S' => self.parameter(&b),
                b'H' | b'W' | b'G' => return Err(Error::new("a COPY in a statement run by query: use copy_out")),
                _ => {}
            }
        }
        match err {
            Some(e) => Err(e),
            None => Ok(out),
        }
    }

    /// Starts `COPY ... TO STDOUT`: its rows are read with [`Conn::copy_data`].
    pub async fn copy_out(&mut self, sql: &str) -> Result<(), Error> {
        self.send(b'Q', &cstring(sql)).await?;
        loop {
            let (t, b) = self.recv().await?;
            match t {
                b'H' => return Ok(()),
                b'E' => {
                    let e = server_error(&b);
                    self.until_ready().await?;
                    return Err(e);
                }
                b'S' => self.parameter(&b),
                b'N' => {}
                other => return Err(Error::new(format!("COPY: message {:?} from the server", other as char))),
            }
        }
    }

    /// The next bytes of a COPY's rows; `None` once it is done.
    pub async fn copy_data(&mut self) -> Result<Option<Vec<u8>>, Error> {
        loop {
            let (t, b) = self.recv().await?;
            match t {
                b'd' => return Ok(Some(b)),
                b'c' => {}
                b'C' => {
                    self.until_ready().await?;
                    return Ok(None);
                }
                b'E' => {
                    let e = server_error(&b);
                    self.until_ready().await?;
                    return Err(e);
                }
                _ => {}
            }
        }
    }

    async fn until_ready(&mut self) -> Result<(), Error> {
        loop {
            if self.recv().await?.0 == b'Z' {
                return Ok(());
            }
        }
    }

    /// Runs `START_REPLICATION ...`, and from then on streams: the reader
    /// gives the server's messages (XLogData, keepalives), and the writer
    /// sends this client's status.
    pub async fn start_replication(mut self, command: &str) -> Result<(ReplReader, ReplWriter), Error> {
        self.send(b'Q', &cstring(command)).await?;
        loop {
            let (t, b) = self.recv().await?;
            match t {
                b'W' => break,
                b'E' => return Err(server_error(&b)),
                b'S' => self.parameter(&b),
                b'N' => {}
                other => return Err(Error::new(format!("START_REPLICATION: message {:?} from the server", other as char))),
            }
        }
        Ok((ReplReader { rd: self.rd }, ReplWriter { wr: self.wr }))
    }

    /// Closes the connection.
    pub async fn close(mut self) {
        let _ = self.send(b'X', &[]).await;
        let _ = self.wr.shutdown().await;
    }
}

/// The server's side of a replication stream.
pub struct ReplReader {
    rd: BufReader<ReadHalf<Stream>>,
}

impl ReplReader {
    /// The next CopyData's bytes; `None` once the server ends the stream.
    pub async fn next(&mut self) -> Result<Option<Vec<u8>>, Error> {
        loop {
            let (t, b) = read_msg(&mut self.rd).await?;
            match t {
                b'd' => return Ok(Some(b)),
                b'c' | b'C' | b'Z' => return Ok(None),
                b'E' => return Err(Error { io: true, ..server_error(&b) }),
                _ => {}
            }
        }
    }
}

/// This client's side of a replication stream.
pub struct ReplWriter {
    wr: WriteHalf<Stream>,
}

impl ReplWriter {
    /// Sends one CopyData message.
    pub async fn send(&mut self, data: &[u8]) -> Result<(), Error> {
        self.wr.write_all(&frame(b'd', data)).await.map_err(|e| Error::io(format!("writing to Postgres: {e}")))?;
        self.wr.flush().await.map_err(|e| Error::io(format!("writing to Postgres: {e}")))
    }
}

async fn read_msg<R: AsyncRead + Unpin>(rd: &mut R) -> Result<Msg, Error> {
    let mut head = [0u8; 5];
    rd.read_exact(&mut head).await.map_err(|e| Error::io(format!("reading from Postgres: {e}")))?;
    let len = u32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if !(4..=MAX_MESSAGE).contains(&len) {
        return Err(Error::io(format!("a message from Postgres of {len} bytes")));
    }
    let mut body = vec![0u8; len - 4];
    rd.read_exact(&mut body).await.map_err(|e| Error::io(format!("reading from Postgres: {e}")))?;
    Ok((head[0], body))
}

fn frame(t: u8, body: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(body.len() + 5);
    m.push(t);
    m.extend_from_slice(&((body.len() + 4) as u32).to_be_bytes());
    m.extend_from_slice(body);
    m
}

fn cstring(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hmac256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC takes a key of any length");
    m.update(msg);
    m.finalize().into_bytes().into()
}

/// SCRAM's Hi(): PBKDF2 with HMAC-SHA-256, one block.
fn hi(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut s = salt.to_vec();
    s.extend_from_slice(&1u32.to_be_bytes());
    let mut u = hmac256(password, &s);
    let mut out = u;
    for _ in 1..iterations {
        u = hmac256(password, &u);
        for (o, x) in out.iter_mut().zip(u.iter()) {
            *o ^= x;
        }
    }
    out
}

fn data_row(b: &[u8]) -> Result<Vec<Option<String>>, Error> {
    let mut r = Reader::new(b);
    let n = r.i16()? as usize;
    let mut row = Vec::with_capacity(n);
    for _ in 0..n {
        let len = r.i32()?;
        row.push(match len {
            -1 => None,
            l => Some(String::from_utf8_lossy(r.bytes(l as usize)?).into_owned()),
        });
    }
    Ok(row)
}

/// An ErrorResponse's fields as an error: its message, detail and hint.
fn server_error(b: &[u8]) -> Error {
    let mut message = String::new();
    let mut detail = None;
    let mut hint = None;
    let mut code = None;
    let mut severity = String::new();
    let mut r = Reader::new(b);
    while let Ok(t) = r.u8() {
        if t == 0 {
            break;
        }
        let Ok(v) = r.cstr() else { break };
        match t {
            b'M' => message = v,
            b'D' => detail = Some(v),
            b'H' => hint = Some(v),
            b'C' => code = Some(v),
            b'V' => severity = v,
            _ => {}
        }
    }
    let mut m = format!("Postgres: {message}");
    if let Some(d) = detail {
        m.push_str(&format!(" ({d})"));
    }
    if let Some(h) = hint {
        m.push_str(&format!(". {h}"));
    }
    Error { message: m, code, io: severity == "FATAL" || severity == "PANIC" }
}

/// Reads a message's fields.
pub struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(b: &'a [u8]) -> Reader<'a> {
        Reader { b, at: 0 }
    }

    fn short() -> Error {
        Error::new("a message from Postgres ended too soon")
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.b.len()).ok_or_else(Self::short)?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }

    pub fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.bytes(1)?[0])
    }

    pub fn i16(&mut self) -> Result<i16, Error> {
        let b = self.bytes(2)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }

    pub fn i32(&mut self) -> Result<i32, Error> {
        let b = self.bytes(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        Ok(self.i32()? as u32)
    }

    pub fn u64(&mut self) -> Result<u64, Error> {
        let b = self.bytes(8)?;
        Ok(u64::from_be_bytes(b.try_into().expect("8 bytes")))
    }

    /// A NUL-terminated string.
    pub fn cstr(&mut self) -> Result<String, Error> {
        let rest = &self.b[self.at..];
        let n = rest.iter().position(|&c| c == 0).ok_or_else(Self::short)?;
        let s = String::from_utf8_lossy(&rest[..n]).into_owned();
        self.at += n + 1;
        Ok(s)
    }

    pub fn rest(&mut self) -> &'a [u8] {
        let s = &self.b[self.at..];
        self.at = self.b.len();
        s
    }
}

/// Quotes `s` as an SQL string literal.
pub fn literal(s: &str) -> String {
    if s.contains('\\') {
        format!("E'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
    } else {
        format!("'{}'", s.replace('\'', "''"))
    }
}

/// Quotes `s` as an SQL identifier.
pub fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// The stream after TLS is negotiated as `cfg.ssl` asks.
async fn tls(mut tcp: TcpStream, cfg: &Config) -> Result<Stream, Error> {
    if cfg.ssl == SslMode::Disable {
        return Ok(Box::new(tcp));
    }
    // SSLRequest.
    let mut m = 8u32.to_be_bytes().to_vec();
    m.extend_from_slice(&80877103u32.to_be_bytes());
    tcp.write_all(&m).await.map_err(|e| Error::io(format!("{}: {e}", cfg.shown())))?;
    let mut answer = [0u8; 1];
    tcp.read_exact(&mut answer).await.map_err(|e| Error::io(format!("{}: {e}", cfg.shown())))?;
    match answer[0] {
        b'S' => {}
        b'N' if cfg.ssl == SslMode::Prefer => return Ok(Box::new(tcp)),
        b'N' => return Err(Error::io(format!("{}: the server does not take TLS connections, and sslmode asks for TLS", cfg.shown()))),
        other => return Err(Error::io(format!("{}: answered {other} to an SSLRequest", cfg.shown()))),
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions().map_err(|e| Error::io(format!("TLS: {e}")))?;
    let verify = match cfg.ssl {
        SslMode::VerifyCa | SslMode::VerifyFull => true,
        SslMode::Require => cfg.root_cert.is_some(),
        _ => false,
    };
    let config = if verify {
        let mut roots = rustls::RootCertStore::empty();
        match &cfg.root_cert {
            Some(pem) => {
                for c in rustls_pemfile::certs(&mut &pem[..]) {
                    let c = c.map_err(|e| Error::io(format!("sslrootcert: {e}")))?;
                    roots.add(c).map_err(|e| Error::io(format!("sslrootcert: {e}")))?;
                }
            }
            None => roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()),
        }
        let webpki = rustls::client::WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone()).build().map_err(|e| Error::io(format!("TLS: {e}")))?;
        let v: Arc<dyn rustls::client::danger::ServerCertVerifier> = match cfg.ssl {
            SslMode::VerifyFull => webpki,
            _ => Arc::new(AnyName(webpki)),
        };
        builder.dangerous().with_custom_certificate_verifier(v).with_no_client_auth()
    } else {
        builder.dangerous().with_custom_certificate_verifier(Arc::new(Unchecked(provider))).with_no_client_auth()
    };
    let name = rustls::pki_types::ServerName::try_from(cfg.host.clone()).map_err(|e| Error::io(format!("{}: {e}", cfg.host)))?;
    let s = tokio_rustls::TlsConnector::from(Arc::new(config)).connect(name, tcp).await.map_err(|e| Error::io(format!("{}: TLS: {e}", cfg.shown())))?;
    Ok(Box::new(s))
}

/// `sslmode=prefer` and `require` without a root: the connection is
/// encrypted, the certificate not checked (as libpq).
#[derive(Debug)]
struct Unchecked(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for Unchecked {
    fn verify_server_cert(&self, _: &rustls::pki_types::CertificateDer<'_>, _: &[rustls::pki_types::CertificateDer<'_>], _: &rustls::pki_types::ServerName<'_>, _: &[u8], _: rustls::pki_types::UnixTime) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(&self, m: &[u8], c: &rustls::pki_types::CertificateDer<'_>, d: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self, m: &[u8], c: &rustls::pki_types::CertificateDer<'_>, d: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// `sslmode=verify-ca`: the chain checked, the name not.
#[derive(Debug)]
struct AnyName(Arc<rustls::client::WebPkiServerVerifier>);

impl rustls::client::danger::ServerCertVerifier for AnyName {
    fn verify_server_cert(&self, e: &rustls::pki_types::CertificateDer<'_>, i: &[rustls::pki_types::CertificateDer<'_>], n: &rustls::pki_types::ServerName<'_>, o: &[u8], t: rustls::pki_types::UnixTime) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        match self.0.verify_server_cert(e, i, n, o, t) {
            Err(rustls::Error::InvalidCertificate(rustls::CertificateError::NotValidForName | rustls::CertificateError::NotValidForNameContext { .. })) => Ok(rustls::client::danger::ServerCertVerified::assertion()),
            r => r,
        }
    }

    fn verify_tls12_signature(&self, m: &[u8], c: &rustls::pki_types::CertificateDer<'_>, d: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls12_signature(m, c, d)
    }

    fn verify_tls13_signature(&self, m: &[u8], c: &rustls::pki_types::CertificateDer<'_>, d: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls13_signature(m, c, d)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_parse_as_libpq_reads_them() {
        let c = Config::parse("postgres://app:p%40ss@db.example:6543/shop?sslmode=require&application_name=x").unwrap();
        assert_eq!((c.user.as_str(), c.password.as_deref(), c.host.as_str(), c.port, c.database.as_str()), ("app", Some("p@ss"), "db.example", 6543, "shop"));
        assert_eq!((c.ssl, c.application_name.as_str()), (SslMode::Require, "x"));
        assert_eq!(c.shown(), "postgres://app@db.example:6543/shop");
        let c = Config::parse("postgresql://u@[::1]/").unwrap();
        assert_eq!((c.host.as_str(), c.port, c.database.as_str(), c.ssl), ("::1", 5432, "u", SslMode::Prefer));
        assert!(Config::parse("http://x").is_err());
        assert!(Config::parse("postgres://u@h/d?sslmode=sometimes").is_err());
    }

    /// D-B15: with no sslmode, a server elsewhere on the network is
    /// asked for TLS and refused without it; a server on this machine
    /// may still answer in plain text, as libpq's `prefer` lets it.
    #[test]
    fn with_no_sslmode_a_remote_server_needs_tls() {
        let mode = |url: &str| Config::parse(url).unwrap().ssl;
        assert_eq!(mode("postgres://u@db.example.com/d"), SslMode::Require);
        assert_eq!(mode("postgres://u@10.0.0.7:6543/d"), SslMode::Require);
        assert_eq!(mode("postgres://u@localhost/d?host=db.example.com"), SslMode::Require);
        for local in ["postgres://u@localhost/d", "postgres://u@127.0.0.1/d", "postgres://u@[::1]/d", "postgres:///d"] {
            assert_eq!(mode(local), SslMode::Prefer, "{local}");
        }
        // Asked for by name, prefer and disable are kept.
        assert_eq!(mode("postgres://u@db.example.com/d?sslmode=prefer"), SslMode::Prefer);
        assert_eq!(mode("postgres://u@db.example.com/d?sslmode=disable"), SslMode::Disable);
    }

    #[test]
    fn scram_matches_rfc_7677() {
        // RFC 7677's example: user "user", password "pencil".
        let salt = base64::engine::general_purpose::STANDARD.decode("W22ZaJ0SNY7soEsUEjb6gQ==").unwrap();
        let salted = hi(b"pencil", &salt, 4096);
        let client_key = hmac256(&salted, b"Client Key");
        let stored_key = Sha256::digest(client_key);
        let auth = "n=user,r=rOprNGfwEbeRWgbNEkqO,r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096,c=biws,r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0";
        let signature = hmac256(&stored_key, auth.as_bytes());
        let proof: Vec<u8> = client_key.iter().zip(signature.iter()).map(|(a, b)| a ^ b).collect();
        assert_eq!(base64::engine::general_purpose::STANDARD.encode(proof), "dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ=");
        let server = hmac256(&hmac256(&salted, b"Server Key"), auth.as_bytes());
        assert_eq!(base64::engine::general_purpose::STANDARD.encode(server), "6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=");
    }

    #[test]
    fn literals_and_identifiers_are_quoted() {
        assert_eq!(literal("it's"), "'it''s'");
        assert_eq!(literal(r"a\b"), r"E'a\\b'");
        assert_eq!(ident(r#"we"ird"#), r#""we""ird""#);
    }
}
