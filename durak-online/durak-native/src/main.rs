//! Durak AI with the engine running natively.
//!
//! Serves the same web app the browser version uses, on this computer only,
//! and opens it in the default browser. The app notices it is being served
//! from here and sends its searches to this program instead of running the
//! slower WebAssembly engine inside the browser. Close the window to stop it.
//!
//! `durak-ai [--port N] [--no-browser]`

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use durak_core::{solve_exact, GameState, Search, SolveParams};
use serde::Deserialize;
use tiny_http::{Header, Method, Request, Response, Server};

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

const FIRST_PORT: u16 = 8787;
const DEFAULT_ROLLOUTS: u32 = 1000;
const DEFAULT_MAX_DEPTH: u32 = 500;
/// A kept search is dropped when it has not been asked about for this long...
const KEEP_SEARCHES: Duration = Duration::from_secs(120);
/// ...and no more than this many are kept at once.
const MAX_KEPT_SEARCHES: usize = 64;
/// The browser engine stops at 6 million positions for memory; natively there is room for more.
const MAX_EXACT_NODES: u64 = 12_000_000;
/// The app sizes exact searches for the browser engine's speed, which the
/// native engine matches (measured: within a few percent).
const EXACT_SPEEDUP: u64 = 1;

/// The same request the browser engine takes (see `durak-wasm`).
#[derive(Deserialize)]
struct SolveRequest {
    state: GameState,
    #[serde(default)]
    determinizations: Option<u32>,
    #[serde(default)]
    rollouts_per_determinization: Option<u32>,
    #[serde(default)]
    max_depth: Option<u32>,
    #[serde(default)]
    seed: Option<u64>,
    /// Keep the search under this id and go on with it when asked again
    /// about the same position.
    #[serde(default)]
    search: Option<String>,
}

#[derive(Deserialize)]
struct ExactRequest {
    state: GameState,
    #[serde(default)]
    max_nodes: Option<u64>,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let open_browser = !args.iter().any(|a| a == "--no-browser");
    let port: Option<u16> = args.iter().position(|a| a == "--port").and_then(|i| args.get(i + 1)?.parse().ok());

    // Only this computer can reach it.
    let ports: Vec<u16> = match port {
        Some(p) => vec![p],
        None => (FIRST_PORT..FIRST_PORT + 20).collect(),
    };
    let Some((server, port)) = ports.iter().find_map(|p| Server::http(("127.0.0.1", *p)).ok().map(|s| (s, *p))) else {
        eprintln!("Could not open a port for Durak AI (tried {ports:?}). Is it already running?");
        wait_for_enter();
        return;
    };
    let url = format!("http://127.0.0.1:{port}/");
    println!("Durak AI is running at {url}");
    println!("The engine runs natively on this computer. Close this window to stop it.");
    if ASSETS.is_empty() {
        println!("Note: this copy was built without the web app (run `npm run build` in web/ before building it).");
    }
    if open_browser {
        open(&url);
    }

    // Each search runs on one thread; the app sends one per core at once, plus an exact search.
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let server = Arc::new(server);
    let handlers: Vec<_> = (0..cores * 2 + 2)
        .map(|_| {
            let server = Arc::clone(&server);
            let origin = format!("http://127.0.0.1:{port}");
            std::thread::spawn(move || {
                while let Ok(req) = server.recv() {
                    handle(req, &origin, cores);
                }
            })
        })
        .collect();
    for h in handlers {
        let _ = h.join();
    }
}

