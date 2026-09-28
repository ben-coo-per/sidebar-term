//! libproc-based reads of the Foreground process group (members, comm, path, argv, cwd) and of
//! a process's children (for Suites' walk of a Session's tree).
//! The macOS backend of `detect::os`; libproc lives in libSystem, so no crate beyond `libc`.
//! Never panics: every syscall failure (process gone, other uid, zombie) reads as `None` / empty.
//! Sources: docs/research/agent-detection.md, docs/research/cwd-git.md. OWNER: detection agent.

use super::{non_empty, Proc};
use libc::{c_char, c_int, c_void, pid_t};
use std::mem::{size_of, MaybeUninit};
use std::ptr;

/// `<sys/proc_info.h>`: `proc_listpids` type selecting the members of one process group.
/// Not exported by the `libc` crate.
const PROC_PGRP_ONLY: u32 = 2;
/// `<sys/proc_info.h>`: `proc_listpids` type selecting the children of one process.
const PROC_PPID_ONLY: u32 = 6;
/// `<sys/proc.h>`: `p_stat` of a zombie.
const SZOMB: u32 = 5;
/// Upper bound on process-group size we bother to list (a pty's foreground group is tiny).
const MAX_MEMBERS: usize = 4096;
/// Upper bound on argc we parse out of `KERN_PROCARGS2`.
const MAX_ARGS: usize = 4096;

/// Pids of every process in process group `pgid`, ascending. Empty when the group does not
/// exist (or on any error).
pub fn group_members(pgid: pid_t) -> Vec<pid_t> {
    list_pids(PROC_PGRP_ONLY, pgid)
}

/// Pids of the direct children of `ppid`, ascending. Empty when it has none (or on any error).
/// A recursive walk over this is how a Session's whole process tree is read without `ps`.
pub fn children(ppid: pid_t) -> Vec<pid_t> {
    list_pids(PROC_PPID_ONLY, ppid)
}

fn list_pids(kind: u32, id: pid_t) -> Vec<pid_t> {
    if id <= 0 {
        return Vec::new();
    }
    let mut cap = 16usize;
    loop {
        let mut buf: Vec<pid_t> = vec![0; cap];
        let bytes = (cap * size_of::<pid_t>()) as c_int;
        // SAFETY: `buf` is a writable buffer of exactly `bytes` bytes.
        let n = unsafe {
            libc::proc_listpids(kind, id as u32, buf.as_mut_ptr().cast::<c_void>(), bytes)
        };
        if n <= 0 {
            return Vec::new();
        }
        let count = (n as usize / size_of::<pid_t>()).min(cap);
        if count < cap || cap >= MAX_MEMBERS {
            buf.truncate(count);
            buf.retain(|&p| p > 0);
            buf.sort_unstable();
            buf.dedup();
            return buf;
        }
        // The buffer was filled: the list may be longer. Retry with more room.
        cap *= 4;
    }
}

/// `comm` of a live process (`PROC_PIDT_SHORTBSDINFO`). Works across uids. `None` for a
/// zombie or a vanished pid. `pbsi_comm` is the exec'd file's *resolved* name, truncated to
/// 16 bytes (`MAXCOMLEN`): a symlink `claude -> versions/2.1.267` reads as `2.1.267`.
pub fn short_info(pid: pid_t) -> Option<Proc> {
    if pid <= 0 {
        return None;
    }
    let size = size_of::<libc::proc_bsdshortinfo>() as c_int;
    let mut info = MaybeUninit::<libc::proc_bsdshortinfo>::zeroed();
    // SAFETY: `info` is a zeroed, writable `proc_bsdshortinfo` of `size` bytes.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDT_SHORTBSDINFO,
            0,
            info.as_mut_ptr().cast::<c_void>(),
            size,
        )
    };
    if n != size {
        return None;
    }
    // SAFETY: the kernel filled all `size` bytes; the struct is plain data (all-zero is valid too).
    let info = unsafe { info.assume_init() };
    if info.pbsi_status == SZOMB {
        return None;
    }
    Some(Proc {
        pid,
        ppid: info.pbsi_ppid as pid_t,
        comm: c_chars_to_string(&info.pbsi_comm),
    })
}

/// Resolved executable path (`proc_pidpath`). Works across uids; `None` once the file was
/// deleted (verified: the auto-updater removes old Claude Code versions while they run).
pub fn exe_path(pid: pid_t) -> Option<String> {
    if pid <= 0 {
        return None;
    }
    let mut buf = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: `buf` is writable for its full length.
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast::<c_void>(), buf.len() as u32) };
    if n <= 0 {
        return None;
    }
    let bytes = buf.get(..n as usize)?;
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    non_empty(String::from_utf8_lossy(&bytes[..end]).into_owned())
}

