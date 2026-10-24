//! File search for the command bar (#55): the Windows Search index first,
//! a bounded folder walk when the index is not there.
//!
//! The index is read through the OLE DB provider `Search.CollatorDSO`
//! (`SystemIndex`, Windows Search SQL), the same one PowerToys Run and
//! Flow Launcher use. Queries run on one worker thread that keeps its
//! session open; a caller waits at most [`INDEX_BUDGET`] and then falls back.
//! Paths come from `System.ItemUrl`: `System.ItemPathDisplay` is localized
//! (`C:\Benutzer\…` on German Windows) and cannot be opened.
//!
//! User text never reaches the SQL unescaped: string literals double `'`,
//! `LIKE` patterns bracket `%`, `_` and `[`, and `CONTAINS` terms drop `"`
//! and `*` (see [`like_escape`], [`contains_terms`]).

use crate::settings_pages::SettingsHit;
use serde::Serialize;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How long a caller waits for the index (the first query after idle can
/// take ~0.7 s).
pub const INDEX_BUDGET: Duration = Duration::from_millis(1500);
/// Default and largest number of file results.
pub const DEFAULT_LIMIT: usize = 40;
pub const MAX_LIMIT: usize = 100;
/// Longest query that is sent to the index.
const MAX_QUERY_CHARS: usize = 120;
const MAX_TERMS: usize = 6;

const WALK_BUDGET: Duration = Duration::from_secs(2);
const WALK_MAX_DEPTH: usize = 6;
const WALK_MAX_RESULTS: usize = 50;
const WALK_SKIP_DIRS: &[&str] = &[".git", "node_modules", "appdata", ".cursor", "target"];

/// One file or folder found.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FileHit {
    /// `"file"` or `"folder"`.
    pub kind: &'static str,
    pub path: String,
    pub name: String,
    /// The folder it is in.
    pub folder: String,
    /// Last change, ms since 1970 (UTC), when known.
    pub modified: Option<i64>,
    /// `"name"` or `"contents"`: what matched.
    #[serde(rename = "match")]
    pub matched: &'static str,
}

/// `cli_search`'s answer.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResults {
    pub query: String,
    /// `"index"`, `"walk"` (the index was not there), or `"off"` (no file
    /// search: not asked for, or the index failed and no walk was wanted).
    pub source: &'static str,
    pub settings: Vec<SettingsHit>,
    pub files: Vec<FileHit>,
    /// Time taken, in ms.
    pub ms: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct SearchOptions {
    pub files: bool,
    pub settings: bool,
    /// Walk the user folders when the index fails (slow: up to 2 s).
    pub fallback: bool,
    pub limit: usize,
}

// ── SQL ─────────────────────────────────────────────────────────────────

/// The query as sent: control characters gone, spaces collapsed, at most
/// [`MAX_QUERY_CHARS`] characters.
pub(crate) fn clean_query(q: &str) -> String {
    let s: String = q.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_QUERY_CHARS).collect()
}

/// A Windows Search SQL string literal: `'…'` with `'` doubled.
pub(crate) fn sql_string(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Text that a `LIKE` pattern matches literally: `%`, `_` and `[` are
/// wildcards there, so each goes into brackets.
pub(crate) fn like_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '%' => out.push_str("[%]"),
            '_' => out.push_str("[_]"),
            '[' => out.push_str("[[]"),
            c => out.push(c),
        }
    }
    out
}

/// The `CONTAINS` condition for the query's words: each a quoted prefix
/// term (`"rech*"`), joined with `AND`. Words are split where the index's
/// word breaker splits them (anything but letters and digits), so a term
/// holds letters and digits only: nothing in it needs escaping. Pieces of
/// one character are dropped (`"a*"` matches half the disk and takes
/// seconds). `None` when no word is left.
pub(crate) fn contains_terms(q: &str) -> Option<String> {
    let terms: Vec<String> = clean_query(q)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2)
        .take(MAX_TERMS)
        .map(|w| format!("\"{}*\"", w.chars().take(64).collect::<String>()))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" AND "))
}

const COLUMNS: &str =
    "System.ItemUrl, System.FileName, System.ItemNameDisplay, System.Kind, System.DateModified";

