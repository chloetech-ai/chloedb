//! Files on S3 and S3-compatible stores (feature `s3`): imports read
//! `s3://bucket/key` URLs ([`Input`]) and exports write them ([`Output`]).
//!
//! A URL names one object, or every object under a prefix: one that ends
//! in `/`, or that names no object (as `s3://bucket/events` names
//! `events/part-0.parquet`, `events/year=2026/part-1.parquet`..). They are
//! read into a temporary directory, keeping the paths under the prefix,
//! so a directory of Parquet files with Hive partitions arrives as one.
//! `s3a://` URLs, which Spark writes, are the same.
//!
//! The store is configured as the AWS tools are, by environment:
//! `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` (and `AWS_SESSION_TOKEN`),
//! or a web identity (`AWS_WEB_IDENTITY_TOKEN_FILE` and `AWS_ROLE_ARN`, as
//! EKS gives pods), ECS task credentials, or EC2 instance metadata;
//! `AWS_REGION` (or `AWS_DEFAULT_REGION`); and, for a store other than
//! AWS S3 (DigitalOcean Spaces, MinIO, Cloudflare R2, Ceph..),
//! `AWS_ENDPOINT_URL` (`AWS_ALLOW_HTTP=true` for a plain `http://` one).
//! Profiles in `~/.aws` are not read.

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures::{StreamExt, TryStreamExt};
use object_store::aws::{AmazonS3, AmazonS3Builder};
use object_store::path::Path as Key;
use object_store::{ObjectStore, ObjectStoreExt, PutMode, PutOptions, PutPayload, UpdateVersion, WriteMultipart};

use crate::error::{Error, Result};

/// Objects read or written at once.
const CONCURRENCY: usize = 8;

/// Files up to this size are written in one request; larger ones in parts
/// of this size, [`CONCURRENCY`] at once.
const PART: usize = 16 << 20;

/// Whether `s` is a URL this module reads and writes (`s3://` or
/// `s3a://`, in any case).
pub fn is_url(s: &str) -> bool {
    let l = s.get(..6).unwrap_or(s).to_ascii_lowercase();
    l.starts_with("s3://") || l.starts_with("s3a://")
}

/// A URL's bucket and key (the key may be empty).
fn parse(url: &str) -> Result<(String, String)> {
    let rest = url.split_once("://").map(|(_, r)| r).filter(|_| is_url(url)).ok_or_else(|| Error::Schema(format!("{url}: not an s3:// URL")))?;
    let (bucket, key) = rest.split_once('/').unwrap_or((rest, ""));
    if bucket.is_empty() {
        return Err(Error::Schema(format!("{url}: no bucket (s3://bucket/key)")));
    }
    Ok((bucket.to_string(), key.to_string()))
}

fn store(url: &str, bucket: &str) -> Result<AmazonS3> {
    AmazonS3Builder::from_env().with_bucket_name(bucket).build().map_err(|e| failed(url, e))
}

/// `k` as a key, as it is written: object_store's `Path::from` would
/// percent-encode characters such as `%` and `~`, naming another object.
fn key(url: &str, k: &str) -> Result<Key> {
    Key::parse(k).map_err(|e| failed(url, e))
}

fn failed(url: &str, e: impl std::fmt::Display) -> Error {
    Error::Io(std::io::Error::other(format!("{url}: {e}")))
}

/// Runs `f` on a runtime of its own, on a thread of its own: callers may
/// be inside a runtime already (a server's handler), where blocking on
/// another would panic.
fn run<T: Send>(f: impl std::future::Future<Output = Result<T>> + Send) -> Result<T> {
    std::thread::scope(|s| {
        s.spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
            rt.block_on(f)
        })
        .join()
        .unwrap_or_else(|_| Err(Error::Io(std::io::Error::other("the S3 client panicked"))))
    })
}

