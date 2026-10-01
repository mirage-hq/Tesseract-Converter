//! Omission reporting preserves all distinct diagnostics, including beyond former quotas.

use crate::{push_omission, Omission, OmissionKind, OmissionScope};

fn item(index: usize) -> Omission {
    Omission {
        scope: OmissionScope::Occurrence,
        kind: OmissionKind::Omitted,
        record: format!("VideoClipTrackItem:{index}"),
        reason: "not converted".into(),
    }
}

#[test]
fn reports_past_former_entry_and_text_quotas_keep_every_distinct_omission() {
    let mut omissions = Vec::new();
    for index in 0..1025 {
        let mut report = item(index);
        report.reason = "x".repeat(1024);
        push_omission(&mut omissions, report);
    }
    assert_eq!(omissions.len(), 1025);
    assert_eq!(omissions.last().unwrap().record, item(1024).record);
    assert!(
        omissions
            .iter()
            .map(|report| report.reason.len())
            .sum::<usize>()
            > 1 << 20
    );
    let duplicate = omissions[0].clone();
    push_omission(&mut omissions, duplicate);
    assert_eq!(omissions.len(), 1025);
}
