use crate::models::ResourceSample;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use tracing::debug;

pub struct ResourceSampler {
    clk_tck: f64,
    pid_ticks: HashMap<u32, (u64, Instant)>,
    gpu_sampler: IntelGpuSampler,
}

impl ResourceSampler {
    pub fn new() -> Self {
        let clk_tck = unsafe {
            let tck = libc::sysconf(libc::_SC_CLK_TCK);
            if tck > 0 {
                tck as f64
            } else {
                100.0
            }
        };

        Self {
            clk_tck,
            pid_ticks: HashMap::new(),
            gpu_sampler: IntelGpuSampler::new(),
        }
    }

    pub fn sample(&mut self, app_id: &str, pid: Option<u32>) -> ResourceSample {
        let now = Instant::now();
        let now_ts = chrono::Utc::now().timestamp();
        let gpu_pct = self.gpu_sampler.sample(now);

        let (target_pid, cpu_pct, rss_kb) = if let Some(p) = pid.filter(|&p| p > 0) {
            let (cpu, rss) = self.sample_process(p, now);
            (p as i64, cpu, rss as i64)
        } else {
            (0, 0.0, 0)
        };

        // Garbage collect old tracked PIDs if the map exceeds 32 entries
        if self.pid_ticks.len() > 32 {
            let current_target = target_pid as u32;
            self.pid_ticks.retain(|&k, (_, seen)| {
                k == current_target || now.duration_since(*seen).as_secs() < 60
            });
        }

        ResourceSample {
            ts: now_ts,
            app_id: app_id.to_string(),
            pid: target_pid,
            cpu_pct,
            rss_kb,
            gpu_pct,
        }
    }

    fn sample_process(&mut self, pid: u32, now: Instant) -> (f64, u64) {
        let cpu_pct = match self.read_proc_cpu(pid) {
            Ok(total_ticks) => {
                if let Some((prev_ticks, prev_time)) = self.pid_ticks.insert(pid, (total_ticks, now)) {
                    let delta_ticks = total_ticks.saturating_sub(prev_ticks);
                    let elapsed_secs = now.duration_since(prev_time).as_secs_f64();
                    if elapsed_secs > 0.01 {
                        ((delta_ticks as f64) / self.clk_tck) / elapsed_secs * 100.0
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            }
            Err(_) => {
                self.pid_ticks.remove(&pid);
                0.0
            }
        };

        let rss_kb = self.read_proc_rss(pid).unwrap_or(0);

        (cpu_pct, rss_kb)
    }

    fn read_proc_cpu(&self, pid: u32) -> Result<u64, ()> {
        let path = format!("/proc/{pid}/stat");
        let content = fs::read_to_string(path).map_err(|_| ())?;
        let last_paren = content.rfind(')').ok_or(())?;
        let remainder = &content[last_paren + 1..];
        let mut tokens = remainder.split_whitespace();

        // After ')':
        // index 0: state
        // index 1: ppid
        // ...
        // index 11: utime (field 14)
        // index 12: stime (field 15)
        let utime_str = tokens.nth(11).ok_or(())?;
        let stime_str = tokens.next().ok_or(())?;

        let utime: u64 = utime_str.parse().map_err(|_| ())?;
        let stime: u64 = stime_str.parse().map_err(|_| ())?;

        Ok(utime + stime)
    }

    fn read_proc_rss(&self, pid: u32) -> Result<u64, ()> {
        let path = format!("/proc/{pid}/status");
        let content = fs::read_to_string(path).map_err(|_| ())?;

        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if let Some(val_str) = parts.first() {
                    return val_str.parse::<u64>().map_err(|_| ());
                }
            }
        }
        Err(())
    }
}

pub struct IntelGpuSampler {
    engine_busy_paths: Vec<PathBuf>,
    last_scan: Instant,
    prev_busy_ns: Option<u64>,
    prev_time: Instant,
}

impl IntelGpuSampler {
    pub fn new() -> Self {
        let now = Instant::now();
        let mut sampler = Self {
            engine_busy_paths: Vec::new(),
            last_scan: now,
            prev_busy_ns: None,
            prev_time: now,
        };
        sampler.rescan_engines();
        sampler
    }

    fn rescan_engines(&mut self) {
        self.engine_busy_paths.clear();
        self.last_scan = Instant::now();

        // Scan /sys/class/drm/card*/engine/*/busy_time
        if let Ok(entries) = fs::read_dir("/sys/class/drm") {
            for entry in entries.flatten() {
                let card_path = entry.path();
                let file_name = entry.file_name();
                let name = file_name.to_string_lossy();
                if name.starts_with("card") && !name.contains('-') {
                    let engine_dir = card_path.join("engine");
                    if let Ok(engines) = fs::read_dir(&engine_dir) {
                        for eng in engines.flatten() {
                            let busy_path = eng.path().join("busy_time");
                            if busy_path.is_file() {
                                self.engine_busy_paths.push(busy_path);
                            }
                        }
                    }
                }
            }
        }

        if !self.engine_busy_paths.is_empty() {
            debug!(
                engines = self.engine_busy_paths.len(),
                "Found Intel GPU engine busy_time sysfs files"
            );
        }
    }

    pub fn sample(&mut self, now: Instant) -> f64 {
        // Periodically rescan if no engines were found initially
        if self.engine_busy_paths.is_empty() && now.duration_since(self.last_scan).as_secs() >= 30 {
            self.rescan_engines();
        }

        if self.engine_busy_paths.is_empty() {
            self.prev_time = now;
            return 0.0;
        }

        let mut total_busy_ns: u64 = 0;
        let mut read_count = 0;

        for path in &self.engine_busy_paths {
            if let Ok(s) = fs::read_to_string(path) {
                if let Ok(val) = s.trim().parse::<u64>() {
                    total_busy_ns += val;
                    read_count += 1;
                }
            }
        }

        if read_count == 0 {
            self.prev_time = now;
            return 0.0;
        }

        let gpu_pct = if let Some(prev) = self.prev_busy_ns {
            let delta_ns = total_busy_ns.saturating_sub(prev);
            let elapsed_ns = now.duration_since(self.prev_time).as_nanos() as u64;
            if elapsed_ns > 0 {
                let pct = (delta_ns as f64 / elapsed_ns as f64) * 100.0 / (read_count as f64);
                pct.clamp(0.0, 100.0)
            } else {
                0.0
            }
        } else {
            0.0
        };

        self.prev_busy_ns = Some(total_busy_ns);
        self.prev_time = now;

        gpu_pct
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proc_reading_self() {
        let sampler = ResourceSampler::new();
        let my_pid = std::process::id();

        let ticks = sampler.read_proc_cpu(my_pid).expect("read cpu ticks for self");
        let _ = ticks;

        let rss = sampler.read_proc_rss(my_pid).expect("read rss for self");
        assert!(rss > 0);
    }

    #[test]
    fn test_sample_cycle() {
        let mut sampler = ResourceSampler::new();
        let my_pid = std::process::id();

        let sample1 = sampler.sample("test_self", Some(my_pid));
        assert_eq!(sample1.app_id, "test_self");
        assert_eq!(sample1.pid, my_pid as i64);
        assert!(sample1.rss_kb > 0);

        let idle_sample = sampler.sample("away", None);
        assert_eq!(idle_sample.app_id, "away");
        assert_eq!(idle_sample.pid, 0);
        assert_eq!(idle_sample.cpu_pct, 0.0);
        assert_eq!(idle_sample.rss_kb, 0);
    }
}