/// A directory removed, with what it holds, when dropped.
#[derive(Debug)]
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<TempDir> {
        static N: AtomicU64 = AtomicU64::new(0);
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let p = std::env::temp_dir().join(format!("strata-s3-{}-{t}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&p)?;
        Ok(TempDir(p))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The last segment of a key (`events` of `data/events/`), or `default`.
fn last_segment<'a>(key: &'a str, default: &'a str) -> &'a str {
    key.trim_end_matches('/').rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(default)
}

/// `rel` as a path under a directory, if it stays under it.
fn relative(rel: &str) -> Option<PathBuf> {
    let p = Path::new(rel);
    p.components().all(|c| matches!(c, Component::Normal(_))).then(|| p.to_path_buf()).filter(|p| !p.as_os_str().is_empty())
}

/// A file or directory to read: a local path, or the objects an `s3://`
/// URL names, read into a temporary directory that is removed when this
/// is dropped.
#[derive(Debug)]
pub struct Input {
    path: PathBuf,
    _tmp: Option<TempDir>,
}

impl Input {
    /// `s` as a local path, or the objects of the URL `s` read
    /// ([`super::objects`]).
    pub fn open(s: &str) -> Result<Input> {
        if !is_url(s) {
            return Ok(Input { path: PathBuf::from(s), _tmp: None });
        }
        let (bucket, key) = parse(s)?;
        let tmp = TempDir::new()?;
        let path = run(fetch(s, &bucket, &key, &tmp.0))?;
        Ok(Input { path, _tmp: Some(tmp) })
    }

    /// Where it is on this machine.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether it was read from a URL.
    pub fn fetched(&self) -> bool {
        self._tmp.is_some()
    }
}

/// Reads the object `key`, or the objects under the prefix `key`, into
/// `dir`: the object's file, or a directory named for the prefix.
async fn fetch(url: &str, bucket: &str, key: &str, dir: &Path) -> Result<PathBuf> {
    let s3 = Arc::new(store(url, bucket)?);
    if !key.is_empty() && !key.ends_with('/') {
        let k = self::key(url, key)?;
        match s3.head(&k).await {
            Ok(_) => {
                let file = dir.join(last_segment(key, "object"));
                download(&s3, url, &k, &file).await?;
                return Ok(file);
            }
            Err(object_store::Error::NotFound { .. }) => {}
            Err(e) => return Err(failed(url, e)),
        }
    }
    let prefix = key.trim_end_matches('/');
    let root = dir.join(last_segment(prefix, bucket));
    std::fs::create_dir_all(&root)?;
    let under = (!prefix.is_empty()).then(|| self::key(url, prefix)).transpose()?;
    let listed: Vec<_> = s3.list(under.as_ref()).try_collect().await.map_err(|e| failed(url, e))?;
    let mut files = Vec::new();
    for o in listed {
        let k = o.location.to_string();
        let rel = if prefix.is_empty() { k.as_str() } else { k.strip_prefix(prefix).and_then(|r| r.strip_prefix('/')).unwrap_or("") };
        // Folder markers, which consoles create, hold nothing.
        if rel.is_empty() || rel.ends_with('/') {
            continue;
        }
        let rel = relative(rel).ok_or_else(|| failed(url, format!("object {k}: a key that leaves its prefix")))?;
        files.push((o.location, root.join(rel)));
    }
    if files.is_empty() {
        return Err(Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, format!("{url}: no such object, and no objects under it"))));
    }
    let downloads: Vec<_> = files.iter().map(|(k, f)| download(&s3, url, k, f)).collect();
    futures::stream::iter(downloads).buffer_unordered(CONCURRENCY).try_collect::<Vec<()>>().await?;
    Ok(root)
}

async fn download(s3: &AmazonS3, url: &str, key: &Key, file: &Path) -> Result<()> {
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut out = std::fs::File::create(file)?;
    let mut body = s3.get(key).await.map_err(|e| failed(url, format!("{key}: {e}")))?.into_stream();
    while let Some(chunk) = body.next().await {
        out.write_all(&chunk.map_err(|e| failed(url, format!("{key}: {e}")))?)?;
    }
    out.sync_all()?;
    Ok(())
}

/// A file or directory to write: a local path, or a temporary one whose
/// contents [`Output::finish`] writes to an `s3://` URL.
#[derive(Debug)]
pub struct Output {
    path: PathBuf,
    url: Option<String>,
    _tmp: Option<TempDir>,
}

impl Output {
    /// `s` as a local path, or a temporary path named as the URL's last
    /// segment is (`t.parquet` of `s3://bucket/exports/t.parquet`).
    pub fn new(s: &str) -> Result<Output> {
        if !is_url(s) {
            return Ok(Output { path: PathBuf::from(s), url: None, _tmp: None });
        }
        let (bucket, key) = parse(s)?;
        let tmp = TempDir::new()?;
        let path = tmp.0.join(last_segment(&key, &bucket));
        Ok(Output { path, url: Some(s.to_string()), _tmp: Some(tmp) })
    }

    /// Where to write it on this machine.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The URL it goes to, if it does.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// Writes what is at [`Output::path`] to the URL, if it has one: a
    /// file to the URL's object, a directory's files to objects under the
    /// URL's prefix, at their paths in it. An object already at the URL,
    /// or under it, is an error, as an existing local path is to an
    /// export, checked before anything is written. Returns the bytes
    /// written.
    pub fn finish(self) -> Result<u64> {
        let Some(url) = &self.url else { return Ok(0) };
        let (bucket, key) = parse(url)?;
        let mut files = Vec::new();
        let base = key.trim_end_matches('/');
        if self.path.is_dir() {
            let mut stack = vec![self.path.clone()];
            while let Some(d) = stack.pop() {
                for e in std::fs::read_dir(&d)? {
                    let p = e?.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else {
                        let rel = p.strip_prefix(&self.path).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                        files.push((if base.is_empty() { rel } else { format!("{base}/{rel}") }, p));
                    }
                }
            }
        } else {
            if base.is_empty() {
                return Err(Error::Schema(format!("{url}: a file needs a key (s3://bucket/name.parquet)")));
            }
            files.push((base.to_string(), self.path.clone()));
        }
        files.sort();
        run(async move {
            let s3 = Arc::new(store(url, &bucket)?);
            let taken: Vec<_> = if self.path.is_dir() {
                let under = (!base.is_empty()).then(|| self::key(url, base)).transpose()?;
                s3.list(under.as_ref()).take(1).try_collect().await.map_err(|e| failed(url, e))?
            } else {
                match s3.head(&self::key(url, base)?).await {
                    Ok(o) => vec![o],
                    Err(object_store::Error::NotFound { .. }) => Vec::new(),
                    Err(e) => return Err(failed(url, e)),
                }
            };
            if let Some(o) = taken.first() {
                return Err(Error::Io(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("{url} exists (object {})", o.location))));
            }
            let uploads: Vec<_> = files.iter().map(|(k, p)| upload(&s3, url, k, p)).collect();
            let sizes = futures::stream::iter(uploads).buffer_unordered(CONCURRENCY).try_collect::<Vec<u64>>().await?;
            Ok(sizes.iter().sum())
        })
    }
}

