use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::OnceLock;
use tauri::{Emitter, Manager};
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

use crate::{collab_toml_path, gui_config_path, legacy_gui_config_path, AppState, SavedConfig};

/// User's interactive-shell PATH, cached at first use.
///
/// macOS GUI apps launched from Finder / `open` inherit launchd's minimal
/// PATH (`/usr/bin:/bin:/usr/sbin:/sbin`) — they do NOT read `.zshrc` /
/// `.bash_profile` / `.zshenv`. Sidecars we spawn inherit that minimal
/// PATH, and so do the worker daemons they fork, which means a worker
/// trying to run `claude` fails with ENOENT because the real `claude`
/// binary lives in `~/.local/bin` / `~/.cargo/bin` / `/opt/homebrew/bin`
/// / etc. that the GUI has never heard of.
///
/// Fix: once, lazily, we shell out to the user's login shell in
/// interactive mode (`$SHELL -lic 'printf %s "$PATH"'`) to resolve the
/// PATH they actually see in a terminal, then inject it as `PATH` on
/// every sidecar spawn so workers can find their CLI tools.
///
/// Falls back to the inherited PATH if anything goes wrong — no worse
/// than the pre-fix behaviour.
fn resolve_user_path() -> String {
    static CACHED: OnceLock<String> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
            // `-l` login shell → reads zprofile/profile. `-i` interactive →
            // reads zshrc/bashrc. Together they match what a fresh terminal
            // sees. `printf %s` avoids a trailing newline.
            let output = std::process::Command::new(&shell)
                .args(["-lic", "printf %s \"$PATH\""])
                .output();
            let resolved = match output {
                Ok(o) if o.status.success() => {
                    String::from_utf8_lossy(&o.stdout).trim().to_string()
                }
                _ => String::new(),
            };
            let fallback = std::env::var("PATH").unwrap_or_default();
            if resolved.is_empty() {
                eprintln!(
                    "[path] resolving user shell PATH failed — falling back to inherited PATH"
                );
                fallback
            } else if resolved == fallback {
                // Already matches — no-op for people launching from a terminal.
                resolved
            } else {
                eprintln!("[path] resolved user shell PATH: {resolved}");
                resolved
            }
        })
        .clone()
}

// ─── Config commands ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn load_config() -> SavedConfig {
    // Prefer the canonical platform-native location. If nothing is there,
    // fall back to the old hard-coded `$HOME/.config/...` path so users
    // who set up before the fix don't lose their wizard state. The first
    // subsequent save_config call rewrites at the canonical path.
    let from = |p: PathBuf| -> Option<SavedConfig> {
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
    };
    gui_config_path()
        .and_then(from)
        .or_else(|| legacy_gui_config_path().and_then(from))
        .unwrap_or_default()
}

#[tauri::command]
pub fn save_config(config: SavedConfig) -> Result<(), String> {
    if let Some(path) = gui_config_path() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| e.to_string())?;
    }

    // Also put the server and token in ~/.collab.toml so the collab CLI picks
    // them up. Only those two settings are touched: `instance`, `recipients`
    // and anything else already in the file stay as they are.
    //
    // The admin token goes in as a comment so users can see it exists, but
    // the CLI reads admin tokens from COLLAB_ADMIN_TOKEN env only (we don't
    // want two admin secrets auto-loading if someone has a legacy token in
    // ~/.collab.toml).
    if let Some(toml_path) = collab_toml_path() {
        let existing = match std::fs::read_to_string(&toml_path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            // Unreadable is not the same as empty: writing now would lose it.
            Err(e) => return Err(format!("could not read {}: {e}", toml_path.display())),
        };
        let toml = crate::join_text::merge_app_config(
            &existing,
            &config.server_url,
            &config.token,
            &config.admin_token,
        );
        std::fs::write(&toml_path, toml).map_err(|e| e.to_string())?;
        // The file holds the team token: keep it to this user.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&toml_path, std::fs::Permissions::from_mode(0o600));
        }
    }

    Ok(())
}

#[tauri::command]
pub fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| format!("{:02x}", rng.gen::<u8>()))
        .collect()
}

// ─── File system commands ─────────────────────────────────────────────────────

#[cfg(not(any(target_os = "ios", target_os = "android")))]
#[tauri::command]
pub async fn pick_directory() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("Choose project directory")
        .pick_folder()
        .await
        .map(|h| h.path().to_string_lossy().to_string())
}

#[tauri::command]
pub fn write_file(path: String, content: String) -> Result<(), String> {
    let p = Path::new(&path);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(p, content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn path_exists(path: String) -> bool {
    Path::new(&path).exists()
}

#[tauri::command]
pub fn home_dir() -> Option<String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
}

// ─── Network addresses ────────────────────────────────────────────────────────

/// Keep the addresses another machine could type to reach this one, in the
/// order found, without repeats. Loopback and unspecified addresses only
/// mean "this machine" and are dropped.
pub fn reachable_addresses(candidates: impl IntoIterator<Item = std::net::IpAddr>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for ip in candidates {
        if ip.is_loopback() || ip.is_unspecified() {
            continue;
        }
        let text = ip.to_string();
        if !out.contains(&text) {
            out.push(text);
        }
    }
    out
}

/// The address this machine would use to reach `target`, if it has a route.
/// Connecting a UDP socket only picks a route; it sends nothing.
fn source_address_towards(target: &str) -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect(target).ok()?;
    socket.local_addr().ok().map(|a| a.ip())
}

