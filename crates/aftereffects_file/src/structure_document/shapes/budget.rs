//! Checked serialized-size accounting for expanded shape layers and animators.
use std::io::{self, Write};

pub(super) const EXHAUSTED: &str =
    "shape output serialization failed or its byte count overflowed; affected contribution omitted";

#[derive(Default)]
pub(in crate::structure_document) struct OutputBudget {
    used: usize,
    #[cfg(test)]
    limit: Option<usize>,
}

impl OutputBudget {
    /// Count without another serialized copy or a fixed project-size quota.
    /// Failed serialization leaves the accounting unchanged.
    pub(in crate::structure_document) fn reserve<T: serde::Serialize + ?Sized>(
        &mut self,
        value: &T,
    ) -> bool {
        let mut writer = Counter {
            used: self.used,
            #[cfg(test)]
            limit: self.limit,
        };
        if serde_json::to_writer(&mut writer, value).is_err() {
            return false;
        }
        self.used = writer.used;
        true
    }

    pub(in crate::structure_document) fn checkpoint(&self) -> usize {
        self.used
    }

    pub(in crate::structure_document) fn restore(&mut self, checkpoint: usize) {
        self.used = checkpoint;
    }

    #[cfg(test)]
    pub(super) fn remaining(&self) -> usize {
        self.limit
            .expect("remaining is used only with a test allowance")
            - self.used
    }

    #[cfg(test)]
    pub(in crate::structure_document) fn with_limit(limit: usize) -> Self {
        Self {
            used: 0,
            limit: Some(limit),
        }
    }
}

struct Counter {
    used: usize,
    #[cfg(test)]
    limit: Option<usize>,
}

impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let used = self
            .used
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("shape output byte count overflow"))?;
        #[cfg(test)]
        if self.limit.is_some_and(|limit| used > limit) {
            return Err(io::Error::other("test shape expansion allowance exhausted"));
        }
        self.used = used;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shape_accounting_has_no_aggregate_size_quota() {
        let text = "x".repeat(4096);
        let items = vec![text.as_str(); 16_385];
        let mut budget = OutputBudget::default();
        assert!(budget.reserve(&items));
        let checkpoint = budget.checkpoint();
        assert!(budget.reserve(&true));
        budget.restore(checkpoint);
        assert_eq!(budget.checkpoint(), checkpoint);
    }

    #[test]
    fn reservation_counts_escaped_bytes_and_is_atomic() {
        let mut budget = OutputBudget::with_limit(8);
        assert!(!budget.reserve(&"too long for this budget"));
        assert_eq!(budget.remaining(), 8);
        assert!(budget.reserve(&"\n")); // JSON quotes plus escaped newline.
        assert_eq!(budget.remaining(), 4);
        assert!(budget.reserve(&true));
        assert_eq!(budget.remaining(), 0);
        assert!(!budget.reserve(&0));
    }
}
