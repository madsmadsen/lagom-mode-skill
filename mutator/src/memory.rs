//! Resident memory of a process group (MU-25). The original has no memory limit, so this file
//! has no counterpart in it. License: MIT License, as the rest of the port.

const BYTES_PER_MIB: u64 = 1024 * 1024;

/// The limit in bytes for a `--max-memory-mb` value; `None` when the value is 0 (limit off).
pub fn limit_bytes(megabytes: u64) -> Option<u64> {
    (megabytes != 0).then(|| megabytes.saturating_mul(BYTES_PER_MIB))
}

/// True when `used` is more than `limit`. No limit is never over.
pub fn is_over(used: u64, limit: Option<u64>) -> bool {
    limit.is_some_and(|limit| used > limit)
}

/// Total resident bytes of every process in the process group `pgid`. Zero when the group is
/// gone. An error when the system refuses to tell, for example in a sandbox.
#[cfg(target_os = "macos")]
pub fn group_resident_bytes(pgid: i32) -> std::io::Result<u64> {
    group_pids(pgid)?.into_iter().map(resident_bytes).sum()
}

/// `PROC_PGRP_ONLY` of `<sys/proc_info.h>`: `proc_listpids` lists the processes of one group.
#[cfg(target_os = "macos")]
const PROC_PGRP_ONLY: u32 = 2;

/// The pids in group `pgid`. The buffer grows until the list fits. `proc_listpids` returns 0
/// both for an empty group and for a failure, so `errno` tells the two apart.
#[cfg(target_os = "macos")]
fn group_pids(pgid: i32) -> std::io::Result<Vec<i32>> {
    let Ok(group) = u32::try_from(pgid) else {
        return Ok(Vec::new());
    };
    let mut pids = vec![i32::default(); 256];
    loop {
        let size = i32::try_from(pids.len() * size_of::<i32>()).unwrap_or(i32::MAX);
        // SAFETY: `__error` returns the address of the thread's `errno`, which is writable.
        unsafe { *libc::__error() = i32::default() };
        // SAFETY: `pids` holds `size` writable bytes, and `proc_listpids` writes at most that many.
        let bytes =
            unsafe { libc::proc_listpids(PROC_PGRP_ONLY, group, pids.as_mut_ptr().cast(), size) };
        let count = usize::try_from(bytes).unwrap_or_default() / size_of::<i32>();
        if let Some(error) = listing_error(count, std::io::Error::last_os_error()) {
            return Err(error);
        }
        if count < pids.len() {
            pids.truncate(count);
            return Ok(pids);
        }
        pids.resize(pids.len() * 2, i32::default());
    }
}

/// The failure of a listing that found nothing. `proc_listpids` returns 0 for a failure too, and
/// then `errno` is not 0.
#[cfg(target_os = "macos")]
fn listing_error(count: usize, error: std::io::Error) -> Option<std::io::Error> {
    (count == 0 && error.raw_os_error().is_some_and(|code| code != 0)).then_some(error)
}

/// Resident bytes of one process; zero when it is gone.
#[cfg(target_os = "macos")]
fn resident_bytes(pid: i32) -> std::io::Result<u64> {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
    let size = i32::try_from(size_of::<libc::proc_taskinfo>()).unwrap_or(i32::MAX);
    // SAFETY: `info` is a zeroed buffer of `size` bytes, the size of `proc_taskinfo`.
    let filled = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKINFO,
            u64::default(),
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if filled == size {
        // SAFETY: `proc_pidinfo` filled the whole struct.
        return Ok(unsafe { info.assume_init() }.pti_resident_size);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(0),
        _ => Err(error),
    }
}

/// Total resident bytes of every process in the process group `pgid`, from `/proc`.
#[cfg(not(target_os = "macos"))]
pub fn group_resident_bytes(pgid: i32) -> std::io::Result<u64> {
    // SAFETY: `sysconf` has no preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    proc_group_bytes(
        std::path::Path::new("/proc"),
        pgid,
        u64::try_from(page).unwrap_or(4096),
    )
}

/// Sums the resident pages of every `<proc_dir>/<pid>/stat` whose process group is `pgid`.
/// Linux reads `/proc` this way. The function is public so that every host can test it. An
/// error when `proc_dir` cannot be listed. A process that ends during the read counts as zero.
pub fn proc_group_bytes(
    proc_dir: &std::path::Path,
    pgid: i32,
    page_size: u64,
) -> std::io::Result<u64> {
    let total = std::fs::read_dir(proc_dir)?
        .flatten()
        .filter_map(|entry| std::fs::read_to_string(entry.path().join("stat")).ok())
        .filter_map(|text| parse_stat(&text))
        .filter(|(group, _)| *group == pgid)
        .map(|(_, pages)| pages.saturating_mul(page_size))
        .sum();
    Ok(total)
}

/// The stderr line for a read that failed. The run goes on without the memory limit.
pub fn unreadable_notice(error: &std::io::Error) -> String {
    format!(
        "Cannot read the memory of the test run ({error}). The memory limit does not apply to it."
    )
}

