//! Are any `collab worker` processes for this project still running?
//!
//! The quit prompt ("Stop workers before quitting?") must only appear when
//! workers are really running. It used to fire whenever a project directory
//! was set, so it warned on a host with no workers at all, and after workers
//! had already been stopped or killed.
//!
//! The source of truth is the same one `collab ps` / `collab stop` use: the
//! `workers.pids` file the CLI writes when it starts a worker, a JSON map of
//! `name -> { pid, started_at, command }`. For a team.yml project it lives in
//! `~/.collab/teams/<team>/workers.pids`; for a legacy single-repo project in
//! `<project>/.collab/workers.pids`. An entry only counts if its pid is
//! still alive, because the CLI leaves entries behind when a worker dies.

use std::path::{Path, PathBuf};

/// What the pids files say about running workers.
#[derive(Debug, PartialEq, Eq)]
pub enum Running {
    /// No worker process is alive (or no worker was ever started).
    None,
    /// These workers (name, pid) are alive.
    Workers(Vec<(String, u32)>),
    /// A pids file exists but could not be read or parsed. The caller should
    /// assume workers may be running: a needless prompt costs a click, a
    /// missing one leaves workers spending tokens.
    Unknown,
}

impl Running {
    pub fn should_prompt(&self) -> bool {
        match self {
            Running::None => false,
            Running::Workers(list) => !list.is_empty(),
            Running::Unknown => true,
        }
    }
}

/// The `team:` value from a team.yml, unquoted.
pub fn team_name_from_yaml(text: &str) -> Option<String> {
    // team.yml is written by the wizard (`team: "<name>"` at top level) and
    // read by the CLI's YAML parser. Only the top-level key matters here.
    text.lines().find_map(|line| {
        let rest = line.strip_prefix("team:")?;
        let value = rest.split(" #").next().unwrap_or("").trim();
        let unquoted = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        (!unquoted.is_empty()).then(|| unquoted.to_string())
    })
}

/// Every place the CLI may have recorded workers for `project_dir`.
pub fn pid_files(project_dir: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let team = std::fs::read_to_string(project_dir.join("team.yml"))
        .ok()
        .and_then(|text| team_name_from_yaml(&text));
    if let (Some(team), Some(home)) = (team, home) {
        files.push(
            home.join(".collab")
                .join("teams")
                .join(team)
                .join("workers.pids"),
        );
    }
    files.push(project_dir.join(".collab").join("workers.pids"));
    files
}

/// Read the pids files and keep the entries whose pid `alive` says is running.
pub fn running_workers(files: &[PathBuf], alive: impl Fn(u32) -> bool) -> Running {
    #[derive(serde::Deserialize)]
    struct Entry {
        pid: u32,
    }

    let mut live = Vec::new();
    for file in files {
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Running::Unknown,
        };
        let entries: std::collections::BTreeMap<String, Entry> = match serde_json::from_str(&text) {
            Ok(entries) => entries,
            Err(_) => return Running::Unknown,
        };
        live.extend(
            entries
                .into_iter()
                .filter(|(_, e)| alive(e.pid))
                .map(|(name, e)| (name, e.pid)),
        );
    }
    if live.is_empty() {
        Running::None
    } else {
        Running::Workers(live)
    }
}