fn handle(mut req: Request, origin: &str, cores: usize) {
    let path = req.url().split('?').next().unwrap_or("/").to_string();
    let method = req.method().clone();

    if path.starts_with("/api/") {
        // Searches are for this app only, not for other sites open in the browser.
        let foreign = req
            .headers()
            .iter()
            .any(|h| h.field.equiv("Origin") && h.value.as_str() != origin);
        if foreign {
            let _ = req.respond(Response::from_string("forbidden").with_status_code(403));
            return;
        }
        let mut body = String::new();
        if method == Method::Post && req.as_reader().read_to_string(&mut body).is_err() {
            let _ = req.respond(Response::from_string("unreadable request").with_status_code(400));
            return;
        }
        let reply = match (method, path.as_str()) {
            (Method::Get, "/api/info") => Ok(serde_json::json!({
                "engine": "native",
                "version": env!("CARGO_PKG_VERSION"),
                "cores": cores,
            })
            .to_string()),
            (Method::Post, "/api/solve") => run_solve(&body),
            (Method::Post, "/api/exact") => run_exact(&body),
            _ => Err((404, "no such endpoint".to_string())),
        };
        let _ = match reply {
            Ok(json) => req.respond(Response::from_string(json).with_header(header("Content-Type", "application/json"))),
            Err((code, msg)) => req.respond(Response::from_string(msg).with_status_code(code)),
        };
        return;
    }

    let file = if path == "/" { "index.html" } else { path.trim_start_matches('/') };
    match ASSETS.iter().find(|(p, _)| *p == file) {
        Some((p, bytes)) => {
            let _ = req.respond(Response::from_data(*bytes).with_header(header("Content-Type", mime(p))));
        }
        None if ASSETS.is_empty() => {
            let _ = req.respond(
                Response::from_string("This copy of Durak AI was built without the web app.")
                    .with_status_code(500),
            );
        }
        None => {
            let _ = req.respond(Response::from_string("not found").with_status_code(404));
        }
    }
}

/// Searches kept between requests, by the id the app's worker sent: the
/// search, its depth limit and when it was last used.
type Kept = HashMap<String, (Search, u32, Instant)>;

fn kept() -> MutexGuard<'static, Kept> {
    static KEPT: OnceLock<Mutex<Kept>> = OnceLock::new();
    // A search that panicked must not take the others down with it.
    KEPT.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

fn run_solve(body: &str) -> Result<String, (u16, String)> {
    let r: SolveRequest = serde_json::from_str(body).map_err(|e| (400, format!("could not read the solve request: {e}")))?;
    let max_depth = r.max_depth.unwrap_or(DEFAULT_MAX_DEPTH);

    // Asked again about the same position: go on with the search already
    // made for it (as the browser engine does, see durak-wasm).
    let held = r.search.as_ref().and_then(|id| kept().remove(id));
    let mut search = match held {
        Some((search, depth, _)) if depth == max_depth && *search.state() == r.state => search,
        _ => {
            let params = SolveParams { max_depth, ..SolveParams::app() };
            Search::new(&r.state, params, r.seed.unwrap_or_else(seed)).map_err(|e| (422, e.to_string()))?
        }
    };
    let result = search
        .run(r.determinizations.unwrap_or(1), r.rollouts_per_determinization.unwrap_or(DEFAULT_ROLLOUTS))
        .map_err(|e| (422, e.to_string()))?;

    if let Some(id) = r.search {
        let mut kept = kept();
        kept.retain(|_, (_, _, used)| used.elapsed() < KEEP_SEARCHES);
        if kept.len() < MAX_KEPT_SEARCHES {
            kept.insert(id, (search, max_depth, Instant::now()));
        }
    }
    serde_json::to_string(&result).map_err(|e| (500, e.to_string()))
}

fn run_exact(body: &str) -> Result<String, (u16, String)> {
    let r: ExactRequest = serde_json::from_str(body).map_err(|e| (400, format!("could not read the exact-solve request: {e}")))?;
    r.state.validate().map_err(|e| (422, e.to_string()))?;
    let nodes = (r.max_nodes.unwrap_or(2_000_000) * EXACT_SPEEDUP).min(MAX_EXACT_NODES);
    let result = solve_exact(&r.state, nodes).map_err(|e| (422, e.to_string()))?;
    serde_json::to_string(&result).map_err(|e| (500, e.to_string()))
}

/// A different seed for every search.
fn seed() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    let mut x = t ^ COUNTER.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid header")
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn open(url: &str) {
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if r.is_err() {
        println!("Open {url} in your browser.");
    }
}

fn wait_for_enter() {
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
}
