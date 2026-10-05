//! Invalid decoder configuration must fail before a picture-media loss exists.
use super::*;
use crate::media_metadata::{inspect_export_h264, read_export_movie_metadata, validate_h264};
use h264_reader::avcc::AvcDecoderConfigurationRecord;
use std::io::Cursor;

#[test]
fn export_media_malformed_avc_version_is_fatal_before_picture_loss() {
    assert_malformed_configuration(&["version"]);
}

#[test]
fn export_media_malformed_avc_pps_framing_is_fatal_before_picture_loss() {
    assert_malformed_configuration(&["oversized PPS", "missing PPS", "empty PPS", "empty SPS"]);
}

#[test]
fn export_media_malformed_avc_pps_content_is_fatal_before_picture_loss() {
    assert_malformed_configuration(&["wrong NAL", "invalid PPS bits"]);
}

#[test]
fn export_media_malformed_avc_extension_is_fatal_before_picture_loss() {
    assert_malformed_configuration(&["extension depth", "extension count"]);
}

#[test]
fn export_media_zero_sps_count_is_rejected_by_h264_readers() {
    let bytes = srgb_video();
    let metadata = read_export_movie_metadata(Cursor::new(&bytes), bytes.len() as u64).unwrap();
    let description = metadata
        .tracks
        .into_iter()
        .find(|track| track.handler == *b"vide")
        .unwrap()
        .sample_description
        .unwrap();
    let (_, range) = description
        .children
        .iter()
        .find(|(kind, _)| kind == b"avcC")
        .unwrap();
    let valid = &bytes[usize::try_from(range.start).unwrap()..usize::try_from(range.end).unwrap()];
    let configuration = AvcDecoderConfigurationRecord::try_from(valid).unwrap();
    let mut pps_start = 6;
    for sps in configuration.sequence_parameter_sets() {
        pps_start += 2 + sps.unwrap().len();
    }
    // Remove the SPS records, retaining complete PPS framing. Direct calls
    // avoid an earlier archive or demux rejection masking the zero-count path.
    let mut record = valid[..6].to_vec();
    record[5] &= 0xe0;
    record.extend_from_slice(&valid[pps_start..]);
    let configuration = AvcDecoderConfigurationRecord::try_from(record.as_slice()).unwrap();
    assert_eq!(configuration.num_of_sequence_parameter_sets(), 0);
    assert_eq!(configuration.picture_parameter_sets().count(), 1);
    for error in [
        validate_h264(&description, &record).unwrap_err(),
        inspect_export_h264(&description, &record).unwrap_err(),
    ] {
        assert!(
            matches!(error, crate::error::BuildError::Unsupported(ref reason)
            if reason == "missing H.264 sequence parameter sets")
        );
    }
}

fn assert_malformed_configuration(defects: &[&str]) {
    let valid = srgb_video();
    let start = valid.windows(4).position(|x| x == b"avcC").unwrap() + 4;
    let mut pps_count = start + 6;
    for _ in 0..valid[start + 5] & 0x1f {
        let length = usize::from(u16::from_be_bytes([valid[pps_count], valid[pps_count + 1]]));
        pps_count += 2 + length;
    }
    assert_eq!(valid[pps_count], 1);
    let pps_length = pps_count + 1;
    let pps = pps_length + 2;
    let length = usize::from(u16::from_be_bytes([
        valid[pps_length],
        valid[pps_length + 1],
    ]));
    for &defect in defects {
        let root = tempfile::tempdir().unwrap();
        let mut bytes = valid.clone();
        match defect {
            "version" => bytes[start] = 0,
            "oversized PPS" => {
                bytes[pps_length..pps_length + 2].copy_from_slice(&u16::MAX.to_be_bytes())
            }
            "missing PPS" => bytes[pps_count] = 0,
            "empty PPS" => bytes[pps_length..pps_length + 2].fill(0),
            "empty SPS" => bytes[start + 6..start + 8].fill(0),
            "extension depth" => bytes[pps + length + 1] = 0xfa,
            "extension count" => bytes[pps + length + 3] = 1,
            "wrong NAL" => bytes[pps] = 0x67,
            "invalid PPS bits" => bytes[pps + 1..pps + length].fill(0),
            _ => unreachable!(),
        }
        let file = prepared_archive(root.path(), &bytes);
        let result = Premiere.prepare_export(&file, file.project(), &Default::default());
        assert!(result.is_err(), "malformed {defect} became export facts");
    }
}
