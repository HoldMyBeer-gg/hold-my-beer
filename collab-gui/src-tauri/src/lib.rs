pub mod commands;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_shell::process::CommandChild;
use tokio::sync::Mutex;

// ─── State ────────────────────────────────────────────────────────────────────

pub struct AppState {
    pub server_process: Mutex<Option<CommandChild>>,
    pub server_alive: Arc<AtomicBool>,
    /// Last project directory passed to `start_server`. Used by the
    /// window-close handler to run `collab stop all` in the right place so
    /// worker daemons don't leak after the user closes the GUI.
    pub current_project_dir: Mutex<Option<PathBuf>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            server_process: Mutex::new(None),
            server_alive: Arc::new(AtomicBool::new(false)),
            current_project_dir: Mutex::new(None),
        }
    }
}

// ─── Config ───────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SavedConfig {
    // JS sends camelCase (`serverUrl`, `projectDir`, `setupComplete`, …)
    // and Tauri only auto-converts top-level invoke args, not nested struct
    // fields. Without `rename_all = "camelCase"` here, serde silently dropped
    // every field whose JS name didn't match the Rust field name and reset it
    // to its default on every save — which is why the config file stayed stuck
    // on its defaults no matter what the wizard did.
    #[serde(default)]
    pub token: String,
    /// Admin secret (`adm_…` or legacy). Optional — only needed for admin
    /// operations like `collab team create`. Workers auth with `token`
    /// (the team token) instead.
    #[serde(default)]
    pub admin_token: String,
    /// Name of the team this wizard is set up for. Written into team.yml's
    /// top-level `team:` key; also used as a label in the wizard.
    #[serde(default)]
    pub team_name: String,
    #[serde(default = "default_server_url")]
    pub server_url: String,
    #[serde(default)]
    pub identity: String,
    #[serde(default)]
    pub project_dir: String,
    #[serde(default)]
    pub setup_complete: bool,
    #[serde(default)]
    pub cli_template: String,
    #[serde(default)]
    pub model: String,
}

fn default_server_url() -> String {
    "http://localhost:8000".into()
}

impl Default for SavedConfig {
    fn default() -> Self {
        Self {
            token: String::new(),
            admin_token: String::new(),
            team_name: String::new(),
            server_url: default_server_url(),
            identity: String::new(),
            project_dir: String::new(),
            setup_complete: false,
            cli_template: "claude -p {prompt} --model {model} --allowedTools Bash,Read,Write,Edit"
                .into(),
            model: "haiku".into(),
        }
    }
}

/// Canonical per-platform location for the wizard's saved config.
/// Linux:   `$XDG_CONFIG_HOME` or `~/.config/hold-my-beer-gui/config.json`
/// macOS:   `~/Library/Application Support/hold-my-beer-gui/config.json`
/// Windows: `%APPDATA%\hold-my-beer-gui\config.json`
pub fn gui_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|base| base.join("hold-my-beer-gui").join("config.json"))
}

/// Pre-1.0 location — a hard-coded `$HOME/.config/hold-my-beer-gui/...` path
/// that was wrong on macOS (should be `Library/Application Support`) and
/// Windows (should be `%APPDATA%`). Kept around so `load_config` can read
/// it once at startup for users who set up under the old path, then
/// transparently migrate them to the canonical location on next save.
pub fn legacy_gui_config_path() -> Option<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("hold-my-beer-gui")
            .join("config.json"),
    )
}

pub fn collab_toml_path() -> Option<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    Some(PathBuf::from(home).join(".collab.toml"))
}

// ─── Entry point ─────────────────────────────────────────────────────────────

/// Returns `true` if the caller should proceed with closing/exiting, `false`
/// if the user asked to stay open. A `true` return means the caller has
/// already kicked off `shutdown_session` on an async task and will then call
/// `app.exit(0)` — or, for the no-session case, nothing further is needed.
///
/// All three quit paths route through this helper so the prompt and cleanup
/// behaviour are identical:
///   * `WindowEvent::CloseRequested` — Cmd+W, red dot, OS-native close
///   * `RunEvent::ExitRequested`     — programmatic `app.exit()` (after our
///     own async cleanup completes)
///   * `applicationShouldTerminate:` — Cmd+Q on macOS. tao's app delegate
///     does not implement this selector (verified in tao 0.34.6
///     `src/platform_impl/macos/app_delegate.rs`), so Cocoa falls back to
///     `NSTerminateNow` and the process dies before any tao/wry event ever
///     fires. We add the selector to tao's delegate class via the obj-c
///     runtime in `install_macos_quit_intercept` below.
fn handle_quit_attempt(app: &tauri::AppHandle, origin: &str) -> bool {
    use tauri::Manager;
    eprintln!("[shutdown] quit attempt from {origin}");
    let state = app.state::<AppState>();

    let has_session = match state.current_project_dir.try_lock() {
        Ok(guard) => guard.is_some(),
        // Lock contended → some other code path is touching the session right
        // now, which means we definitely have an active session worth warning
        // about. Default to "yes, prompt" rather than silently quitting.
        Err(_) => true,
    };
    if !has_session {
        return true;
    }

    let choice = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title("Stop workers before quitting?")
        .set_description(
            "`collab worker` processes are still running and will keep \
             spending tokens until they're stopped.\n\n\
             Yes: stop all workers and quit.\n\
             No: cancel and keep the GUI open."
        )
        .set_buttons(rfd::MessageButtons::YesNo)
        .show();
    eprintln!("[shutdown] dialog returned: {choice:?}");

    if matches!(choice, rfd::MessageDialogResult::Yes) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            eprintln!("[shutdown] running shutdown_session");
            commands::shutdown_session(&app).await;
            eprintln!("[shutdown] shutdown_session done — calling app.exit");
            app.exit(0);
        });
        // Tell the caller to block the immediate close; our async task will
        // fire `app.exit(0)` once cleanup completes.
        false
    } else {
        eprintln!("[shutdown] user cancelled quit");
        false
    }
}