/// Is a process with this pid running?
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        // Signal 0 checks for existence without sending anything. EPERM means
        // the process exists but belongs to someone else: still alive.
        let rc = unsafe { libc::kill(pid, 0) };
        rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(handle, &mut code) != 0;
            CloseHandle(handle);
            ok && code == STILL_ACTIVE as u32
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hmb-workers-{}-{}-{}",
            tag,
            std::process::id(),
            rand::random::<u32>()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn pids_json(entries: &[(&str, u32)]) -> String {
        let map: serde_json::Map<String, serde_json::Value> = entries
            .iter()
            .map(|(name, pid)| {
                (
                    name.to_string(),
                    serde_json::json!({ "pid": pid, "started_at": "2026-11-11T00:00:00Z", "command": "collab worker" }),
                )
            })
            .collect();
        serde_json::Value::Object(map).to_string()
    }

    #[test]
    fn reads_the_team_name_from_team_yml() {
        assert_eq!(
            team_name_from_yaml("team: \"pool\"\nserver: \"x\"\n"),
            Some("pool".into())
        );
        assert_eq!(
            team_name_from_yaml("# c\nteam: pool\n"),
            Some("pool".into())
        );
        assert_eq!(team_name_from_yaml("team: 'pool'\n"), Some("pool".into()));
        assert_eq!(team_name_from_yaml("server: \"x\"\n"), None);
        assert_eq!(team_name_from_yaml("team: \"\"\n"), None);
    }

    #[test]
    fn looks_in_the_team_dir_and_the_legacy_project_dir() {
        let project = scratch("proj");
        let home = scratch("home");
        fs::write(project.join("team.yml"), "team: \"pool\"\n").unwrap();

        let files = pid_files(&project, Some(&home));
        assert_eq!(
            files,
            vec![
                home.join(".collab")
                    .join("teams")
                    .join("pool")
                    .join("workers.pids"),
                project.join(".collab").join("workers.pids"),
            ]
        );
    }

    #[test]
    fn without_team_yml_only_the_legacy_file_is_checked() {
        let project = scratch("proj");
        let home = scratch("home");
        assert_eq!(
            pid_files(&project, Some(&home)),
            vec![project.join(".collab").join("workers.pids")]
        );
    }

    #[test]
    fn no_pids_file_means_no_workers_and_no_prompt() {
        let dir = scratch("none");
        let r = running_workers(&[dir.join("workers.pids")], |_| true);
        assert_eq!(r, Running::None);
        assert!(!r.should_prompt());
    }

    #[test]
    fn a_live_worker_is_reported_and_prompts() {
        let dir = scratch("live");
        let f = dir.join("workers.pids");
        fs::write(&f, pids_json(&[("builder", 4242)])).unwrap();
        let r = running_workers(&[f], |pid| pid == 4242);
        assert_eq!(r, Running::Workers(vec![("builder".into(), 4242)]));
        assert!(r.should_prompt());
    }

    #[test]
    fn entries_left_behind_by_dead_workers_do_not_prompt() {
        // The case that showed the prompt after workers had been killed.
        let dir = scratch("dead");
        let f = dir.join("workers.pids");
        fs::write(&f, pids_json(&[("builder", 4242), ("reviewer", 4343)])).unwrap();
        let r = running_workers(&[f], |_| false);
        assert_eq!(r, Running::None);
        assert!(!r.should_prompt());
    }

    #[test]
    fn only_the_live_ones_are_listed_across_both_files() {
        let a = scratch("a");
        let b = scratch("b");
        fs::write(a.join("workers.pids"), pids_json(&[("builder", 1)])).unwrap();
        fs::write(
            b.join("workers.pids"),
            pids_json(&[("reviewer", 2), ("idler", 3)]),
        )
        .unwrap();
        let r = running_workers(&[a.join("workers.pids"), b.join("workers.pids")], |pid| {
            pid != 2
        });
        assert_eq!(
            r,
            Running::Workers(vec![("builder".into(), 1), ("idler".into(), 3)])
        );
    }

    #[test]
    fn an_unreadable_pids_file_prompts_to_be_safe() {
        let dir = scratch("bad");
        let f = dir.join("workers.pids");
        fs::write(&f, "not json").unwrap();
        let r = running_workers(&[f], |_| false);
        assert_eq!(r, Running::Unknown);
        assert!(r.should_prompt());
    }

    #[test]
    fn this_process_counts_as_alive_and_a_free_pid_does_not() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(u32::MAX - 7));
    }
}
