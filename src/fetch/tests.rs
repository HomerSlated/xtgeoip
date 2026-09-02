//! Unit tests for [`crate::fetch`].
//!
//! Kept in a child module file rather than inside the parent so that
//! `src/fetch.rs` — which carries a guardian signature — does not change
//! when a test does. A child module sees its parent's private items, so
//! this needs no visibility widening: the parent's public API is
//! unchanged by the move.

// ── mock HTTP server (#88)
// ───────────────────────────────────────────────
//
// The network path was previously untestable in practice. It needs no
// production seam: `fetch()` takes its URL from `config.maxmind.url`
// and enforces no scheme, so pointing that at a local listener
// drives the real code — `resolve_version`, `check_download_size`,
// `acquire_remote_archive`, `send_with_retry` — with nothing stubbed.
//
// Hand-rolled rather than a dev-dependency: the request shapes are
// trivial (GET, no body) and this project is deliberately
// conservative about dependency surface. Responses close the
// connection, so no keep-alive handling is needed.
use std::{
    io::BufRead,
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use zip::write::{SimpleFileOptions, ZipWriter};

use super::*;

#[derive(Clone, Debug)]
struct MockRequest {
    line: String,
    headers: Vec<(String, String)>,
}

impl MockRequest {
    /// Header lookup by lowercased name.
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// The request target, e.g. `/?suffix=zip`.
    fn target(&self) -> &str {
        self.line.split_whitespace().nth(1).unwrap_or("")
    }
}

struct MockReply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl MockReply {
    fn new(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    fn header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.to_string(), v.to_string()));
        self
    }
}

struct MockServer {
    port: u16,
    seen: Arc<Mutex<Vec<MockRequest>>>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MockServer {
    fn start<F>(router: F) -> Self
    where
        F: Fn(&MockRequest) -> MockReply + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        listener.set_nonblocking(true).expect("nonblocking");

        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (seen_t, stop_t) = (Arc::clone(&seen), Arc::clone(&stop));

        let handle = thread::spawn(move || {
            while !stop_t.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if let Some(req) = read_request(&stream) {
                            seen_t.lock().unwrap().push(req.clone());
                            let _ = write_reply(stream, router(&req));
                        }
                    }
                    // Nonblocking accept with a short poll: no hang if the
                    // client makes fewer requests than expected.
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            port,
            seen,
            stop,
            handle: Some(handle),
        }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn requests(&self) -> Vec<MockRequest> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn read_request(stream: &TcpStream) -> Option<MockRequest> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut reader = io::BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let request_line = line.trim_end().to_string();
    if request_line.is_empty() {
        return None;
    }

    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).ok()? == 0 {
            break;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }

    Some(MockRequest {
        line: request_line,
        headers,
    })
}

fn write_reply(mut stream: TcpStream, reply: MockReply) -> io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        reply.body.len()
    );
    for (k, v) in &reply.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(&reply.body)?;
    stream.flush()
}

/// A `Config` whose MaxMind URL points at `url` and whose archive dir is
/// `archive_dir`. Credentials are no longer part of `Config` (#103) —
/// `fetch()` takes them as separate, already-decrypted arguments; tests
/// pass their own dummy `"123456"`/`"test-license-key"` directly.
fn mock_config(url: &str, archive_dir: &Path) -> crate::config::Config {
    crate::config::Config {
        paths: crate::config::Paths {
            archive_dir: archive_dir.display().to_string(),
            archive_prune: 3,
            output_dir: archive_dir.display().to_string(),
        },
        maxmind: crate::config::MaxMind {
            url: url.to_string(),
            credentials: None,
        },
        logging: None,
        processing: None,
    }
}

/// Reply carrying a valid-looking archive filename, so `resolve_version`
/// succeeds and the flow reaches the download.
fn versioned_reply(body: &[u8]) -> MockReply {
    MockReply::ok(body.to_vec()).header(
        "Content-Disposition",
        "attachment; filename=\"GeoLite2-Country-CSV_20260101.zip\"",
    )
}

/// Credentials must reach MaxMind as HTTP basic auth, and nowhere else.
#[test]
fn remote_fetch_sends_basic_auth() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start(|_| MockReply::new(401));
    let cfg = mock_config(&server.url(), dir.path());

    let _ = fetch(&cfg, FetchMode::Remote, "123456", "test-license-key");

    let reqs = server.requests();
    assert!(!reqs.is_empty(), "server saw no request");
    let auth = reqs[0]
        .header("authorization")
        .expect("no Authorization header sent");
    assert!(
        auth.starts_with("Basic "),
        "expected HTTP basic auth, got {auth:?}"
    );
    assert!(
        reqs[0].target().contains("suffix=zip"),
        "unexpected target {:?}",
        reqs[0].target()
    );
}