/// Addresses to show when adding a machine to a team hosted here.
///
/// Two probes: an address on the public internet side (192.0.2.1 is
/// reserved for documentation, so nothing is ever contacted) finds the
/// home or office network address, and Tailscale's own service address
/// finds the tailnet address when Tailscale is running. Without Tailscale
/// both give the same answer and it is listed once.
#[tauri::command]
pub fn local_addresses() -> Vec<String> {
    reachable_addresses(
        ["192.0.2.1:9", "100.100.100.100:9"]
            .into_iter()
            .filter_map(source_address_towards),
    )
}

#[cfg(test)]
mod address_tests {
    use super::reachable_addresses;
    use std::net::IpAddr;

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter().map(|s| s.parse().unwrap()).collect()
    }

    #[test]
    fn keeps_network_addresses_in_order_without_repeats() {
        let got = reachable_addresses(ips(&["192.168.1.20", "100.64.0.5", "192.168.1.20"]));
        assert_eq!(got, vec!["192.168.1.20", "100.64.0.5"]);
    }

    #[test]
    fn drops_addresses_that_only_mean_this_machine() {
        let got = reachable_addresses(ips(&["127.0.0.1", "0.0.0.0", "::1", "::", "10.0.0.7"]));
        assert_eq!(got, vec!["10.0.0.7"]);
    }

    #[test]
    fn nothing_usable_is_an_empty_list() {
        assert!(reachable_addresses(ips(&["127.0.0.1"])).is_empty());
    }

    /// Whatever this machine's network looks like, the command must not
    /// panic and must never offer loopback.
    #[test]
    fn the_live_probe_never_offers_loopback() {
        for a in super::local_addresses() {
            let ip: IpAddr = a.parse().unwrap();
            assert!(!ip.is_loopback() && !ip.is_unspecified(), "{a}");
        }
    }
}

// ─── Server lifecycle ─────────────────────────────────────────────────────────

/// Spawn the bundled `collab-server` sidecar.
///
/// `admin_token` is the admin secret the GUI generated for this session —
/// passed as `COLLAB_ADMIN_TOKEN` (highest priority in the server's token
/// lookup) so it beats any stale `token = …` in `~/.collab.toml`. Workers
/// later authenticate with the team token via their own `COLLAB_TOKEN`
/// env; the two are deliberately distinct.
#[tauri::command]
pub async fn start_server(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    server_url: String,
    admin_token: String,
    project_dir: String,
) -> Result<(), String> {
    let port: u16 = server_url
        .trim_end_matches('/')
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8000);

    {
        let mut guard = state.server_process.lock().await;
        if let Some(child) = guard.take() {
            let _ = child.kill();
        }
    }

    let cwd = if project_dir.is_empty() {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        PathBuf::from(&project_dir)
    };

    // Remember where we're running so the window-close handler knows where
    // to run `collab stop all` on shutdown. See also `mark_session_active`,
    // which is called from the launch flow when we probe an existing server
    // and skip the local spawn — the dir still needs to be remembered so
    // Cmd+Q still warns the user about local worker processes.
    {
        let mut dir_guard = state.current_project_dir.lock().await;
        *dir_guard = Some(cwd.clone());
    }

    let sidecar = app
        .shell()
        .sidecar("collab-server")
        .map_err(|e| format!("Could not locate collab-server sidecar: {e}"))?
        // COLLAB_ADMIN_TOKEN is the authoritative admin slot; server lookup
        // priority is COLLAB_ADMIN_TOKEN > COLLAB_TOKEN > ~/.collab.toml, so
        // setting it here wins even if the user's shell has a stale
        // COLLAB_TOKEN or their ~/.collab.toml points at a different team.
        .env("COLLAB_ADMIN_TOKEN", &admin_token)
        .env("COLLAB_HOST", "0.0.0.0")
        // See `resolve_user_path` — without this, worker daemons spawned
        // downstream inherit launchd's minimal PATH and can't find `claude`.
        .env("PATH", resolve_user_path())
        .args(["--port", &port.to_string()])
        .current_dir(cwd);

    let (mut rx, child) = sidecar
        .spawn()
        .map_err(|e| format!("Could not start collab-server sidecar: {e}"))?;

    state.server_alive.store(true, Ordering::SeqCst);
    let alive = state.server_alive.clone();
    let app2 = app.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(bytes) | CommandEvent::Stderr(bytes) => {
                    let line = String::from_utf8_lossy(&bytes).to_string();
                    let _ = app2.emit("server-log", &line);
                }
                CommandEvent::Terminated(_) => {
                    alive.store(false, Ordering::SeqCst);
                    break;
                }
                _ => {}
            }
        }
    });

    let mut guard = state.server_process.lock().await;
    *guard = Some(child);
    Ok(())
}