/// Hidden and dot folders, app data, package folders, build output: what the
/// folder walk never visited either.
const EXCLUDE: &str = "SCOPE='file:' \
    AND NOT System.ItemUrl LIKE '%/.%' \
    AND NOT System.ItemUrl LIKE '%/AppData/%' \
    AND NOT System.ItemUrl LIKE '%/node[_]modules/%' \
    AND NOT System.ItemUrl LIKE '%/target/%' \
    AND NOT System.ItemUrl LIKE '%/$Recycle.Bin/%'";

/// Files and folders whose name starts with the query or has a word that
/// does, best ranked first.
pub(crate) fn names_sql(q: &str, top: usize) -> Option<String> {
    let q = clean_query(q);
    if q.is_empty() {
        return None;
    }
    let like = format!("System.FileName LIKE {}", sql_string(&format!("{}%", like_escape(&q))));
    let words = contains_terms(&q)
        .map(|t| format!(" OR CONTAINS(System.FileName, {})", sql_string(&t)))
        .unwrap_or_default();
    Some(format!(
        "SELECT TOP {top} {COLUMNS} FROM SystemIndex WHERE {EXCLUDE} \
         AND ({like}{words}) ORDER BY System.Search.Rank DESC"
    ))
}

/// Files whose contents (or any indexed property) have the query's words.
pub(crate) fn contents_sql(q: &str, top: usize) -> Option<String> {
    let terms = contains_terms(q)?;
    Some(format!(
        "SELECT TOP {top} {COLUMNS} FROM SystemIndex WHERE {EXCLUDE} \
         AND CONTAINS(*, {terms}) ORDER BY System.Search.Rank DESC",
        terms = sql_string(&terms),
    ))
}

/// `file:C:/Users/me/a b.txt` → `C:\Users\me\a b.txt`. `None` for anything
/// that is not a local drive path (`file://server/…` would reach the network).
pub(crate) fn url_to_path(url: &str) -> Option<String> {
    let rest = url.get(..5).filter(|p| p.eq_ignore_ascii_case("file:")).map(|_| &url[5..])?;
    // `file:///C:/…` and `file:C:/…` are both local.
    let rest = if rest.starts_with("///") { &rest[3..] } else { rest };
    let b = rest.as_bytes();
    if b.len() < 3 || !b[0].is_ascii_alphabetic() || b[1] != b':' || (b[2] != b'/' && b[2] != b'\\') {
        return None;
    }
    Some(rest.replace('/', "\\"))
}

/// Folders a result must not be in (also checked here, not only in SQL).
pub(crate) fn excluded(path: &str) -> bool {
    path.to_ascii_lowercase()
        .split(['\\', '/'])
        .any(|seg| seg.starts_with('.') || matches!(seg, "appdata" | "node_modules" | "target" | "$recycle.bin"))
}

// ── ranking ─────────────────────────────────────────────────────────────

/// A row from the index (or the walk).
#[derive(Debug, Clone, Default)]
pub(crate) struct Row {
    pub path: String,
    pub name: String,
    pub folder: bool,
    pub modified: Option<i64>,
}

fn hit(row: Row, matched: &'static str) -> FileHit {
    let folder = Path::new(&row.path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let name = if row.name.is_empty() {
        Path::new(&row.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    } else {
        row.name
    };
    FileHit { kind: if row.folder { "folder" } else { "file" }, path: row.path, name, folder, modified: row.modified, matched }
}

/// Name hits before content hits; among names, those starting with the
/// query first, then those containing it, then word matches; the index's own
/// rank order within each group. Duplicates dropped, at most `limit`.
pub(crate) fn rank(q: &str, names: Vec<Row>, contents: Vec<Row>, limit: usize) -> Vec<FileHit> {
    let q = clean_query(q).to_lowercase();
    let mut seen = std::collections::HashSet::new();
    let mut tiered: Vec<(u8, usize, FileHit)> = Vec::new();
    for (i, row) in names.into_iter().enumerate() {
        if !seen.insert(row.path.to_lowercase()) {
            continue;
        }
        let file = Path::new(&row.path).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let tier = if file.starts_with(&q) {
            0
        } else if file.contains(&q) {
            1
        } else {
            2
        };
        tiered.push((tier, i, hit(row, "name")));
    }
    let base = tiered.len();
    for (i, row) in contents.into_iter().enumerate() {
        if !seen.insert(row.path.to_lowercase()) {
            continue;
        }
        tiered.push((3, base + i, hit(row, "contents")));
    }
    tiered.sort_by_key(|(tier, i, _)| (*tier, *i));
    tiered.into_iter().map(|(_, _, h)| h).take(limit).collect()
}

// ── folder walk (no index) ──────────────────────────────────────────────

fn system_ms(t: SystemTime) -> Option<i64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis() as i64)
}