/// A non-success status must abort with the status reported, not proceed.
#[test]
fn non_success_status_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start(|_| MockReply::new(401));
    let cfg = mock_config(&server.url(), dir.path());

    let err = fetch(&cfg, FetchMode::Remote, "123456", "test-license-key")
        .expect_err("must fail");
    assert!(
        err.to_string().contains("401"),
        "error should name the status: {err}"
    );
}

/// 429 is a client error, so `send_with_retry` must NOT retry it —
/// hammering a rate limit is exactly the wrong response, and MaxMind's cap
/// is the real constraint on this project's test runs.
#[test]
fn rate_limit_is_not_retried() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start(|_| MockReply::new(429));
    let cfg = mock_config(&server.url(), dir.path());

    let _ = fetch(&cfg, FetchMode::Remote, "123456", "test-license-key");
    assert_eq!(
        server.requests().len(),
        1,
        "a rate-limit response must not be retried"
    );
}

/// `resolve_version` reads the version from `Content-Disposition`; without
/// it there is no version, and proceeding would mis-name the archive.
#[test]
fn missing_content_disposition_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start(|_| MockReply::ok("body"));
    let cfg = mock_config(&server.url(), dir.path());

    let err = fetch(&cfg, FetchMode::Remote, "123456", "test-license-key")
        .expect_err("must fail");
    assert!(
        err.to_string().contains("Content-Disposition"),
        "unhelpful error: {err}"
    );
}

/// The header is attacker-influenced and names a file on disk. These are
/// the traversal shapes the guardian audit reasoned about statically; here
/// they are executed.
#[test]
fn hostile_content_disposition_is_rejected() {
    for hostile in [
        "attachment; filename=\"../../etc/passwd\"",
        "attachment; filename=\"/etc/shadow\"",
        "attachment; filename=\"..\"",
        "attachment; filename=\"\"",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let h = hostile.to_string();
        let server = MockServer::start(move |_| {
            MockReply::ok("x").header("Content-Disposition", &h)
        });
        let cfg = mock_config(&server.url(), dir.path());

        assert!(
            fetch(&cfg, FetchMode::Remote, "123456", "test-license-key")
                .is_err(),
            "accepted hostile Content-Disposition {hostile:?}"
        );
        // Nothing may be written outside the archive dir, and nothing
        // resembling a traversal target inside it.
        assert!(
            !Path::new("/tmp/passwd").exists(),
            "traversal escaped the archive dir"
        );
    }
}

/// End-to-end proof of the `PartialDownload` guard: a checksum mismatch
/// must fail *and* leave no `.part` file behind.
#[test]
fn checksum_mismatch_leaves_no_partial_download() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start(|req| {
        if req.target().contains("sha256") {
            // Deliberately not the hash of the body below.
            MockReply::ok(
                "0000000000000000000000000000000000000000000000000000000000000000  x.zip",
            )
        } else {
            versioned_reply(b"not-a-real-archive")
        }
    });
    let cfg = mock_config(&server.url(), dir.path());

    let err = fetch(&cfg, FetchMode::Remote, "123456", "test-license-key")
        .expect_err("must fail");
    assert!(
        err.to_string().contains("Checksum verification failed"),
        "unexpected error: {err}"
    );

    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".part"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "partial download left behind: {leftovers:?}"
    );
}

/// The property `redirect_policy` cannot express, and the reason the R2 hop
/// is safe (#101). `reqwest` strips `Authorization` cross-origin; since
/// MaxMind redirects to a different origin on *every* fetch, that stripping
/// is load-bearing continuously. Two servers are two origins.
#[test]
fn credentials_are_not_forwarded_across_origin_redirect() {
    let dir = tempfile::tempdir().unwrap();
    let target = MockServer::start(|_| MockReply::new(401));
    let target_url = target.url();
    let origin = MockServer::start(move |_| {
        MockReply::new(302).header("Location", &target_url)
    });
    let cfg = mock_config(&origin.url(), dir.path());

    let _ = fetch(&cfg, FetchMode::Remote, "123456", "test-license-key");

    let followed = target.requests();
    assert_eq!(followed.len(), 1, "redirect was not followed");
    assert!(
        followed[0].header("authorization").is_none(),
        "Authorization was forwarded across origins — the license key would \
         leak to the redirect target"
    );
    // And it *was* sent to the intended origin.
    assert!(
        origin.requests()[0].header("authorization").is_some(),
        "credentials never reached the configured endpoint"
    );
}

