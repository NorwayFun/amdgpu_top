use std::io::Read;
use std::fs;
use std::time::Duration;
use std::path::{Path, PathBuf};
use crate::DevicePath;

#[derive(Debug, Default, Clone)]
pub struct ProcInfo {
    pub pid: i32,
    pub name: String,
    pub fds: Vec<i32>,
}

fn get_fds<T: AsRef<Path>>(fd_dir_path: &mut PathBuf, device_path: &[T]) -> Vec<i32> {
    let Ok(fd_list) = fs::read_dir(&fd_dir_path) else { return Vec::new() };

    fd_list.filter_map(|dir_entry| {
        let fd = dir_entry.ok()?.file_name();
        let link = {
            fd_dir_path.push(fd.clone());
            let link = fs::read_link(&fd_dir_path).ok()?;
            fd_dir_path.pop();
            link
        };

        // e.g. "/dev/dri/renderD128" or "/dev/dri/card0"
        if device_path.iter().any(|path| link.starts_with(path)) {
            fd.to_str()?.parse::<i32>().ok()
        } else {
            None
        }
    }).collect()
}

pub fn get_process_list() -> Vec<i32> {
    const SYSTEMD_CMDLINE: &[&[u8]] = &[ b"/lib/systemd", b"/usr/lib/systemd" ];

    let Ok(proc_dir) = fs::read_dir("/proc") else { return Vec::new() };
    let mut buf_cmdline = [0u8; 16];

    proc_dir.filter_map(|dir_entry| {
        let dir_entry = dir_entry.ok()?;
        let metadata = dir_entry.metadata().ok()?;

        if !metadata.is_dir() { return None }

        let pid = dir_entry.file_name().to_str()?.parse::<i32>().ok()?;

        if pid == 1 { return None } // init process, systemd

        // filter systemd processes from fdinfo target
        // gnome-shell share the AMDGPU driver context with systemd processes
        {
            buf_cmdline = Default::default();
            let mut f = fs::File::open(format!("/proc/{pid}/cmdline")).ok()?;
            f.read_exact(&mut buf_cmdline).ok()?;

            if SYSTEMD_CMDLINE.iter().any(|path| buf_cmdline.starts_with(path)) {
                return None;
            }
        }

        Some(pid)
    }).collect()
}

pub fn update_index_by_all_proc<T: AsRef<Path>>(
    vec_info: &mut Vec<ProcInfo>,
    device_path: &[T],
    all_proc: &[i32],
) {
    vec_info.clear();

    let mut buf_path = PathBuf::with_capacity(32);
    let mut buf_name = String::with_capacity(16);

    for p in all_proc {
        buf_path.clear();
        buf_name.clear();

        let pid = *p;

        buf_path.push("/proc");
        buf_path.push(pid.to_string());

        let fds = get_fds(&mut buf_path.join("fd/"), device_path);

        if fds.is_empty() { continue }

        buf_path.push("comm");

        // Maximum 16 characters
        // https://www.kernel.org/doc/html/latest/filesystems/proc.html#proc-pid-comm-proc-pid-task-tid-comm
        let Ok(mut f) = fs::File::open(&buf_path) else { continue };
        if f.read_to_string(&mut buf_name).is_err() { continue }
        buf_name.pop(); // trim '\n'
        let name = buf_name.clone();

        vec_info.push(ProcInfo { pid, name, fds });
    }
}

pub fn update_index(vec_info: &mut Vec<ProcInfo>, device_path: &DevicePath) {
    update_index_by_all_proc(
        vec_info,
        &[&device_path.render, &device_path.card],
        &get_process_list(),
    );
}

pub fn spawn_update_index_thread(
    device_paths: Vec<DevicePath>,
    interval: u64,
) {
    let mut buf_index: Vec<ProcInfo> = Vec::new();
    let interval = Duration::from_secs(interval);

    std::thread::spawn(move || loop {
        let all_proc = get_process_list();

        for device_path in &device_paths {
            update_index_by_all_proc(
                &mut buf_index,
                &[&device_path.render, &device_path.card],
                &all_proc,
            );

            let lock = device_path.arc_proc_index.lock();
            if let Ok(mut index) = lock {
                index.clone_from(&buf_index);
            }
        }

        std::thread::sleep(interval);
    });
}

// Calculate usage (%) from previous and current usage (ns)
pub fn diff_usage(pre_usage_ns: i64, cur_usage_ns: i64, interval: &Duration) -> i64 {
    let diff_ns = if pre_usage_ns == 0 || cur_usage_ns < pre_usage_ns {
        return 0;
    } else {
        cur_usage_ns.saturating_sub(pre_usage_ns) as u128
    };

    (diff_ns * 100)
        .checked_div(interval.as_nanos())
        .unwrap_or(0) as i64
}