/// Names containing the query under Documents, Desktop, Downloads, Pictures
/// and the profile, breadth first, at most 2 s and 50 results.
pub fn walk(query: &str) -> Result<Vec<Row>, String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let home = dirs::home_dir().ok_or_else(|| "could not resolve home directory".to_string())?;
    let start = Instant::now();
    let mut results: Vec<Row> = Vec::new();

    // Prefer user-facing folders first so the 2s budget isn't burned in deep trees.
    let mut roots: Vec<std::path::PathBuf> = [
        home.join("Documents"),
        home.join("Desktop"),
        home.join("Downloads"),
        home.join("Pictures"),
        home.clone(),
    ]
    .into_iter()
    .filter(|p| p.is_dir())
    .collect();
    roots.dedup();

    // BFS — shallow matches beat deep noise for launcher UX.
    let mut queue: std::collections::VecDeque<(std::path::PathBuf, usize)> =
        roots.into_iter().map(|p| (p, 0)).collect();
    let mut seen = std::collections::HashSet::new();

    while let Some((dir, depth)) = queue.pop_front() {
        if start.elapsed() > WALK_BUDGET || results.len() >= WALK_MAX_RESULTS {
            break;
        }
        if depth > WALK_MAX_DEPTH {
            continue;
        }
        let key = dir.to_string_lossy().to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            if start.elapsed() > WALK_BUDGET || results.len() >= WALK_MAX_RESULTS {
                break;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let lower = name.to_lowercase();
            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            let path = entry.path();
            if lower.contains(&q) {
                results.push(Row {
                    path: path.to_string_lossy().into_owned(),
                    name: name.clone(),
                    folder: file_type.is_dir(),
                    modified: entry.metadata().ok().and_then(|m| m.modified().ok()).and_then(system_ms),
                });
            }
            if file_type.is_dir() && !WALK_SKIP_DIRS.contains(&lower.as_str()) {
                queue.push_back((path, depth + 1));
            }
        }
    }

    results.sort_by_key(|r| r.name.to_lowercase());
    results.truncate(WALK_MAX_RESULTS);
    Ok(results)
}

// ── putting it together ─────────────────────────────────────────────────

/// Index results for `query`, ranked; `Err` when the index is off, failed
/// or took longer than [`INDEX_BUDGET`].
pub fn index_files(query: &str, limit: usize) -> Result<Vec<FileHit>, String> {
    let q = clean_query(query);
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let names = names_sql(&q, limit).ok_or("empty query")?;
    // Contents only from 3 characters on: two letters match half the disk.
    let contents = (q.chars().count() >= 3).then(|| contents_sql(&q, limit)).flatten();
    let (names, contents) = index::run(names, contents, limit, INDEX_BUDGET)?;
    let keep = |rows: Vec<Row>| rows.into_iter().filter(|r| !excluded(&r.path)).collect::<Vec<_>>();
    Ok(rank(&q, keep(names), keep(contents), limit))
}

/// Settings pages and files for the command bar.
pub fn search(query: &str, opts: SearchOptions) -> SearchResults {
    let start = Instant::now();
    let q = clean_query(query);
    let limit = opts.limit.clamp(1, MAX_LIMIT);
    let settings = if opts.settings {
        crate::settings_pages::search(&q, 6, crate::settings_pages::ui_is_german())
    } else {
        Vec::new()
    };
    let (source, files) = if !opts.files || q.is_empty() {
        ("off", Vec::new())
    } else {
        match index_files(&q, limit) {
            Ok(files) => ("index", files),
            Err(e) => {
                tracing::debug!("search: index unavailable ({e})");
                if opts.fallback {
                    let rows = walk(&q).unwrap_or_default();
                    ("walk", rows.into_iter().take(limit).map(|r| hit(r, "name")).collect())
                } else {
                    ("off", Vec::new())
                }
            }
        }
    };
    SearchResults { query: q, source, settings, files, ms: start.elapsed().as_millis() as u64 }
}