/// The hop limit `redirect_policy` does express. The server redirects to
/// itself, so without a bound this would never terminate; the location is
/// shared so the router can name a URL that does not exist until after the
/// server has bound its port.
#[test]
fn redirect_loop_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let location = Arc::new(Mutex::new(String::new()));
    let for_router = Arc::clone(&location);

    let server = MockServer::start(move |_| {
        let to = for_router.lock().unwrap().clone();
        MockReply::new(302).header("Location", &to)
    });
    *location.lock().unwrap() = server.url();

    let cfg = mock_config(&server.url(), dir.path());
    assert!(
        fetch(&cfg, FetchMode::Remote, "123456", "test-license-key").is_err(),
        "an unbounded redirect chain must fail rather than loop"
    );
    assert!(
        server.requests().len() <= MAX_REDIRECTS + 1,
        "followed more than {MAX_REDIRECTS} redirects: {} requests",
        server.requests().len()
    );
}

// ── concurrent-fetch safety (#100) ───────────────────────────────────────

/// The temp path must not be derivable from the version alone. That was
/// guardian F-1: two `xtgeoip fetch` processes resolving the same version
/// shared one `.part` file, so their writes could interleave and either
/// one's guard could delete a path the other was still writing.
#[test]
fn part_path_is_not_shared_between_processes() {
    let archive =
        Path::new("/var/lib/xt_geoip/GeoLite2-Country-CSV_20260714.zip");
    let shared = archive.with_extension("zip.part");
    let mine = part_path(archive);

    assert_ne!(mine, shared, "the .part path is still version-derived");
    assert!(
        mine.to_string_lossy().contains(&process::id().to_string()),
        "expected this process's id in {mine:?}"
    );
    // Same directory, so renaming onto archive_path stays an atomic
    // same-filesystem operation rather than a cross-device copy.
    assert_eq!(mine.parent(), archive.parent());
}

/// A `.part` file must stay invisible to archive discovery *and* to
/// pruning. That combination is what made the pre-#99 leaks immortal, and
/// changing the name must not disturb either half.
#[test]
fn part_path_is_neither_discoverable_nor_prunable() {
    let archive =
        Path::new("/var/lib/xt_geoip/GeoLite2-Country-CSV_20260714.zip");
    let mine = part_path(archive);
    let name = mine.file_name().unwrap().to_string_lossy().into_owned();

    assert!(
        !name.ends_with(".zip"),
        "{name} would be found as an archive"
    );
    assert!(
        !name.ends_with(".zip.sha256"),
        "{name} looks like a checksum"
    );
    assert!(name.ends_with(".part"), "{name} lost its .part suffix");
}

// ── partial-download cleanup ─────────────────────────────────────────────

/// The default: any early return removes the `.part` file. Six error
/// paths in `acquire_remote_archive` previously leaked it, and because
/// `prune_csv_archives` matches only `.zip`/`.zip.sha256`, leaked files
/// were never reclaimed.
#[test]
fn partial_download_is_removed_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("archive.zip.part");
    fs::write(&part, b"half a download").unwrap();

    drop(PartialDownload::new(&part));
    assert!(!part.exists(), "armed guard must delete the .part file");
}

/// After a successful rename the file has moved; the guard must not chase
/// it (and must not delete anything at the old path).
#[test]
fn disarmed_guard_keeps_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("archive.zip.part");
    fs::write(&part, b"complete").unwrap();

    let mut guard = PartialDownload::new(&part);
    guard.disarm();
    drop(guard);

    assert!(part.exists(), "disarmed guard must not delete");
}

/// A guard whose file never got created — e.g. `File::create` failed —
/// must drop silently rather than warn about a missing file.
#[test]
fn missing_partial_download_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("never-created.zip.part");
    assert!(!part.exists());

    drop(PartialDownload::new(&part));
    assert!(!part.exists());
}

// ── zip fixtures ─────────────────────────────────────────────────────────

/// One entry to place in a test zip.
struct E {
    name: &'static str,
    size: usize,
    exec: bool,
}

fn clean(name: &'static str, size: usize) -> E {
    E {
        name,
        size,
        exec: false,
    }
}

/// Build a zip at `path` from `entries`. Names are written verbatim — the
/// writer does not normalise `..` for plain `start_file` — so this can
/// craft the malicious entries the security scanner must reject.
fn write_zip(path: &Path, entries: &[E]) {
    let file = File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    for e in entries {
        let mut opts = SimpleFileOptions::default();
        if e.exec {
            opts = opts.unix_permissions(0o755);
        }
        zip.start_file(e.name, opts).unwrap();
        zip.write_all(&vec![b'x'; e.size]).unwrap();
    }
    zip.finish().unwrap();
}

