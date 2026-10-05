#![cfg(feature = "ffmpeg-library")]

//! Saved Object Mask omission and edited export of a retained ordinary sibling.
//! The mask records are native; the two-track/media scaffold is supplementary.

use super::support::*;
use premiere_file::OmissionScope;
use serde_json::json;
use tesseract_file::TesseractFile;

#[test]
fn native_saved_object_mask_import_omits_only_masked_picture_and_exports_edited_sibling() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mask = include_str!("../fixtures/object_mask/opacity.xml");
    // Two overlapping occurrences of the same pinned media: the masked one
    // must disappear, while the unmasked one still covers the output timeline.
    let xml = one_second()
        .replace("<TrackItem ObjectRef=\"3\"/>", "<TrackItem ObjectRef=\"362\"/>")
        .replace("VideoClipTrackItem ObjectID=\"3\"", "VideoClipTrackItem ObjectID=\"362\"")
        .replace(
            "<Tracks><Track ObjectURef=\"track-1\"/></Tracks>",
            "<Tracks><Track ObjectURef=\"track-1\"/><Track ObjectURef=\"track-2\" Index=\"1\"/></Tracks>",
        )
        .replace(
            "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
            "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"665\"/></Components></ComponentChain></VideoComponentChain>",
        )
        .replace(
            "</PremiereData>",
            &format!("{mask}
<VideoClipTrack ObjectUID=\"track-2\"><ClipTrack><Track><ID>2</ID><Index>1</Index></Track><ClipItems><TrackItems><TrackItem ObjectRef=\"9\"/></TrackItems><Index>1</Index></ClipItems></ClipTrack></VideoClipTrack>
<VideoClipTrackItem ObjectID=\"9\"><ClipTrackItem><ComponentOwner><Components ObjectRef=\"10\"/></ComponentOwner><TrackItem><End>254016000000</End></TrackItem><SubClip ObjectRef=\"11\"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem>
<VideoComponentChain ObjectID=\"10\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>
<SubClip ObjectID=\"11\"><Clip ObjectRef=\"6\"/><Name>Retained sibling</Name></SubClip>
</PremiereData>"),
        );
    let source = fixture(root, &xml);
    let output = root.join("converted");
    let omissions = premiere_to_tesseract(&source, &output, None, false).unwrap();
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(omissions[0].scope, OmissionScope::Occurrence);
    assert_eq!(omissions[0].record, "VideoClipTrackItem:362");
    assert!(omissions[0]
        .reason
        .contains("Object Mask could not recover"));
    assert!(omissions[0]
        .reason
        .contains("owning masked occurrence is omitted"));

    let mut document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    // Import always appends the ordinary black canvas to the picture layers.
    assert_eq!(layers.len(), 2, "only the sibling and black canvas remain");
    assert!(layers
        .iter()
        .any(|layer| { layer["type"] == "Rect" && layer["name"] == "Premiere black canvas" }));
    let retained = layers
        .iter_mut()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    assert_eq!(
        crate::test_support::layer_range(retained),
        &json!({"start": 0, "duration": 1000})
    );
    assert_eq!(retained["source"]["assetId"], "premiere-video-1");
    assert!(retained["masks"].as_array().is_none_or(Vec::is_empty));
    assert!(retained["effects"].as_array().is_none_or(Vec::is_empty));
    retained["transform"]["opacity"] = json!(70.0);
    retained["transform"]["rotation"] = json!(15.0);
    let edited = archive(root, &document, &root.join("media/source.mp4"));
    let native = root.join("edited-native");
    let omissions = tesseract_to_premiere(&edited, &native, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");

    // Inspect emitted XML directly, not a round trip through our native reader.
    // This proves current sibling values were authored, not Object Mask export
    // or Adobe acceptance. No hidden native records/sidecar state are replayed.
    let xml = read_xml(&native.join("project.prproj"));
    let written = roxmltree::Document::parse(&xml).unwrap();
    let records = written.root_element();
    assert_eq!(
        records
            .children()
            .filter(|node| node.has_tag_name("VideoClipTrackItem"))
            .count(),
        1
    );
    assert!(!xml.contains("SubComponents"));
    assert!(!xml.contains("AEMask"));
    for (name, value) in [("Opacity", "70"), ("Rotation", "15")] {
        assert!(
            records.children().any(|node| {
                node.has_tag_name("VideoComponentParam")
                    && node
                        .children()
                        .any(|child| child.has_tag_name("Name") && child.text() == Some(name))
                    && node.children().any(|child| {
                        child.has_tag_name("StartKeyframe")
                            && child
                                .text()
                                .is_some_and(|wire| wire.split(',').nth(1) == Some(value))
                    })
            }),
            "missing edited {name} = {value}"
        );
    }
    assert_eq!(track_item_ticks(&written, "End"), [254_016_000_000]);
}