/// `cli_search_files`: paths only, index first, the walk as before when the
/// index is not there.
pub fn file_paths(query: &str) -> Result<Vec<String>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    match index_files(query, DEFAULT_LIMIT) {
        Ok(files) => Ok(files.into_iter().map(|f| f.path).collect()),
        Err(e) => {
            tracing::debug!("search: index unavailable ({e}); walking folders");
            Ok(walk(query)?.into_iter().map(|r| r.path).collect())
        }
    }
}

// ── the index ───────────────────────────────────────────────────────────

#[cfg(windows)]
mod index {
    use super::Row;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};
    use windows::core::{Interface, GUID, HSTRING, PWSTR};
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::System::Com::StructuredStorage::{
        PropVariantClear, PropVariantToFileTime, PropVariantToStringAlloc, PROPVARIANT,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Search::{
        CSearchManager, ICommandText, IDBCreateCommand, IDBCreateSession, IDBInitialize, IDataInitialize,
        IGetRow, IRowset, ISearchManager, MSDAINITIALIZE,
    };
    use windows::Win32::System::Variant::PSTF_UTC;
    use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, PSGetPropertyKeyFromName};

    /// `DBGUID_DEFAULT` (oledb.h); not in the `windows` crate.
    const DBGUID_DEFAULT: GUID = GUID::from_u128(0xc8b521fb_5cf3_11ce_ade5_00aa0044773d);
    const BATCH: usize = 32;

    type Answer = Result<(Vec<Row>, Vec<Row>), String>;

    struct Job {
        names: String,
        contents: Option<String>,
        limit: usize,
        reply: Sender<Answer>,
    }

    /// When the running job started (ms since the worker's epoch), 0 = idle.
    static BUSY_SINCE: AtomicU64 = AtomicU64::new(0);

    fn epoch() -> &'static Instant {
        static E: OnceLock<Instant> = OnceLock::new();
        E.get_or_init(Instant::now)
    }

    fn now_ms() -> u64 {
        epoch().elapsed().as_millis() as u64 + 1
    }

    fn worker() -> Result<Sender<Job>, String> {
        static TX: OnceLock<Mutex<Option<Sender<Job>>>> = OnceLock::new();
        let slot = TX.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().unwrap();
        if let Some(tx) = guard.as_ref() {
            return Ok(tx.clone());
        }
        let (tx, rx) = channel::<Job>();
        std::thread::Builder::new()
            .name("versailles-search".into())
            .spawn(move || {
                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                let mut conn: Option<Conn> = None;
                for job in rx {
                    BUSY_SINCE.store(now_ms(), Ordering::SeqCst);
                    let answer = run_job(&mut conn, &job);
                    if answer.is_err() {
                        // Reconnect next time (the service may have restarted).
                        conn = None;
                    }
                    BUSY_SINCE.store(0, Ordering::SeqCst);
                    let _ = job.reply.send(answer);
                }
            })
            .map_err(|e| format!("search worker: {e}"))?;
        *guard = Some(tx.clone());
        Ok(tx)
    }

    /// Ask the worker; wait at most `budget`.
    pub fn run(names: String, contents: Option<String>, limit: usize, budget: Duration) -> Result<(Vec<Row>, Vec<Row>), String> {
        let since = BUSY_SINCE.load(Ordering::SeqCst);
        if since != 0 && now_ms().saturating_sub(since) > budget.as_millis() as u64 {
            return Err("the index is still busy with an earlier query".into());
        }
        let (reply, answer) = channel();
        worker()?
            .send(Job { names, contents, limit, reply })
            .map_err(|_| "search worker stopped".to_string())?;
        answer.recv_timeout(budget).map_err(|_| "the index took too long".to_string())?
    }

    struct Keys {
        url: PROPERTYKEY,
        file_name: PROPERTYKEY,
        display: PROPERTYKEY,
        kind: PROPERTYKEY,
        modified: PROPERTYKEY,
    }

    struct Conn {
        session: IDBCreateCommand,
        keys: Keys,
    }

    fn err(e: windows::core::Error) -> String {
        e.message().to_string().trim().to_string() + &format!(" ({:#010x})", e.code().0)
    }

    unsafe fn key(name: &str) -> Result<PROPERTYKEY, String> {
        let mut k = PROPERTYKEY::default();
        PSGetPropertyKeyFromName(&HSTRING::from(name), &mut k).map_err(err)?;
        Ok(k)
    }

    unsafe fn connect() -> Result<Conn, String> {
        // Fails when the Windows Search service is off.
        let manager: ISearchManager =
            CoCreateInstance(&CSearchManager, None, CLSCTX_LOCAL_SERVER).map_err(|e| format!("Windows Search: {}", err(e)))?;
        let catalog = manager.GetCatalog(&HSTRING::from("SystemIndex")).map_err(err)?;
        let helper = catalog.GetQueryHelper().map_err(err)?;
        let raw: PWSTR = helper.ConnectionString().map_err(err)?;
        let conn_str = raw.to_string().unwrap_or_default();
        CoTaskMemFree(Some(raw.0 as _));
        let init: IDataInitialize = CoCreateInstance(&MSDAINITIALIZE, None, CLSCTX_INPROC_SERVER).map_err(err)?;
        let mut source: Option<windows::core::IUnknown> = None;
        init.GetDataSource(None, CLSCTX_INPROC_SERVER.0, &HSTRING::from(conn_str), &IDBInitialize::IID, &mut source)
            .map_err(err)?;
        let db: IDBInitialize = source.ok_or("no data source")?.cast().map_err(err)?;
        db.Initialize().map_err(err)?;
        let session: IDBCreateCommand = db
            .cast::<IDBCreateSession>()
            .map_err(err)?
            .CreateSession(None, &IDBCreateCommand::IID)
            .map_err(err)?
            .cast()
            .map_err(err)?;
        Ok(Conn {
            session,
            keys: Keys {
                url: key("System.ItemUrl")?,
                file_name: key("System.FileName")?,
                display: key("System.ItemNameDisplay")?,
                kind: key("System.Kind")?,
                modified: key("System.DateModified")?,
            },
        })
    }

    fn run_job(conn: &mut Option<Conn>, job: &Job) -> Answer {
        unsafe {
            if conn.is_none() {
                *conn = Some(connect()?);
            }
            let c = conn.as_ref().unwrap();
            let names = query(c, &job.names)?;
            // Contents only when names leave room.
            let contents = match &job.contents {
                Some(sql) if names.len() < job.limit => query(c, sql)?,
                _ => Vec::new(),
            };
            Ok((names, contents))
        }
    }

    unsafe fn text(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<String> {
        let mut value: PROPVARIANT = store.GetValue(key).ok()?;
        let out = PropVariantToStringAlloc(&value).ok().map(|p| {
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as _));
            s
        });
        let _ = PropVariantClear(&mut value);
        out.filter(|s| !s.is_empty())
    }

    unsafe fn time_ms(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<i64> {
        let mut value: PROPVARIANT = store.GetValue(key).ok()?;
        let ft = PropVariantToFileTime(&value, PSTF_UTC).ok();
        let _ = PropVariantClear(&mut value);
        let ft = ft?;
        let ticks = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
        (ticks > 0).then(|| (ticks / 10_000) as i64 - 11_644_473_600_000)
    }

    unsafe fn query(c: &Conn, sql: &str) -> Result<Vec<Row>, String> {
        let command: ICommandText =
            c.session.CreateCommand(None, &ICommandText::IID).map_err(err)?.cast().map_err(err)?;
        command.SetCommandText(&DBGUID_DEFAULT, &HSTRING::from(sql)).map_err(err)?;
        let mut out: Option<windows::core::IUnknown> = None;
        command.Execute(None, &IRowset::IID, None, None, Some(&mut out)).map_err(err)?;
        let Some(out) = out else {
            return Ok(Vec::new());
        };
        let rowset: IRowset = out.cast().map_err(err)?;
        let get_row: IGetRow = rowset.cast().map_err(err)?;
        let mut rows = Vec::new();
        loop {
            let mut handles = [0usize; BATCH];
            let mut ptr = handles.as_mut_ptr();
            let mut got = 0usize;
            // The projection of GetNextRows takes the handle array awkwardly;
            // call the vtable slot with our own array.
            let hr = (Interface::vtable(&rowset).GetNextRows)(
                Interface::as_raw(&rowset),
                0,
                0,
                BATCH as isize,
                &mut got,
                &mut ptr,
            );
            if hr.is_err() {
                return Err(err(windows::core::Error::from(hr)));
            }
            if got == 0 {
                break;
            }
            for &h in &handles[..got] {
                let Ok(unknown) = get_row.GetRowFromHROW(None, h, &IPropertyStore::IID) else {
                    continue;
                };
                let Ok(store) = unknown.cast::<IPropertyStore>() else {
                    continue;
                };
                let Some(path) = text(&store, &c.keys.url).and_then(|u| super::url_to_path(&u)) else {
                    continue;
                };
                let kind = text(&store, &c.keys.kind).unwrap_or_default().to_ascii_lowercase();
                rows.push(Row {
                    name: text(&store, &c.keys.display)
                        .or_else(|| text(&store, &c.keys.file_name))
                        .unwrap_or_default(),
                    folder: kind.split(';').any(|k| k.trim() == "folder"),
                    modified: time_ms(&store, &c.keys.modified),
                    path,
                });
            }
            let _ = rowset.ReleaseRows(got, handles.as_ptr(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut());
            if got < BATCH {
                break;
            }
        }
        Ok(rows)
    }
}

#[cfg(not(windows))]
mod index {
    use super::Row;
    use std::time::Duration;
    pub fn run(_: String, _: Option<String>, _: usize, _: Duration) -> Result<(Vec<Row>, Vec<Row>), String> {
        Err("the Windows Search index is only available on Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_strings_double_quotes() {
        assert_eq!(sql_string("it's"), "'it''s'");
        assert_eq!(sql_string("'; DROP TABLE x; --"), "'''; DROP TABLE x; --'");
        assert_eq!(sql_string(""), "''");
    }

    #[test]
    fn like_wildcards_are_literal() {
        assert_eq!(like_escape("50%_off[1]"), "50[%][_]off[[]1]");
        assert_eq!(like_escape("plain name"), "plain name");
    }

    #[test]
    fn contains_terms_are_quoted_prefixes() {
        assert_eq!(contains_terms("rechnung").as_deref(), Some("\"rechnung*\""));
        assert_eq!(contains_terms("  rechnung   2024 ").as_deref(), Some("\"rechnung*\" AND \"2024*\""));
        // Quotes, stars and other punctuation split words; terms are letters and digits.
        assert_eq!(contains_terms("say \"hi\" *now*").as_deref(), Some("\"say*\" AND \"hi*\" AND \"now*\""));
        assert_eq!(contains_terms("o'brien report_2024.pdf").as_deref(), Some("\"brien*\" AND \"report*\" AND \"2024*\" AND \"pdf*\""));
        assert_eq!(contains_terms("Größe").as_deref(), Some("\"Größe*\""));
        // One-letter pieces would match nearly everything.
        assert_eq!(contains_terms("a_b"), None);
        assert_eq!(contains_terms("\"\" * \u{7}"), None);
        assert_eq!(contains_terms(""), None);
        let many = contains_terms("aa bb cc dd ee ff gg hh").unwrap();
        assert_eq!(many.matches(" AND ").count(), MAX_TERMS - 1);
    }

    #[test]
    fn user_text_cannot_leave_its_literal() {
        let evil = "x' OR 1=1 OR System.FileName LIKE '%";
        let sql = names_sql(evil, 40).unwrap();
        // Every quote the user typed is doubled, so the WHERE clause has
        // exactly the quotes we put there plus the doubled ones.
        assert!(sql.contains("LIKE 'x'' OR 1=1 OR System.FileName LIKE ''[%]%'"), "{sql}");
        assert!(sql.contains("CONTAINS(System.FileName, '\"OR*\" AND \"OR*\" AND \"System*\""), "{sql}");
        let quotes = sql.matches('\'').count();
        assert_eq!(quotes % 2, 0, "{sql}");
        let sql = contents_sql("o'brien", 10).unwrap();
        assert!(sql.contains("CONTAINS(*, '\"brien*\"')"), "{sql}");
        assert!(sql.starts_with("SELECT TOP 10 System.ItemUrl"), "{sql}");
        assert!(names_sql("  ", 10).is_none());
        // Too short for words: the name prefix alone.
        let sql = names_sql("a_b", 10).unwrap();
        assert!(sql.contains("AND (System.FileName LIKE 'a[_]b%')"), "{sql}");
        assert!(contents_sql("a_b", 10).is_none());
        // Control characters never reach the SQL.
        assert!(!names_sql("a\r\nb\0c", 10).unwrap().contains(['\r', '\n', '\0']));
    }

    #[test]
    fn item_urls_become_local_paths() {
        assert_eq!(url_to_path("file:C:/Users/me/a b.txt").as_deref(), Some(r"C:\Users\me\a b.txt"));
        assert_eq!(url_to_path("FILE:D:/x").as_deref(), Some(r"D:\x"));
        assert_eq!(url_to_path("file:///C:/Users/me").as_deref(), Some(r"C:\Users\me"));
        assert_eq!(url_to_path("file://server/share/x"), None);
        assert_eq!(url_to_path("file:\\\\server\\share"), None);
        assert_eq!(url_to_path("mapi://x"), None);
        assert_eq!(url_to_path("file:"), None);
    }

    #[test]
    fn excludes_dot_and_app_folders() {
        assert!(excluded(r"C:\Users\me\.cargo\registry\x.rs"));
        assert!(excluded(r"C:\Users\me\AppData\Local\x"));
        assert!(excluded(r"C:\p\node_modules\a\index.js"));
        assert!(excluded(r"C:\p\src-tauri\target\debug\x.exe"));
        assert!(!excluded(r"C:\Users\me\Documents\Rechnung.pdf"));
        assert!(!excluded(r"C:\Users\me\Documents\v1.2\notes.txt"));
    }

    fn row(path: &str) -> Row {
        Row { path: path.into(), ..Default::default() }
    }

    #[test]
    fn ranks_name_prefix_then_substring_then_other_names_then_contents() {
        let names = vec![
            row(r"C:\d\Berichte.pdf"),      // the index matched it some other way
            row(r"C:\d\my report.pdf"),     // contains
            row(r"C:\d\Q3-report.xlsx"),    // contains
            row(r"C:\d\Report 2024.docx"),  // prefix
            row(r"C:\d\report-final.docx"), // prefix
        ];
        let contents = vec![row(r"C:\d\notes.txt"), row(r"C:\d\Report 2024.docx")];
        let out = rank("report", names, contents, 40);
        let paths: Vec<&str> = out.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                r"C:\d\Report 2024.docx",
                r"C:\d\report-final.docx",
                r"C:\d\my report.pdf",
                r"C:\d\Q3-report.xlsx",
                r"C:\d\Berichte.pdf",
                r"C:\d\notes.txt",
            ]
        );
        assert_eq!(out[0].matched, "name");
        assert_eq!(out[5].matched, "contents");
        assert_eq!(out[0].name, "Report 2024.docx");
        assert_eq!(out[0].folder, r"C:\d");
        assert_eq!(out[0].kind, "file");
        assert_eq!(rank("report", vec![row(r"C:\a\report.txt")], vec![row(r"C:\b\report.txt")], 1).len(), 1);
    }

    #[test]
    fn search_without_files_is_settings_only() {
        let r = search("bluetooth", SearchOptions { files: false, settings: true, fallback: false, limit: 5 });
        assert_eq!(r.source, "off");
        assert!(r.files.is_empty());
        assert_eq!(r.settings[0].uri, "ms-settings:bluetooth");
    }

    /// Read-only: queries the real index and lists the AppsFolder.
    /// `cargo test --profile release-test --lib live_index -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_index_and_apps_folder() {
        for q in ["ver", "rechnung", "report", "a_b", "50%", "o'brien", "readme"] {
            let t = Instant::now();
            let r = index_files(q, 40);
            let ms = t.elapsed().as_millis();
            match r {
                Ok(files) => {
                    println!("index {q:?}: {} hits in {ms} ms", files.len());
                    for f in files.iter().take(5) {
                        println!("  [{}:{}] {} | {}", f.kind, f.matched, f.name, f.path);
                    }
                }
                Err(e) => println!("index {q:?}: error after {ms} ms: {e}"),
            }
        }
        let t = Instant::now();
        let walk = file_paths("readme");
        println!("cli_search_files(readme): {:?} items in {} ms", walk.map(|v| v.len()), t.elapsed().as_millis());
        let t = Instant::now();
        let (all, packaged) = crate::apps::apps_folder_counts();
        println!("AppsFolder: {all} items, {packaged} packaged, in {} ms", t.elapsed().as_millis());
        let t = Instant::now();
        let recent = crate::recent::recent_files(30);
        println!("recent: {:?} in {} ms", recent.as_ref().map(|r| r.len()), t.elapsed().as_millis());
        if let Ok(r) = recent {
            for f in r.iter().take(5) {
                println!("  [{}] {} | {}", f.kind, f.name, f.path);
            }
        }
    }
}
