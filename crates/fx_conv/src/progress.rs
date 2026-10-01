//! Optional, command-scoped observations of real conversion work.

/// A phase-local measurement, not a promise of successful conversion or fidelity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversionProgress {
    /// Stable name for the current operation, independent of its native source.
    pub phase: &'static str,
    /// Work units processed, including units omitted with diagnostics.
    pub completed: Option<usize>,
    /// Fixed phase denominator, or absent when work cannot be counted.
    pub total: Option<usize>,
    /// Human-readable units such as layers, clips, or tracks.
    pub unit: Option<&'static str>,
    /// Resets timing even when consecutive scopes use the same phase name.
    pub started: bool,
}

/// A borrowed observer; absent observers preserve existing library behavior.
/// Callbacks may run on a scoped worker and should return promptly without panicking.
#[derive(Clone, Copy, Default)]
pub struct Progress<'a> {
    callback: Option<&'a (dyn Fn(ConversionProgress) + Sync)>,
}

impl<'a> Progress<'a> {
    /// Observe this conversion without installing process-global state.
    pub fn new(callback: &'a (dyn Fn(ConversionProgress) + Sync)) -> Self {
        Self {
            callback: Some(callback),
        }
    }

    /// Enter work whose denominator is not available, clearing prior measurements.
    pub fn stage(self, phase: &'static str) {
        self.emit(ConversionProgress {
            phase,
            completed: None,
            total: None,
            unit: None,
            started: true,
        });
    }

    /// Start a measurable phase. Count processed units, including diagnosed omissions.
    pub fn phase(self, phase: &'static str, unit: &'static str, total: usize) -> ProgressPhase<'a> {
        let measurement = ConversionProgress {
            phase,
            completed: Some(0),
            total: Some(total),
            unit: Some(unit),
            started: true,
        };
        self.emit(measurement);
        ProgressPhase {
            observer: self,
            measurement,
        }
    }

    fn emit(self, measurement: ConversionProgress) {
        if let Some(callback) = self.callback {
            callback(measurement);
        }
    }
}

/// A phase's fixed denominator and callback, reusable across its sequential work loop.
#[derive(Clone, Copy)]
pub struct ProgressPhase<'a> {
    observer: Progress<'a>,
    measurement: ConversionProgress,
}

impl ProgressPhase<'_> {
    /// Report the number of completed units; never counts a unit before it is processed.
    pub fn update(self, completed: usize) {
        self.observer.emit(ConversionProgress {
            completed: Some(completed),
            started: false,
            ..self.measurement
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn phases_reset_and_observe_actual_counts() {
        let events = Mutex::new(Vec::new());
        let callback = |event| events.lock().unwrap().push(event);
        let progress = Progress::new(&callback);
        progress.stage("reading");
        let phase = progress.phase("layers", "layers", 2);
        phase.update(1);
        phase.update(2);
        progress.phase("layers", "layers", 3);
        progress.stage("writing");
        let events = events.into_inner().unwrap();
        assert_eq!(events.len(), 6);
        assert_eq!(events[0].total, None);
        assert!(events[1].started);
        assert_eq!(events[2].completed, Some(1));
        assert!(!events[2].started);
        assert_eq!(events[3].completed, Some(2));
        assert_eq!(events[4].completed, Some(0));
        assert!(events[4].started);
        assert_eq!(events[5].total, None);
    }
}