async fn upload(s3: &AmazonS3, url: &str, key: &str, file: &Path) -> Result<u64> {
    let size = std::fs::metadata(file)?.len();
    let k = self::key(url, key)?;
    if size as usize <= PART {
        s3.put(&k, PutPayload::from(std::fs::read(file)?)).await.map_err(|e| failed(url, format!("{key}: {e}")))?;
        return Ok(size);
    }
    let up = s3.put_multipart(&k).await.map_err(|e| failed(url, format!("{key}: {e}")))?;
    let mut w = WriteMultipart::new_with_chunk_size(up, PART);
    let mut f = std::fs::File::open(file)?;
    let mut buf = vec![0u8; PART];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        w.wait_for_capacity(CONCURRENCY).await.map_err(|e| failed(url, format!("{key}: {e}")))?;
        w.write(&buf[..n]);
    }
    w.finish().await.map_err(|e| failed(url, format!("{key}: {e}")))?;
    Ok(size)
}

/// A bucket, or a prefix of one (`s3://bucket/prefix/`), as the store of a
/// database's cold segments ([`crate::storage::cold`]): one client, with a
/// runtime of its own, for the database's life. Calls block the calling
/// thread, which may be a runtime's (the runtime is another).
pub struct Bucket {
    url: String,
    prefix: String,
    s3: AmazonS3,
    rt: Option<tokio::runtime::Runtime>,
}

impl std::fmt::Debug for Bucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bucket").field("url", &self.url).finish()
    }
}

