//! Defer source-wide ID allocation without losing reservations from failed fits.

use super::*;

#[derive(Debug)]
pub(super) struct Key {
    pub(super) time_ms: i64,
    pub(super) value: PropertyValue,
    pub(super) easing: PropertyKeyframeEasing,
}

#[derive(Default)]
pub(super) struct Builder {
    identity: Option<u64>,
    times: Vec<i64>,
}

impl Builder {
    // Called at each type's original identity-computation point, not at entry.
    pub(super) fn identify(
        &mut self,
        entry: &AnimationGraphEntry,
        code: &str,
    ) -> Result<(), BakeError> {
        self.identity = Some(conversion_identity_seed(
            &serde_json::to_vec(&entry.target)?,
            code.as_bytes(),
        ));
        Ok(())
    }

    pub(super) fn record(
        &mut self,
        time_ms: i64,
        value: PropertyValue,
        easing: PropertyKeyframeEasing,
    ) -> Key {
        self.times.push(time_ms);
        Key {
            time_ms,
            value,
            easing,
        }
    }

    // Reserve before propagating errors: Text can fail after allocating some IDs.
    pub(super) fn finish(
        self,
        result: Result<Vec<Key>, BakeError>,
        target: &PropertyTarget,
        used_ids: &mut BTreeSet<String>,
        budget: &mut Budget,
    ) -> Result<PropertyAnimator, BakeError> {
        let ids: Vec<_> = self
            .times
            .into_iter()
            .map(|time| {
                let identity = self.identity.expect("recorded key times have an identity");
                fx_schema::KeyframeId::new(converted_keyframe_id(identity, time, used_ids))
            })
            .collect();
        let keys = result?;
        debug_assert_eq!(keys.len(), ids.len());
        let keys = keys
            .into_iter()
            .zip(ids)
            .map(|(key, id)| {
                PropertyKeyframe::new(
                    id,
                    TimeOffset::from_millis(key.time_ms),
                    key.value,
                    key.easing,
                )
            })
            .collect();
        let track = PropertyKeyframeTrack::new(keys)?;
        track.validate_for_target(target)?;
        budget.keys = budget
            .keys
            .checked_add(track.keyframes().len())
            .ok_or(BakeError::Budget("key counter overflow"))?;
        Ok(PropertyAnimator::keyframes(track))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_fit_preserves_partial_reservations_and_collision_order() {
        let mut builder = Builder {
            identity: Some(42),
            times: Vec::new(),
        };
        builder.record(
            0,
            PropertyValue::String("first".into()),
            PropertyKeyframeEasing::Hold,
        );
        builder.record(
            10,
            PropertyValue::String("second".into()),
            PropertyKeyframeEasing::Hold,
        );
        let mut expected = BTreeSet::new();
        converted_keyframe_id(42, 0, &mut expected);
        converted_keyframe_id(42, 10, &mut expected);
        let mut actual = BTreeSet::new();
        let mut budget = Budget::default();
        // Failure never validates the target, so any scalar target is sufficient.
        let target = PropertyTarget::layer(LayerId::new(7), PropType::Opacity);
        assert!(matches!(
            builder.finish(
                Err(BakeError::Validation(20)),
                &target,
                &mut actual,
                &mut budget
            ),
            Err(BakeError::Validation(20))
        ));
        assert_eq!(actual, expected);
        assert_eq!(budget.keys, 0);
        assert_eq!(
            converted_keyframe_id(42, 0, &mut actual),
            converted_keyframe_id(42, 0, &mut expected)
        );
    }
}