fn open_zip(path: &Path) -> ZipArchive<File> {
    ZipArchive::new(File::open(path).unwrap()).unwrap()
}

// ── parse_content_disposition_filename ───────────────────────────────────

#[test]
fn cd_unquoted_filename() {
    assert_eq!(
        parse_content_disposition_filename(
            "attachment; filename=GeoLite2-Country-CSV_20260227.zip"
        ),
        Some("GeoLite2-Country-CSV_20260227.zip")
    );
}

#[test]
fn cd_quoted_filename() {
    assert_eq!(
        parse_content_disposition_filename(
            "attachment; filename=\"GeoLite2-Country-CSV_20260227.zip\""
        ),
        Some("GeoLite2-Country-CSV_20260227.zip")
    );
}

#[test]
fn cd_case_insensitive_key() {
    assert_eq!(
        parse_content_disposition_filename("attachment; FileName=x.zip"),
        Some("x.zip")
    );
}

#[test]
fn cd_missing_filename_is_none() {
    assert_eq!(parse_content_disposition_filename("attachment"), None);
}

#[test]
fn cd_empty_filename_is_none() {
    assert_eq!(
        parse_content_disposition_filename("attachment; filename="),
        None
    );
    assert_eq!(
        parse_content_disposition_filename("attachment; filename=\"\""),
        None
    );
}

// ── find_latest_local_csv_archive ────────────────────────────────────────

#[test]
fn find_latest_picks_highest_version() {
    let dir = TempDir::new().unwrap();
    for date in ["20260101", "20260315", "20260227"] {
        fs::write(
            dir.path().join(format!("GeoLite2-Country-CSV_{date}.zip")),
            b"",
        )
        .unwrap();
    }
    let (path, version) = find_latest_local_csv_archive(dir.path()).unwrap();
    assert_eq!(version.as_str(), "20260315");
    assert!(path.ends_with("GeoLite2-Country-CSV_20260315.zip"));
}

#[test]
fn find_latest_skips_nonmatching_names() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("GeoLite2-Country-CSV_20260101.zip"), b"")
        .unwrap();
    // wrong product, checksum sidecar, and unrelated file — all ignored
    fs::write(dir.path().join("GeoLite2-City-CSV_20260901.zip"), b"").unwrap();
    fs::write(
        dir.path().join("GeoLite2-Country-CSV_20260101.zip.sha256"),
        b"",
    )
    .unwrap();
    fs::write(dir.path().join("notes.txt"), b"").unwrap();
    let (_, version) = find_latest_local_csv_archive(dir.path()).unwrap();
    assert_eq!(version.as_str(), "20260101");
}

#[test]
fn find_latest_errors_when_empty() {
    let dir = TempDir::new().unwrap();
    assert!(find_latest_local_csv_archive(dir.path()).is_err());
}

// ── verify_zip_magic ─────────────────────────────────────────────────────

#[test]
fn zip_magic_accepts_real_zip() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("real.zip");
    write_zip(&path, &[clean("a.csv", 4)]);
    assert!(verify_zip_magic(&path).is_ok());
}

#[test]
fn zip_magic_rejects_non_zip() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("fake.zip");
    fs::write(&path, b"not a zip at all").unwrap();
    assert!(verify_zip_magic(&path).is_err());
}

// ── scan_zip_entries (security scanner) ──────────────────────────────────

#[test]
fn scan_rejects_path_traversal() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("evil.zip");
    write_zip(&path, &[clean("../escape.txt", 4)]);
    let err = scan_zip_entries(&mut open_zip(&path)).unwrap_err();
    assert!(err.to_string().contains("traversal"), "{err}");
}

#[test]
fn scan_rejects_absolute_path() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("abs.zip");
    // drive-letter form triggers the `:/` branch and survives the writer
    // verbatim (a leading `/` can be stripped by some tooling).
    write_zip(&path, &[clean("C:/evil.txt", 4)]);
    let err = scan_zip_entries(&mut open_zip(&path)).unwrap_err();
    assert!(err.to_string().contains("absolute"), "{err}");
}

#[test]
fn scan_rejects_executable_bits() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("exec.zip");
    write_zip(
        &path,
        &[E {
            name: "run.sh",
            size: 4,
            exec: true,
        }],
    );
    let err = scan_zip_entries(&mut open_zip(&path)).unwrap_err();
    assert!(err.to_string().contains("executable"), "{err}");
}

