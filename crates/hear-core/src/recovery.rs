//! Non-critical historical filesystem recovery. The core accepts requests and
//! starts capture before these bounded sweeps; only the old export jobs are
//! marked during startup. Live work is excluded by its existing admission slot
//! or media lifetime guard, never by a timestamp heuristic.
use crate::{CoreState, archive, events, moments, resources};
use std::fs::{self, ReadDir};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn entries(path: &std::path::Path) -> Option<ReadDir> {
    if !fs::symlink_metadata(path).ok()?.file_type().is_dir() {
        return None;
    }
    fs::read_dir(path).ok()
}

pub fn run(shared: Arc<CoreState>, stop: Arc<AtomicBool>) {
    let mut next = Instant::now();
    let mut files: [Option<ReadDir>; 3] = [None, None, None];
    let mut after = String::new();
    let mut jobs_done = true;
    while !stop.load(Ordering::Acquire) {
        if files.iter().all(Option::is_none) && jobs_done && Instant::now() >= next {
            files = [
                entries(&shared.directory),
                entries(&shared.directory.join("export_tmp")),
                entries(&shared.directory.join("screenshots")),
            ];
            after.clear();
            jobs_done = false;
            next = Instant::now() + Duration::from_secs(1800);
        }
        for (kind, iterator) in files.iter_mut().enumerate() {
            let Some(iter) = iterator else { continue };
            // ASR preparation and exports already own these single-flight
            // permits for their entire lifetime, including temporary files.
            let work = match kind {
                0 => Some(resources::Work::Asr),
                1 => Some(resources::Work::Export),
                _ => None,
            };
            let permit = work.and_then(|w| shared.resources.try_admit(w));
            if work.is_some() && permit.is_none() {
                continue;
            }
            let mut done = false;
            for _ in 0..32 {
                if stop.load(Ordering::Acquire) {
                    return;
                }
                let Some(entry) = iter.next() else {
                    done = true;
                    break;
                };
                let Ok(entry) = entry else { continue };
                match kind {
                    0 => events::recover_asr_entry(&entry),
                    1 => {
                        let _ = archive::cleanup_spool_entry(&entry);
                    }
                    _ => {
                        let Ok(_gate) = shared.cleanup_gate.lock() else {
                            return;
                        };
                        let orphan = shared
                            .store
                            .lock()
                            .ok()
                            .and_then(|store| {
                                moments::orphan(&store.db, &entry.file_name().to_string_lossy())
                                    .ok()
                            })
                            .unwrap_or(false);
                        if orphan {
                            let _ = fs::remove_file(entry.path());
                        }
                    }
                }
            }
            if done {
                *iterator = None;
            }
        }
        if !jobs_done {
            let jobs = shared
                .store
                .lock()
                .ok()
                .and_then(|s| archive::recovery_jobs(&s.db, &after).ok());
            if let Some(jobs) = jobs {
                jobs_done = jobs.is_empty();
                for job in jobs {
                    if stop.load(Ordering::Acquire) {
                        return;
                    }
                    after = job.id.clone();
                    if archive::recover_job(&job) {
                        if let Ok(store) = shared.store.lock() {
                            let _ = archive::finish_recovery(&store.db, &job);
                        }
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
