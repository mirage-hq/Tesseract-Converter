//! Command-scoped progress; callbacks update a snapshot, not the output rate.

use std::{
    io::{self, Write},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use fx_conv::ConversionProgress;
use media_transcode::{Progress, ProgressStatus};
use serde_json::{json, Value};

const INTERVAL: Duration = Duration::from_secs(5);
const STALE_AFTER: Duration = Duration::from_secs(10);

#[derive(Clone)]
struct Snapshot {
    phase: &'static str,
    media: Option<Progress>,
    conversion: Option<ConversionMeasurement>,
    advanced_at: Instant,
}

#[derive(Clone)]
struct ConversionMeasurement {
    progress: ConversionProgress,
    started_at: Instant,
    initial_completed: usize,
}

impl Snapshot {
    fn update_conversion(&mut self, progress: ConversionProgress, now: Instant) {
        let reset = progress.started
            || self.conversion.as_ref().is_none_or(|old| {
                old.progress.phase != progress.phase
                    || old.progress.total != progress.total
                    || old.progress.unit != progress.unit
                    || progress.completed < old.progress.completed
            });
        let started_at = if reset {
            now
        } else {
            self.conversion.as_ref().map_or(now, |old| old.started_at)
        };
        if reset
            || self
                .conversion
                .as_ref()
                .is_some_and(|old| progress.completed > old.progress.completed)
        {
            self.advanced_at = now;
        }
        let initial_completed = if reset {
            progress.completed.unwrap_or(0)
        } else {
            self.conversion
                .as_ref()
                .map_or(0, |old| old.initial_completed)
        };
        self.phase = progress.phase;
        self.media = None;
        self.conversion = Some(ConversionMeasurement {
            progress,
            started_at,
            initial_completed,
        });
    }

    fn event(&self, command: &str, kind: &str, elapsed: Duration, now: Instant) -> Value {
        let encoding = self.phase == "transcode";
        let fresh = now.saturating_duration_since(self.advanced_at) <= STALE_AFTER;
        let finite = |value: Option<f64>| value.filter(|n| n.is_finite() && *n >= 0.0);
        let mut event = json!({
            "schemaVersion": 1, "type": kind, "command": command,
            "phase": self.phase, "elapsedSeconds": elapsed.as_secs_f64(),
            "completed": self.media.as_ref().and_then(|p| finite(p.processed_seconds)),
            "total": self.media.as_ref().and_then(|p| finite(p.total_seconds)),
            "unit": if self.media.is_some() { Some("seconds") } else { None },
            "ratePerSecond": if encoding && fresh { self.media.as_ref().and_then(|p| finite(p.speed)) } else { None },
            "etaSeconds": if encoding && fresh { self.media.as_ref().and_then(|p| finite(p.eta_seconds)) } else { None },
        });
        if let Some(measurement) = &self.conversion {
            let progress = measurement.progress;
            let counts = progress
                .completed
                .zip(progress.total)
                .filter(|(completed, total)| completed <= total);
            let seconds = self
                .advanced_at
                .saturating_duration_since(measurement.started_at)
                .as_secs_f64();
            let rate = counts
                .filter(|(completed, total)| {
                    fresh
                        && *completed > measurement.initial_completed
                        && *total > 0
                        && seconds > 0.0
                })
                .and_then(|(completed, _)| {
                    finite(Some(
                        (completed - measurement.initial_completed) as f64 / seconds,
                    ))
                })
                .filter(|rate| *rate > 0.0);
            let eta = counts.zip(rate).and_then(|((completed, total), rate)| {
                finite(Some((total - completed) as f64 / rate))
            });
            event["completed"] = json!(counts.map(|(completed, _)| completed));
            event["total"] = json!(counts.map(|(_, total)| total));
            event["unit"] = json!(progress.unit);
            event["percentage"] = json!(counts
                .filter(|(_, total)| *total > 0)
                .map(|(completed, total)| 100.0 * completed as f64 / total as f64));
            event["ratePerSecond"] = json!(rate);
            event["etaSeconds"] = json!(eta);
            event["etaScope"] = json!("phase");
        }
        if let Some(media) = &self.media {
            // Preserve the original transcode progress keys for existing consumers.
            event["source"] = json!(media.source.to_string_lossy());
            event["processed_seconds"] = event["completed"].clone();
            event["total_seconds"] = event["total"].clone();
            event["speed"] = event["ratePerSecond"].clone();
            event["eta_seconds"] = event["etaSeconds"].clone();
            event["status"] = if kind == "complete" {
                json!("complete")
            } else {
                json!(media.status)
            };
        }
        event
    }
}

struct State {
    snapshot: Snapshot,
    stop: bool,
}

type Shared = Arc<(Mutex<State>, Condvar)>;

pub(super) struct Reporter {
    command: &'static str,
    json: bool,
    started: Instant,
    shared: Shared,
    worker: Option<JoinHandle<()>>,
}

impl Reporter {
    pub(super) fn start(
        command: &'static str,
        phase: &'static str,
        json: bool,
    ) -> io::Result<Self> {
        Self::with_writer(command, phase, json, INTERVAL, io::stderr())
    }

    fn with_writer<W: Write + Send + 'static>(
        command: &'static str,
        phase: &'static str,
        json_mode: bool,
        interval: Duration,
        mut writer: W,
    ) -> io::Result<Self> {
        let started = Instant::now();
        let shared = Arc::new((
            Mutex::new(State {
                snapshot: Snapshot {
                    phase,
                    media: None,
                    conversion: None,
                    advanced_at: started,
                },
                stop: false,
            }),
            Condvar::new(),
        ));
        let worker_state = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("conversion-progress".into())
            .spawn(move || {
                let (lock, wake) = &*worker_state;
                loop {
                    let state = lock.lock().unwrap_or_else(|e| e.into_inner());
                    let (state, _) = wake
                        .wait_timeout_while(state, interval, |state| !state.stop)
                        .unwrap_or_else(|e| e.into_inner());
                    if state.stop {
                        break;
                    }
                    let snapshot = state.snapshot.clone();
                    drop(state);
                    let event =
                        snapshot.event(command, "progress", started.elapsed(), Instant::now());
                    let line = if json_mode {
                        event.to_string()
                    } else {
                        human_line(&event)
                    };
                    // A broken diagnostic pipe must not panic or kill a valid export.
                    if writeln!(writer, "{line}")
                        .and_then(|()| writer.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            command,
            json: json_mode,
            started,
            shared,
            worker: Some(worker),
        })
    }

    pub(super) fn update_conversion(&self, progress: ConversionProgress) {
        self.shared
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .snapshot
            .update_conversion(progress, Instant::now());
    }

    pub(super) fn update_media(&self, progress: &Progress) {
        // Only the caller's successful return establishes completion.
        if progress.status == ProgressStatus::Complete {
            return;
        }
        let mut state = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        let increased = progress.processed_seconds.is_some_and(|value| {
            state
                .snapshot
                .media
                .as_ref()
                .and_then(|p| p.processed_seconds)
                .is_none_or(|old| value > old)
        });
        if increased {
            state.snapshot.advanced_at = Instant::now();
        }
        state.snapshot.phase = match progress.status {
            ProgressStatus::Unknown => "preparing",
            ProgressStatus::Encoding => "transcode",
            ProgressStatus::Finishing => "finalizing",
            ProgressStatus::Complete => return,
        };
        state.snapshot.media = Some(progress.clone());
    }

    pub(super) fn finish(&mut self, success: bool) {
        self.stop();
        if success && self.json {
            let snapshot = self
                .shared
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .snapshot
                .clone();
            let mut event = snapshot.event(
                self.command,
                "complete",
                self.started.elapsed(),
                Instant::now(),
            );
            event["phase"] = json!("complete");
            if snapshot.conversion.is_some() {
                // The last phase's counts must not become whole-command counts.
                for field in ["completed", "total", "unit", "percentage", "etaScope"] {
                    event[field] = Value::Null;
                }
            }
            event["etaSeconds"] = Value::Null;
            event["ratePerSecond"] = Value::Null;
            if snapshot.media.is_some() {
                event["speed"] = Value::Null;
                event["eta_seconds"] = Value::Null;
            }
            write_event(&event);
        }
    }

    fn stop(&mut self) {
        {
            let mut state = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
            state.stop = true;
            self.shared.1.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for Reporter {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(super) fn write_event(event: &Value) {
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "{event}").and_then(|()| stderr.flush());
}

pub(super) fn write_error(command: &str, error: &anyhow::Error) {
    write_event(&json!({
        "schemaVersion": 1, "type": "error", "command": command, "phase": "failed",
        "message": format!("{error:#}"),
        "causes": error.chain().map(ToString::to_string).collect::<Vec<_>>(),
    }));
}

fn human_line(event: &Value) -> String {
    let command = event["command"].as_str().unwrap_or("conversion");
    let phase = match event["phase"].as_str() {
        Some("transcode") => "encoding",
        Some(phase) => phase,
        None => "working",
    };
    let elapsed = event["elapsedSeconds"].as_f64().unwrap_or(0.0);
    let count = match (event["completed"].as_f64(), event["total"].as_f64()) {
        (Some(done), Some(total)) if total > 0.0 => {
            let percent = (100.0 * done / total).clamp(0.0, 100.0);
            match event["unit"].as_str() {
                Some("seconds") => format!(" {percent:.1}% ({done:.1}/{total:.1}s)"),
                unit => format!(
                    " {percent:.1}% ({done:.0}/{total:.0} {})",
                    unit.unwrap_or("items")
                ),
            }
        }
        _ => String::new(),
    };
    let eta = event["etaSeconds"].as_f64().map_or_else(
        || {
            if command != "convert" {
                "unknown".to_owned()
            } else if event["total"].as_u64().is_some_and(|n| n > 0) && event["completed"] == 0 {
                "estimating".to_owned()
            } else {
                "unavailable".to_owned()
            }
        },
        |seconds| {
            if command == "convert" {
                format!("~{seconds:.0}s")
            } else {
                format!("{seconds:.0}s")
            }
        },
    );
    let label = if command == "convert" {
        "phase ETA"
    } else {
        "ETA"
    };
    format!("{command}: {phase}{count} — elapsed {elapsed:.0}s, {label} {eta}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    struct Lines(mpsc::Sender<Vec<u8>>);
    impl Write for Lines {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .send(bytes.to_vec())
                .map_err(|_| io::ErrorKind::BrokenPipe)?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn conversion_snapshot(now: Instant) -> Snapshot {
        Snapshot {
            phase: "reading",
            media: None,
            conversion: None,
            advanced_at: now,
        }
    }

    fn tracks(completed: usize, total: usize, started: bool) -> ConversionProgress {
        ConversionProgress {
            phase: "baking-scripts",
            completed: Some(completed),
            total: Some(total),
            unit: Some("tracks"),
            started,
        }
    }

    #[test]
    fn conversion_percentage_and_eta_use_only_measured_phase_work() {
        let now = Instant::now();
        let mut snapshot = conversion_snapshot(now);
        snapshot.update_conversion(tracks(0, 100, true), now);
        let initial = snapshot.event("convert", "progress", Duration::from_secs(30), now);
        assert_eq!(initial["percentage"], 0.0);
        assert!(initial["etaSeconds"].is_null());
        assert!(human_line(&initial).contains("phase ETA estimating"));
        snapshot.update_conversion(tracks(25, 100, false), now + Duration::from_secs(5));
        let measured = snapshot.event(
            "convert",
            "progress",
            Duration::from_secs(35),
            now + Duration::from_secs(5),
        );
        assert_eq!(measured["percentage"], 25.0);
        assert_eq!(measured["ratePerSecond"], 5.0);
        assert_eq!(measured["etaSeconds"], 15.0);
        assert_eq!(measured["etaScope"], "phase");
        assert!(human_line(&measured).contains("25.0% (25/100 tracks)"));
        assert!(human_line(&measured).contains("phase ETA ~15s"));
        // A heartbeat or duplicate callback cannot invent progress or count down ETA.
        snapshot.update_conversion(tracks(25, 100, false), now + Duration::from_secs(7));
        let heartbeat = snapshot.event(
            "convert",
            "progress",
            Duration::from_secs(37),
            now + Duration::from_secs(7),
        );
        assert_eq!(heartbeat["etaSeconds"], 15.0);
        let stalled = snapshot.event(
            "convert",
            "progress",
            Duration::from_secs(60),
            now + Duration::from_secs(30),
        );
        assert_eq!(stalled["percentage"], 25.0);
        assert!(stalled["etaSeconds"].is_null());
        assert!(stalled["ratePerSecond"].is_null());
    }

    #[test]
    fn conversion_phase_transitions_reset_counts_rate_and_eta() {
        let now = Instant::now();
        let mut snapshot = conversion_snapshot(now);
        snapshot.update_conversion(tracks(0, 4, true), now);
        snapshot.update_conversion(tracks(4, 4, false), now + Duration::from_secs(2));
        let finished = snapshot.event(
            "convert",
            "progress",
            Duration::from_secs(2),
            now + Duration::from_secs(2),
        );
        assert_eq!(finished["percentage"], 100.0);
        assert_eq!(finished["etaSeconds"], 0.0);
        // Another hybrid scope can have exactly the same name and denominator.
        snapshot.update_conversion(tracks(0, 4, true), now + Duration::from_secs(3));
        let restarted = snapshot.event(
            "convert",
            "progress",
            Duration::from_secs(3),
            now + Duration::from_secs(3),
        );
        assert_eq!(restarted["percentage"], 0.0);
        assert!(restarted["etaSeconds"].is_null());
        snapshot.update_conversion(
            ConversionProgress {
                phase: "publishing",
                completed: None,
                total: None,
                unit: None,
                started: true,
            },
            now + Duration::from_secs(4),
        );
        let publishing = snapshot.event(
            "convert",
            "progress",
            Duration::from_secs(4),
            now + Duration::from_secs(4),
        );
        for field in [
            "completed",
            "total",
            "percentage",
            "ratePerSecond",
            "etaSeconds",
        ] {
            assert!(publishing[field].is_null(), "{field}");
        }
    }

    #[test]
    fn zero_or_invalid_counts_never_produce_nonfinite_percent_or_eta() {
        let now = Instant::now();
        for (done, total) in [(0, 0), (3, 2)] {
            let mut snapshot = conversion_snapshot(now);
            snapshot.update_conversion(tracks(0, total, true), now);
            snapshot.update_conversion(tracks(done, total, false), now + Duration::from_secs(1));
            let event = snapshot.event(
                "convert",
                "progress",
                Duration::from_secs(1),
                now + Duration::from_secs(1),
            );
            assert!(event["percentage"].is_null());
            assert!(event["etaSeconds"].is_null());
            assert!(event["ratePerSecond"].is_null());
        }
    }

    #[test]
    fn heartbeat_arrives_without_callbacks_and_stops_promptly() {
        let (tx, rx) = mpsc::channel();
        let mut reporter = Reporter::with_writer(
            "convert",
            "checking",
            true,
            Duration::from_millis(20),
            Lines(tx),
        )
        .unwrap();
        let mut bytes = Vec::new();
        while bytes.iter().filter(|b| **b == b'\n').count() < 2 {
            bytes.extend(rx.recv_timeout(Duration::from_secs(2)).unwrap());
        }
        for line in String::from_utf8(bytes).unwrap().lines() {
            let event: Value = serde_json::from_str(line).unwrap();
            assert_eq!(event["type"], "progress");
            assert_eq!(event["phase"], "checking");
            assert!(event["completed"].is_null());
            assert!(event["etaSeconds"].is_null());
        }
        reporter.finish(false);
        while rx.try_recv().is_ok() {}
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn measured_eta_expires_without_fabricating_progress() {
        let now = Instant::now();
        let snapshot = Snapshot {
            phase: "transcode",
            advanced_at: now,
            conversion: None,
            media: Some(Progress {
                source: "input.mov".into(),
                status: ProgressStatus::Encoding,
                processed_seconds: Some(8.0),
                total_seconds: Some(32.0),
                speed: Some(2.0),
                eta_seconds: Some(12.0),
            }),
        };
        let event = snapshot.event("transcode", "progress", Duration::from_secs(4), now);
        assert_eq!(event["etaSeconds"], 12.0);
        assert_eq!(event["processed_seconds"], 8.0);
        assert!(human_line(&event).contains("25.0%"));
        let stale = snapshot.event(
            "transcode",
            "progress",
            Duration::from_secs(20),
            now + Duration::from_secs(20),
        );
        assert_eq!(stale["completed"], 8.0);
        assert!(stale["etaSeconds"].is_null());
    }

    #[test]
    fn callback_updates_do_not_emit_per_frame_lines() {
        let (tx, rx) = mpsc::channel();
        let mut reporter = Reporter::with_writer(
            "transcode",
            "preparing",
            false,
            Duration::from_secs(5),
            Lines(tx),
        )
        .unwrap();
        for frame in 0..1_000 {
            reporter.update_media(&Progress {
                source: "clip.mov".into(),
                status: ProgressStatus::Encoding,
                processed_seconds: Some(f64::from(frame)),
                total_seconds: Some(1000.0),
                speed: None,
                eta_seconds: None,
            });
        }
        reporter.finish(false);
        assert!(rx.try_recv().is_err());
    }
}