/// Current working directory (`PROC_PIDVNODEPATHINFO`), as the kernel's canonical vnode path
/// (`/private/tmp`, not `/tmp`). `None` for processes of another uid (EPERM, e.g. `sudo`).
pub fn cwd(pid: pid_t) -> Option<String> {
    if pid <= 0 {
        return None;
    }
    let size = size_of::<libc::proc_vnodepathinfo>() as c_int;
    let mut info = MaybeUninit::<libc::proc_vnodepathinfo>::zeroed();
    // SAFETY: `info` is a zeroed, writable `proc_vnodepathinfo` of `size` bytes.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            info.as_mut_ptr().cast::<c_void>(),
            size,
        )
    };
    if n != size {
        return None;
    }
    // SAFETY: filled by the kernel; plain data.
    let info = unsafe { info.assume_init() };
    // `vip_path` is `char[MAXPATHLEN]`, declared by `libc` as `[[c_char; 32]; 32]`.
    let path: &[[c_char; 32]; 32] = &info.pvi_cdir.vip_path;
    let flat: Vec<c_char> = path.iter().flatten().copied().collect();
    non_empty(c_chars_to_string(&flat))
}

/// Full argv (`sysctl KERN_PROCARGS2`). Same-uid processes only; `None` otherwise.
pub fn argv(pid: pid_t) -> Option<Vec<String>> {
    parse_procargs2(&procargs2(pid)?)
}

/// Full argv and the environment the process was exec'd with (`KERN_PROCARGS2`), as
/// `KEY=value` strings. Same-uid processes only; `None` otherwise. The environment is empty for
/// Apple's platform binaries (`/bin/sleep`, `/bin/zsh`): the kernel withholds it (verified).
pub fn argv_env(pid: pid_t) -> Option<(Vec<String>, Vec<String>)> {
    let buf = procargs2(pid)?;
    let (argv, rest) = split_procargs2(&buf)?;
    Some((argv, parse_env(rest)))
}

/// The raw `KERN_PROCARGS2` buffer of `pid`.
fn procargs2(pid: pid_t) -> Option<Vec<u8>> {
    if pid <= 0 {
        return None;
    }
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size: libc::size_t = 0;
    // Size query first: the kernel copies the *tail* of the args area when the buffer is
    // smaller than it, so the buffer must be at least this big.
    // SAFETY: a null `oldp` asks only for the size, written to `size`.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            ptr::null_mut(),
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size];
    // SAFETY: `buf` is writable for `size` bytes and `size` says so.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast::<c_void>(),
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    buf.truncate(size);
    Some(buf)
}

/// Parse a `KERN_PROCARGS2` buffer: `int argc`, the exec path, NUL padding, then `argc`
/// NUL-terminated argv strings (then env, ignored).
fn parse_procargs2(buf: &[u8]) -> Option<Vec<String>> {
    split_procargs2(buf).map(|(argv, _)| argv)
}

/// The argv of a `KERN_PROCARGS2` buffer and the bytes after it (the environment).
fn split_procargs2(buf: &[u8]) -> Option<(Vec<String>, &[u8])> {
    let argc = i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?);
    if argc <= 0 {
        return None;
    }
    let argc = (argc as usize).min(MAX_ARGS);
    let rest = buf.get(4..)?;
    let exec_end = rest.iter().position(|&b| b == 0)?;
    let rest = rest.get(exec_end..)?;
    let first = rest.iter().position(|&b| b != 0)?;
    let mut rest = rest.get(first..)?;
    let mut args = Vec::with_capacity(argc.min(64));
    while args.len() < argc && !rest.is_empty() {
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        args.push(String::from_utf8_lossy(&rest[..end]).into_owned());
        rest = rest.get(end + 1..).unwrap_or(&[]);
    }
    Some((args, rest))
}

/// The environment strings after argv: NUL-terminated, ended by an empty string (what follows
/// that is the kernel's own `apple` strings, not the environment).
fn parse_env(buf: &[u8]) -> Vec<String> {
    buf.split(|&b| b == 0)
        .take_while(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

fn c_chars_to_string(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_procargs2_shapes() {
        let mut buf = 3i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/bin/sleep\0\0\0\0sleep\0-n\x0030\0HOME=/x\0\0");
        assert_eq!(parse_procargs2(&buf).unwrap(), ["sleep", "-n", "30"]);
        // Truncated / garbage buffers never panic.
        assert_eq!(parse_procargs2(&[]), None);
        assert_eq!(parse_procargs2(&[1, 0]), None);
        assert_eq!(parse_procargs2(&0i32.to_ne_bytes()), None);
        let mut short = 5i32.to_ne_bytes().to_vec();
        short.extend_from_slice(b"/x\0\0a\0b");
        assert_eq!(parse_procargs2(&short).unwrap(), ["a", "b"]);
        let mut no_nul = 1i32.to_ne_bytes().to_vec();
        no_nul.extend_from_slice(b"/bin/sleep");
        assert_eq!(parse_procargs2(&no_nul), None);
    }

    #[test]
    fn procargs2_environment_stops_at_the_apple_strings() {
        let mut buf = 1i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/bin/sleep\0\0sleep\0HOME=/x\0PATH=/bin\0\0executable_path=/y\0");
        let (argv, rest) = split_procargs2(&buf).unwrap();
        assert_eq!(argv, ["sleep"]);
        assert_eq!(parse_env(rest), ["HOME=/x", "PATH=/bin"]);
        assert!(parse_env(&[]).is_empty());
    }

    #[test]
    fn launchd_reads_across_uids_except_cwd_and_argv() {
        // launchd is root's: name and path are readable across uids, cwd and argv are not.
        assert_eq!(short_info(1).map(|p| p.comm).as_deref(), Some("launchd"));
        assert!(exe_path(1).is_some());
        assert!(cwd(1).is_none());
        assert!(argv(1).is_none());
    }
}
