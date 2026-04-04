//! Integration tests for process lifecycle — specifically that `kill_process`
//! kills the entire process group, including child processes spawned by the worker.
//!
//! This test exists because of a real bug: `collab stop` killed the `collab worker`
//! parent but left child `claude` processes running, burning tokens undetected.

#[cfg(unix)]
mod unix {
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};
    use std::time::Duration;

    /// Returns (parent, child) with parent as process group leader (pgid = parent.id()).
    pub fn spawn_worker_and_child() -> (Child, Child) {
        let parent = Command::new("sleep")
            .arg("300")
            .process_group(0) // become group leader: pgid = own pid
            .spawn()
            .expect("failed to spawn parent");
        let pgid = parent.id();

        std::thread::sleep(Duration::from_millis(200));

        let child = Command::new("sleep")
            .arg("300")
            .process_group(pgid as i32) // join parent's group
            .spawn()
            .expect("failed to spawn child");

        std::thread::sleep(Duration::from_millis(100));

        // Verify pgids via ps before proceeding
        let parent_pgid = get_pgid(parent.id());
        let child_pgid = get_pgid(child.id());
        assert_eq!(parent_pgid, Some(pgid), "parent pgid mismatch");
        assert_eq!(child_pgid, Some(pgid), "child did not join parent's process group");

        (parent, child)
    }

    pub fn get_pgid(pid: u32) -> Option<u32> {
        let out = Command::new("ps")
            .args(["-o", "pgid=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }

    /// True if process is running (not zombie, not dead). Uses ps state column.
    pub fn process_running(pid: u32) -> bool {
        let out = match Command::new("ps")
            .args(["-o", "state=", "-p", &pid.to_string()])
            .output()
        {
            Ok(o) => o,
            Err(_) => return false,
        };
        let state = String::from_utf8_lossy(&out.stdout);
        let s = state.trim();
        // Z = zombie (dead but not reaped), empty = not found
        !s.is_empty() && !s.starts_with('Z')
    }
}

#[test]
#[cfg(unix)]
fn killpg_kills_parent_and_child() {
    let (mut parent, mut child) = unix::spawn_worker_and_child();
    let parent_pid = parent.id();
    let child_pid = child.id();
    let pgid = parent_pid;

    assert!(unix::process_running(parent_pid), "parent should be running before kill");
    assert!(unix::process_running(child_pid), "child should be running before kill");

    let ret = unsafe { libc::killpg(pgid as libc::pid_t, libc::SIGTERM) };
    assert_eq!(ret, 0, "killpg failed");

    // Wait for both to exit (try_wait is non-blocking, reaps zombie)
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let _ = parent.try_wait();
        let _ = child.try_wait();
        if !unix::process_running(parent_pid) && !unix::process_running(child_pid) {
            break;
        }
        if std::time::Instant::now() >= deadline {
            unsafe { libc::killpg(pgid as libc::pid_t, libc::SIGKILL); }
            let _ = parent.wait();
            let _ = child.wait();
            panic!(
                "Timed out: parent_running={} child_running={} — killpg left orphans",
                unix::process_running(parent_pid),
                unix::process_running(child_pid),
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    assert!(!unix::process_running(parent_pid), "parent must be dead after killpg");
    assert!(!unix::process_running(child_pid), "child must be dead after killpg — no orphaned processes");
}

#[test]
#[cfg(unix)]
fn killing_only_parent_leaves_child_alive_documents_the_bug() {
    // Documents the OLD buggy behavior: killing just the parent PID leaves children running.
    let (mut parent, mut child) = unix::spawn_worker_and_child();
    let parent_pid = parent.id();
    let child_pid = child.id();

    // Old behavior: kill only the parent PID
    unsafe { libc::kill(parent_pid as libc::pid_t, libc::SIGTERM); }
    let _ = parent.wait(); // reap zombie
    std::thread::sleep(std::time::Duration::from_millis(300));

    let child_still_running = unix::process_running(child_pid);

    // Clean up
    unsafe { libc::kill(child_pid as libc::pid_t, libc::SIGKILL); }
    let _ = child.wait();

    assert!(
        child_still_running,
        "Child survived parent kill — documents the bug that cost $160. If this fails, OS behavior changed."
    );
}
