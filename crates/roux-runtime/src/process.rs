/// Look up the current working directory of an OS process by PID.
///
/// Returns `None` if the PID does not exist, the caller lacks permission, or
/// the OS refuses the lookup.
#[cfg(target_os = "macos")]
pub fn cwd_for_pid(pid: u32) -> Option<String> {
    // proc_pidinfo(pid, PROC_PIDVNODEPATHINFO, 0, &mut info, sizeof(info))
    // fills a proc_vnodepathinfo struct; its pvi_cdir.vip_path is the cwd as
    // a NUL-terminated C string of length MAXPATHLEN (1024 on Darwin).
    const MAXPATHLEN: usize = 1024;
    const VNODE_INFO_SIZE: usize = 152;
    const VNODE_INFO_PATH_SIZE: usize = VNODE_INFO_SIZE + MAXPATHLEN;
    const PROC_PIDVNODEPATHINFO: libc::c_int = 9;

    #[repr(C)]
    struct ProcVnodePathInfo {
        pvi_cdir: [u8; VNODE_INFO_PATH_SIZE],
        pvi_rdir: [u8; VNODE_INFO_PATH_SIZE],
    }

    extern "C" {
        fn proc_pidinfo(
            pid: libc::c_int,
            flavor: libc::c_int,
            arg: u64,
            buffer: *mut libc::c_void,
            buffersize: libc::c_int,
        ) -> libc::c_int;
    }

    let mut info = ProcVnodePathInfo {
        pvi_cdir: [0u8; VNODE_INFO_PATH_SIZE],
        pvi_rdir: [0u8; VNODE_INFO_PATH_SIZE],
    };
    let size = std::mem::size_of::<ProcVnodePathInfo>() as libc::c_int;

    let ret = unsafe {
        proc_pidinfo(
            pid as libc::c_int,
            PROC_PIDVNODEPATHINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    if ret <= 0 {
        return None;
    }

    let path_bytes = &info.pvi_cdir[VNODE_INFO_SIZE..];
    let nul = path_bytes.iter().position(|&b| b == 0).unwrap_or(path_bytes.len());
    if nul == 0 {
        return None;
    }
    std::str::from_utf8(&path_bytes[..nul]).ok().map(|s| s.to_string())
}

#[cfg(target_os = "linux")]
pub fn cwd_for_pid(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok().map(|p| p.to_string_lossy().into_owned())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn cwd_for_pid(_pid: u32) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cwd_for_pid_returns_current_process_cwd() {
        let pid = std::process::id();
        let cwd = cwd_for_pid(pid).expect("cwd_for_pid should resolve for self");
        let expected = std::env::current_dir().expect("current_dir");
        assert_eq!(
            std::fs::canonicalize(&cwd).unwrap(),
            std::fs::canonicalize(&expected).unwrap(),
            "cwd_for_pid(self) should match std::env::current_dir()"
        );
    }

    #[test]
    fn cwd_for_pid_returns_none_for_nonexistent_pid() {
        assert!(cwd_for_pid(0).is_none());
    }
}