/// Register an active session without spawning a local collab-server.
///
/// Called from the GUI launch flow when we probe an existing server at
/// `cfg.serverUrl` and skip the local spawn (e.g. a mac joiner connecting
/// to a Windows host). The project dir still needs to be remembered so
/// `handle_quit_attempt` can show the "stop workers before quitting?"
/// warning and `shutdown_session` can run `collab stop all` on the
/// local worker daemons this GUI manages.
#[tauri::command]
pub async fn mark_session_active(
    state: tauri::State<'_, AppState>,
    project_dir: String,
) -> Result<(), String> {
    let cwd = if project_dir.is_empty() {
        std::env::current_dir().map_err(|e| e.to_string())?
    } else {
        PathBuf::from(&project_dir)
    };
    let mut guard = state.current_project_dir.lock().await;
    *guard = Some(cwd);
    Ok(())
}

#[tauri::command]
pub async fn stop_server(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.server_process.lock().await;
    if let Some(child) = guard.take() {
        child.kill().map_err(|e| e.to_string())?;
    }
    state.server_alive.store(false, Ordering::SeqCst);
    Ok(())
}

/// Best-effort cleanup called from the window-close handler.
///
/// Steps:
///   1. If we know the project dir, run `collab stop all` via sidecar so
///      every worker daemon tracked in the lifecycle manifest receives
///      SIGTERM. This is the same command a user would type to clean up.
///   2. Kill the embedded `collab-server` child we spawned.
///
/// Both steps are best-effort — a failure in step 1 must not block step 2,
/// and a failure in step 2 must not block process exit.
pub async fn shutdown_session(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();

    // Step 1: stop worker daemons via the CLI.
    let project_dir = {
        let guard = state.current_project_dir.lock().await;
        guard.clone()
    };
    if let Some(dir) = project_dir {
        if let Ok(cmd) = app.shell().sidecar("collab") {
            let spawn_result = cmd.args(["stop", "all"]).current_dir(dir).spawn();
            if let Ok((mut rx, _child)) = spawn_result {
                // Wait up to 5s for Terminated so we don't hang exit forever.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                loop {
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match tokio::time::timeout(remaining, rx.recv()).await {
                        Ok(Some(CommandEvent::Terminated(_))) => break,
                        Ok(Some(_)) => continue,
                        Ok(None) => break,
                        Err(_) => break, // timeout
                    }
                }
            }
        }
    }

    // Step 2: kill the embedded server.
    let mut guard = state.server_process.lock().await;
    if let Some(child) = guard.take() {
        let _ = child.kill();
    }
    state.server_alive.store(false, Ordering::SeqCst);
}

#[tauri::command]
pub async fn server_running(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    if !state.server_alive.load(Ordering::SeqCst) {
        let mut guard = state.server_process.lock().await;
        *guard = None;
        return Ok(false);
    }
    Ok(true)
}

// ─── Generic command runner ───────────────────────────────────────────────────

#[tauri::command]
pub async fn run_command(
    app: tauri::AppHandle,
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
    envs: Vec<(String, String)>,
) -> Result<i32, String> {
    // Route bundled binaries through the sidecar API; everything else is rejected
    // (the GUI should only ever invoke `collab` / `collab-server` from here).
    let sidecar_name = match program.as_str() {
        "collab" | "collab-server" => program.as_str(),
        other => return Err(format!("run_command: `{other}` is not an allowed sidecar")),
    };

    let mut cmd = app
        .shell()
        .sidecar(sidecar_name)
        .map_err(|e| format!("Could not locate `{sidecar_name}` sidecar: {e}"))?
        .args(&args);
    if let Some(dir) = &cwd {
        cmd = cmd.current_dir(PathBuf::from(dir));
    }
    // Inject the user's interactive shell PATH so worker daemons spawned
    // by `collab start all` can find `claude` / `cursor` / `codex` / etc.
    // Caller-supplied `envs` win if they include their own PATH.
    let caller_overrides_path = envs.iter().any(|(k, _)| k == "PATH");
    if !caller_overrides_path {
        cmd = cmd.env("PATH", resolve_user_path());
    }
    for (k, v) in &envs {
        cmd = cmd.env(k, v);
    }

    let (mut rx, _child) = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn `{sidecar_name}`: {e}"))?;

    let mut exit_code: i32 = -1;
    while let Some(event) = rx.recv().await {
        match event {
            CommandEvent::Stdout(bytes) => {
                let line = String::from_utf8_lossy(&bytes).to_string();
                let _ = app.emit(
                    "cmd-output",
                    serde_json::json!({ "stream": "out", "line": line }),
                );
            }
            CommandEvent::Stderr(bytes) => {
                let line = String::from_utf8_lossy(&bytes).to_string();
                let _ = app.emit(
                    "cmd-output",
                    serde_json::json!({ "stream": "err", "line": line }),
                );
            }
            CommandEvent::Terminated(payload) => {
                exit_code = payload.code.unwrap_or(-1);
                break;
            }
            _ => {}
        }
    }
    Ok(exit_code)
}