#[test]
fn scan_detects_common_prefix() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nested.zip");
    write_zip(
        &path,
        &[clean("GeoLite2/a.csv", 4), clean("GeoLite2/b.csv", 4)],
    );
    assert_eq!(
        scan_zip_entries(&mut open_zip(&path)).unwrap(),
        Some("GeoLite2".to_string())
    );
}

#[test]
fn scan_flat_archive_has_no_prefix() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("flat.zip");
    write_zip(&path, &[clean("a.csv", 4), clean("b.csv", 4)]);
    assert_eq!(scan_zip_entries(&mut open_zip(&path)).unwrap(), None);
}

// ── extract_archive_to_temp_capped ───────────────────────────────────────

#[test]
fn extract_within_budget_succeeds() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ok.zip");
    write_zip(&path, &[clean("data.csv", 1_000)]);
    let out = extract_archive_to_temp_capped(&path, 10_000)
        .expect("extraction within budget should succeed");
    assert!(out.path().join("data.csv").exists());
}

#[test]
fn extract_exceeding_budget_bails() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bomb.zip");
    write_zip(&path, &[clean("data.csv", 1_000)]);
    let err = extract_archive_to_temp_capped(&path, 100)
        .expect_err("extraction past the budget must be refused");
    assert!(err.to_string().contains("decompression bomb"), "{err}");
}

#[test]
fn extract_strips_common_prefix() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nested.zip");
    write_zip(&path, &[clean("GeoLite2/data.csv", 20)]);
    let out = extract_archive_to_temp_capped(&path, 10_000).unwrap();
    assert!(out.path().join("data.csv").exists());
    assert!(!out.path().join("GeoLite2").exists());
}

#[test]
fn extract_rejects_traversal() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("evil.zip");
    write_zip(&path, &[clean("../escape.txt", 4)]);
    assert!(extract_archive_to_temp_capped(&path, 10_000).is_err());
}

// ── verify_cached_archive ────────────────────────────────────────────────

#[test]
fn cached_archive_matching_checksum_is_true() {
    let dir = TempDir::new().unwrap();
    let archive = dir.path().join("a.zip");
    let checksum = dir.path().join("a.zip.sha256");
    fs::write(&archive, b"payload").unwrap();
    let hash = format!("{:x}", Sha256::digest(b"payload"));
    fs::write(&checksum, format!("{hash}  a.zip\n")).unwrap();
    assert!(verify_cached_archive(&archive, &checksum).unwrap());
}

#[test]
fn cached_archive_mismatch_is_false() {
    let dir = TempDir::new().unwrap();
    let archive = dir.path().join("a.zip");
    let checksum = dir.path().join("a.zip.sha256");
    fs::write(&archive, b"payload").unwrap();
    fs::write(&checksum, format!("{}  a.zip\n", "0".repeat(64))).unwrap();
    assert!(!verify_cached_archive(&archive, &checksum).unwrap());
}

#[test]
fn cached_archive_bad_checksum_format_errors() {
    let dir = TempDir::new().unwrap();
    let archive = dir.path().join("a.zip");
    let checksum = dir.path().join("a.zip.sha256");
    fs::write(&archive, b"payload").unwrap();
    fs::write(&checksum, b"").unwrap();
    assert!(verify_cached_archive(&archive, &checksum).is_err());
}

// ── CSV validation ───────────────────────────────────────────────────────

#[test]
fn locations_csv_valid_ok() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("loc.csv");
    fs::write(
        &path,
        "geoname_id,country_iso_code,continent_code\n6252001,US,NA\n",
    )
    .unwrap();
    assert!(validate_locations_csv(&path).is_ok());
}

#[test]
fn locations_csv_missing_column_bails() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("loc.csv");
    fs::write(&path, "geoname_id,country_iso_code\n6252001,US\n").unwrap();
    assert!(validate_locations_csv(&path).is_err());
}

#[test]
fn blocks_csv_valid_ok() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("blk.csv");
    fs::write(
        &path,
        "network,geoname_id,is_anonymous_proxy,is_satellite_provider\n1.0.0.0/\
         24,6252001,0,0\n",
    )
    .unwrap();
    assert!(validate_blocks_csv(&path).is_ok());
}

#[test]
fn blocks_csv_missing_column_bails() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("blk.csv");
    fs::write(
        &path,
        "network,geoname_id,is_anonymous_proxy\n1.0.0.0/24,6252001,0\n",
    )
    .unwrap();
    assert!(validate_blocks_csv(&path).is_err());
}