/// The process group and the resident pages of one `/proc/<pid>/stat` line. The command name
/// sits in parentheses and can hold spaces and parentheses, so the fields start after the last `)`.
pub fn parse_stat(text: &str) -> Option<(i32, u64)> {
    let (_, rest) = text.rsplit_once(')')?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // After the name: state, ppid, pgrp, ..., rss is the 22nd field after the name.
    Some((fields.get(2)?.parse().ok()?, fields.get(21)?.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MU-25: 0 turns the limit off, and N is N MiB in bytes.
    #[test]
    fn mu_25_limit_bytes_is_mebibytes_and_zero_is_off() {
        assert_eq!(limit_bytes(0), None);
        assert_eq!(limit_bytes(1), Some(1_048_576));
        assert_eq!(limit_bytes(4096), Some(4_294_967_296));
        assert_eq!(limit_bytes(u64::MAX), Some(u64::MAX));
    }

    /// MU-25: the memory is over the limit only when it is more than the limit.
    #[test]
    fn mu_25_is_over_means_strictly_more_than_the_limit() {
        assert!(!is_over(u64::MAX, None));
        assert!(!is_over(99, Some(100)));
        assert!(!is_over(100, Some(100)));
        assert!(is_over(101, Some(100)));
    }

    /// MU-25: a group of two processes counts both, and a pid that is not a group counts nothing.
    #[cfg(target_os = "macos")]
    #[test]
    fn mu_25_group_resident_bytes_counts_every_process_of_the_group() {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("sh")
            .args(["-c", "sleep 30 & sleep 30; wait"])
            .process_group(0)
            .spawn()
            .unwrap();
        let pgid = i32::try_from(child.id()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let pids = group_pids(pgid).unwrap();
        let total = group_resident_bytes(pgid).unwrap();
        // SAFETY: `pgid` names the group of the child spawned above.
        unsafe { libc::kill(-pgid, libc::SIGKILL) };
        let _ = child.wait();
        assert!(pids.len() >= 3, "sh and two sleeps, got {pids:?}");
        assert!(total >= 3 * 16 * 1024, "resident bytes: {total}");
        assert_eq!(group_resident_bytes(pgid).unwrap(), 0, "the group is gone");
        assert_eq!(group_resident_bytes(-1).unwrap(), 0);
    }

    /// MU-25: an empty listing is a failure only when `errno` is set, and a full one never is.
    #[cfg(target_os = "macos")]
    #[test]
    fn mu_25_an_empty_listing_with_errno_set_is_a_failure() {
        let denied = || std::io::Error::from_raw_os_error(libc::EPERM);
        let clear = || std::io::Error::from_raw_os_error(i32::default());
        assert_eq!(
            listing_error(0, denied()).unwrap().raw_os_error(),
            Some(libc::EPERM)
        );
        assert!(listing_error(0, clear()).is_none());
        assert!(listing_error(3, denied()).is_none());
        assert!(listing_error(0, std::io::Error::from(std::io::ErrorKind::Other)).is_none());
    }

    /// MU-25: a process that is gone has no resident memory, and that is not a failure.
    #[cfg(target_os = "macos")]
    #[test]
    fn mu_25_resident_bytes_of_a_gone_process_is_zero() {
        assert_eq!(resident_bytes(i32::MAX).unwrap(), 0);
    }

    /// MU-25: a group larger than the first buffer is listed whole.
    #[cfg(target_os = "macos")]
    #[test]
    fn mu_25_group_pids_lists_more_processes_than_the_first_buffer() {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("sh")
            .args([
                "-c",
                "i=0; while [ $i -lt 300 ]; do sleep 30 & i=$((i+1)); done; wait",
            ])
            .process_group(0)
            .spawn()
            .unwrap();
        let pgid = i32::try_from(child.id()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let count = group_pids(pgid).unwrap().len();
        // SAFETY: `pgid` names the group of the child spawned above.
        unsafe { libc::kill(-pgid, libc::SIGKILL) };
        let _ = child.wait();
        assert!(count > 256, "listed {count}");
    }

    /// MU-25: the Linux reader takes pgrp and rss from `stat`, even when the name holds `) (`.
    #[test]
    fn mu_25_proc_stat_gives_group_and_resident_pages() {
        let text = "123 (a b) (c) S 1 777 777 0 -1 4194304 1 2 3 4 5 6 7 8 20 0 1 0 100 1000 25 18446744073709551615 0 0";
        assert_eq!(parse_stat(text), Some((777, 25)));
        assert_eq!(parse_stat("garbage"), None);
        assert_eq!(parse_stat("1 (x) S 1"), None);
    }

    /// MU-25: the Linux reader adds the processes of one group and skips the others.
    #[test]
    fn mu_25_proc_group_bytes_sums_only_the_group() {
        let dir = tempfile::tempdir().unwrap();
        let stat = |pid: &str, group: u32, pages: u32| {
            let line =
                format!("{pid} (x) S 1 {group} 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 1 1 {pages} 0");
            std::fs::create_dir(dir.path().join(pid)).unwrap();
            std::fs::write(dir.path().join(pid).join("stat"), line).unwrap();
        };
        stat("10", 5, 3);
        stat("11", 5, 4);
        stat("12", 6, 100);
        std::fs::create_dir(dir.path().join("self")).unwrap();
        assert_eq!(proc_group_bytes(dir.path(), 5, 4096).unwrap(), 7 * 4096);
        assert_eq!(proc_group_bytes(dir.path(), 9, 4096).unwrap(), 0);
    }

    /// MU-25: a `/proc` that cannot be listed is an error, not zero bytes, and the notice names it.
    #[test]
    fn mu_25_an_unreadable_proc_is_an_error_and_the_notice_says_the_limit_is_off() {
        let dir = tempfile::tempdir().unwrap();
        let error = proc_group_bytes(&dir.path().join("none"), 5, 4096).unwrap_err();
        let notice = unreadable_notice(&error);
        assert!(
            notice.starts_with("Cannot read the memory of the test run ("),
            "{notice}"
        );
        assert!(
            notice.ends_with(" The memory limit does not apply to it."),
            "{notice}"
        );
        assert!(notice.contains(&error.to_string()), "{notice}");
    }
}