impl Bucket {
    /// The bucket and prefix `url` names, configured by environment as
    /// the module says.
    pub fn open(url: &str) -> Result<Bucket> {
        let (bucket, prefix) = parse(url)?;
        let prefix = match prefix.trim_start_matches('/') {
            "" => String::new(),
            p if p.ends_with('/') => p.to_string(),
            p => format!("{p}/"),
        };
        let s3 = store(url, &bucket)?;
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("chloedb-s3").enable_all().build()?;
        Ok(Bucket { url: format!("s3://{bucket}/{prefix}"), prefix, s3, rt: Some(rt) })
    }

    fn call<T: Send + 'static>(&self, f: impl std::future::Future<Output = Result<T>> + Send + 'static) -> Result<T> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.rt.as_ref().expect("runtime until dropped").spawn(async move {
            let _ = tx.send(f.await);
        });
        rx.recv().unwrap_or_else(|_| Err(Error::Io(std::io::Error::other(format!("{}: the S3 client stopped", self.url)))))
    }

    fn at(&self, k: &str) -> (AmazonS3, String, String) {
        (self.s3.clone(), self.url.clone(), format!("{}{k}", self.prefix))
    }
}

impl Drop for Bucket {
    fn drop(&mut self) {
        // Dropping a runtime blocks, which a runtime's thread may not.
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

/// An error of `url`'s, `NotFound` when the object is missing.
fn failed_at(url: &str, k: &str, e: object_store::Error) -> Error {
    let kind = match e {
        object_store::Error::NotFound { .. } => std::io::ErrorKind::NotFound,
        _ => std::io::ErrorKind::Other,
    };
    Error::Io(std::io::Error::new(kind, format!("{url}{k}: {e}")))
}

impl crate::storage::cold::ObjectStore for Bucket {
    fn put_file(&self, k: &str, path: &Path) -> Result<()> {
        let (s3, url, full) = self.at(k);
        let path = path.to_path_buf();
        self.call(async move { upload(&s3, &url, &full, &path).await.map(|_| ()) })
    }

    fn put(&self, k: &str, bytes: &[u8]) -> Result<()> {
        let (s3, url, full) = self.at(k);
        let payload = PutPayload::from(bytes.to_vec());
        self.call(async move {
            let key = key(&url, &full)?;
            s3.put(&key, payload).await.map_err(|e| failed_at(&url, &full, e))?;
            Ok(())
        })
    }

    fn get_range(&self, k: &str, range: std::ops::Range<u64>) -> Result<Vec<u8>> {
        let (s3, url, full) = self.at(k);
        self.call(async move {
            let key = key(&url, &full)?;
            let b = s3.get_range(&key, range).await.map_err(|e| failed_at(&url, &full, e))?;
            Ok(b.to_vec())
        })
    }

    fn delete(&self, k: &str) -> Result<()> {
        let (s3, url, full) = self.at(k);
        self.call(async move {
            let key = key(&url, &full)?;
            match s3.delete(&key).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
                Err(e) => Err(failed_at(&url, &full, e)),
            }
        })
    }

    fn describe(&self) -> String {
        self.url.clone()
    }

    fn get(&self, k: &str) -> Result<Option<(Vec<u8>, String)>> {
        let (s3, url, full) = self.at(k);
        self.call(async move {
            let key = key(&url, &full)?;
            let got = match s3.get(&key).await {
                Ok(got) => got,
                Err(object_store::Error::NotFound { .. }) => return Ok(None),
                Err(e) => return Err(failed_at(&url, &full, e)),
            };
            let version = entity_tag(&url, &full, got.meta.e_tag.clone())?;
            let bytes = got.bytes().await.map_err(|e| failed_at(&url, &full, e))?;
            Ok(Some((bytes.to_vec(), version)))
        })
    }

    fn put_if_absent(&self, k: &str, bytes: &[u8]) -> Result<Option<String>> {
        self.put_when(k, bytes, PutMode::Create)
    }

    fn put_if_version(&self, k: &str, bytes: &[u8], version: &str) -> Result<Option<String>> {
        self.put_when(k, bytes, PutMode::Update(UpdateVersion { e_tag: Some(version.to_string()), version: None }))
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let (s3, url, full) = self.at(prefix);
        let own = self.prefix.len();
        self.call(async move {
            // object_store lists by whole path segments: list the deepest
            // "directory" the prefix names, and keep the keys that start
            // with all of it.
            let dir = match full.rfind('/') {
                Some(i) => Some(key(&url, &full[..i])?),
                None => None,
            };
            let mut keys = Vec::new();
            let mut listing = s3.list(dir.as_ref());
            while let Some(meta) = listing.next().await {
                let at = meta.map_err(|e| failed_at(&url, &full, e))?.location;
                if at.as_ref().starts_with(&full) {
                    keys.push(at.as_ref()[own..].to_string());
                }
            }
            Ok(keys)
        })
    }

    fn list_dirs(&self, prefix: &str) -> Result<Vec<String>> {
        let (s3, url, full) = self.at(prefix);
        self.call(async move {
            let dir = match full.trim_end_matches('/') {
                "" => None,
                d => Some(key(&url, d)?),
            };
            let listed = s3.list_with_delimiter(dir.as_ref()).await.map_err(|e| failed_at(&url, &full, e))?;
            let mut names: Vec<String> = listed.common_prefixes.iter().filter_map(|p| p.filename().map(str::to_string)).collect();
            names.sort();
            Ok(names)
        })
    }

    fn get_file(&self, k: &str, dest: &Path) -> Result<()> {
        let (s3, url, full) = self.at(k);
        let dest = dest.to_path_buf();
        self.call(async move {
            let key = key(&url, &full)?;
            let got = s3.get(&key).await.map_err(|e| failed_at(&url, &full, e))?;
            let mut parts = got.into_stream();
            let mut f = std::fs::File::create(&dest)?;
            while let Some(part) = parts.next().await {
                f.write_all(&part.map_err(|e| failed_at(&url, &full, e))?)?;
            }
            Ok(f.sync_all()?)
        })
    }
}

impl Bucket {
    /// A put under a condition: the object's new version, or `None` when
    /// the condition did not hold and nothing was written.
    fn put_when(&self, k: &str, bytes: &[u8], mode: PutMode) -> Result<Option<String>> {
        let (s3, url, full) = self.at(k);
        let payload = PutPayload::from(bytes.to_vec());
        self.call(async move {
            let key = key(&url, &full)?;
            let updating = matches!(mode, PutMode::Update(_));
            match s3.put_opts(&key, payload, PutOptions { mode, ..PutOptions::default() }).await {
                Ok(done) => Ok(Some(entity_tag(&url, &full, done.e_tag)?)),
                Err(object_store::Error::AlreadyExists { .. }) | Err(object_store::Error::Precondition { .. }) => Ok(None),
                // Replacing an object that is gone: its version is not the one asked for.
                Err(object_store::Error::NotFound { .. }) if updating => Ok(None),
                Err(e) => Err(failed_at(&url, &full, e)),
            }
        })
    }
}

/// An object's entity tag, which is its version: a store that gives none
/// cannot be written to under a condition.
fn entity_tag(url: &str, k: &str, tag: Option<String>) -> Result<String> {
    tag.ok_or_else(|| Error::Io(std::io::Error::other(format!("{url}{k}: the store gave the object no entity tag, which conditional writes need"))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert!(is_url("s3://b/k") && is_url("S3A://b/k") && !is_url("/tmp/x") && !is_url("s3:/x"));
        assert_eq!(parse("s3://b/a/b.parquet").unwrap(), ("b".into(), "a/b.parquet".into()));
        assert_eq!(parse("s3://b").unwrap(), ("b".into(), "".into()));
        assert!(parse("s3:///k").is_err());
        assert_eq!(last_segment("data/events/", "x"), "events");
        assert_eq!(last_segment("", "bucket"), "bucket");
        assert!(relative("../x").is_none() && relative("a/../../x").is_none() && relative("/etc/passwd").is_none());
        assert_eq!(relative("year=2026/part-0.parquet"), Some(PathBuf::from("year=2026/part-0.parquet")));
    }

    #[test]
    fn local_paths_pass_through() {
        let i = Input::open("/tmp/some.parquet").unwrap();
        assert_eq!(i.path(), Path::new("/tmp/some.parquet"));
        assert!(!i.fetched());
        let o = Output::new("out.parquet").unwrap();
        assert_eq!(o.path(), Path::new("out.parquet"));
        assert_eq!(o.finish().unwrap(), 0);
        let o = Output::new("s3://bucket/exports/t.parquet").unwrap();
        assert!(o.path().ends_with("t.parquet") && o.url() == Some("s3://bucket/exports/t.parquet"));
    }
}