/// Install our own `applicationShouldTerminate:` method on tao's existing
/// macOS app delegate class. Tao does not implement that selector, so without
/// this Cocoa proceeds straight to `NSTerminateNow` on Cmd+Q and the process
/// dies before Tauri ever sees an event — which is why our `RunEvent::Exit`
/// handler never fired and workers were orphaning. We add the method via the
/// obj-c runtime; the call takes effect immediately on the existing delegate
/// instance because obj-c dispatches methods on the class, not the instance.
///
/// Returning `NSTerminateCancel` from this method tells AppKit to abort the
/// terminate sequence. Our handler does that for both the "stay open" and
/// "kicking off async cleanup" cases — the cleanup task calls `app.exit(0)`
/// once it's done, which routes through `RunEvent::ExitRequested` (with
/// `code = Some(0)`) and is allowed through.
#[cfg(target_os = "macos")]
mod macos_quit_intercept {
    use std::ffi::{c_void, CString};
    use std::os::raw::c_char;
    use std::sync::OnceLock;

    static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

    // NSApplicationTerminateReply
    const NS_TERMINATE_CANCEL: usize = 0;
    const NS_TERMINATE_NOW: usize = 1;

    type ObjcClass = *mut c_void;
    type ObjcSel = *mut c_void;
    type ObjcImp = unsafe extern "C" fn();

    #[link(name = "objc", kind = "dylib")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> ObjcClass;
        fn sel_registerName(name: *const c_char) -> ObjcSel;
        fn class_addMethod(
            cls: ObjcClass,
            name: ObjcSel,
            imp: ObjcImp,
            types: *const c_char,
        ) -> bool;
    }

    extern "C" fn application_should_terminate(
        _self: *mut c_void,
        _sel: ObjcSel,
        _sender: *mut c_void,
    ) -> usize {
        let Some(app) = APP_HANDLE.get() else {
            // Not installed yet — let Cocoa proceed.
            return NS_TERMINATE_NOW;
        };
        if super::handle_quit_attempt(app, "applicationShouldTerminate") {
            NS_TERMINATE_NOW
        } else {
            NS_TERMINATE_CANCEL
        }
    }

    /// Add `applicationShouldTerminate:` to tao's `TaoAppDelegateParent`.
    /// Idempotent — calling twice silently fails the second add (which is
    /// the documented behaviour of `class_addMethod` for an existing
    /// selector) and we just keep the first registration.
    pub fn install(app: tauri::AppHandle) {
        let _ = APP_HANDLE.set(app);
        unsafe {
            let class_name = CString::new("TaoAppDelegateParent").unwrap();
            let cls = objc_getClass(class_name.as_ptr());
            if cls.is_null() {
                eprintln!("[shutdown] objc_getClass(TaoAppDelegateParent) returned null — Cmd+Q intercept not installed (tao internals changed?)");
                return;
            }
            let sel_name = CString::new("applicationShouldTerminate:").unwrap();
            let sel = sel_registerName(sel_name.as_ptr());
            // Type encoding: NSUInteger return (Q), self (@), _cmd (:), NSApplication* (@).
            let types = CString::new("Q@:@").unwrap();
            let imp: ObjcImp = std::mem::transmute(
                application_should_terminate
                    as extern "C" fn(*mut c_void, ObjcSel, *mut c_void) -> usize,
            );
            let added = class_addMethod(cls, sel, imp, types.as_ptr());
            eprintln!("[shutdown] applicationShouldTerminate: install added={added}");
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::default())
        .setup(|_app| {
            #[cfg(target_os = "macos")]
            {
                // tao has already created and assigned the app delegate by
                // the time `setup` runs — adding the missing selector now
                // lights it up on the live instance.
                macos_quit_intercept::install(_app.handle().clone());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::load_config,
            commands::save_config,
            commands::generate_token,
            commands::pick_directory,
            commands::write_file,
            commands::read_file,
            commands::path_exists,
            commands::home_dir,
            commands::start_server,
            commands::mark_session_active,
            commands::stop_server,
            commands::server_running,
            commands::run_command,
        ])
        .on_window_event(|window, event| {
            // Cmd+W / red dot / Linux+Windows window-close path.
            // macOS Cmd+Q is intercepted in `macos_quit_intercept` instead.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle().clone();
                if !handle_quit_attempt(&app, "WindowEvent::CloseRequested") {
                    api.prevent_close();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Hold My Beer GUI")
        .run(|app, event| {
            // Programmatic exit path — `app.exit(n)` from our async cleanup
            // task after the user clicked "Yes" in the quit dialog.
            // tao/wry only fires `ExitRequested` for window-Destroyed and
            // `Message::RequestExit`; macOS Cmd+Q does NOT come through here.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                // `code == Some(_)` means *we* are initiating the exit (from
                // `app.exit(n)` after cleanup finished). Don't loop.
                if code.is_some() {
                    eprintln!("[shutdown] ExitRequested (self-initiated), allowing");
                    return;
                }
                if !handle_quit_attempt(app, "RunEvent::ExitRequested") {
                    api.prevent_exit();
                }
            }
        });
}
