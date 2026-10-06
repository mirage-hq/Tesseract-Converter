pub(super) use crate::test_support::{write_archive, write_prproj, TICKS};
pub(super) use premiere_file::{premiere_to_tesseract, tesseract_to_premiere};
use serde_json::Value;
use std::{fs, io::Read, path::Path};
pub(super) const XML: &str = include_str!("../fixtures/one-clip.xml");
pub(super) const MEDIA: &[u8] = include_bytes!("../fixtures/video-30fps.mp4");
#[cfg(feature = "ffmpeg-library")]
pub(super) const MOV: &[u8] = include_bytes!("../fixtures/video-30fps.mov");

pub(super) fn one_second() -> String {
    XML.replace("1270080000000", "254016000000")
        .replace("2540160000000", "254016000000")
}

#[cfg(feature = "ffmpeg-library")]
pub(super) fn two_timelines() -> String {
    let first = one_second();
    let second = first
        .replace("ObjectID=\"", "ObjectID=\"2")
        .replace("ObjectRef=\"", "ObjectRef=\"2")
        .replace("ObjectUID=\"", "ObjectUID=\"z-second-")
        .replace("ObjectURef=\"", "ObjectURef=\"z-second-");
    first.replace(
        "</PremiereData>",
        &second.replace("<PremiereData Version=\"3\">", ""),
    )
}

#[cfg(feature = "ffmpeg-library")]
pub(super) fn build_tesseract_file(
    input: &Path,
    output: &Path,
    sequence: Option<&str>,
) -> anyhow::Result<()> {
    anyhow::ensure!(!output.exists(), "test archive already exists");
    let temp = tempfile::tempdir()?;
    let batch = temp.path().join("batch");
    premiere_to_tesseract(input, &batch, sequence, false)?;
    let projects = project_files(&batch);
    anyhow::ensure!(projects.len() == 1, "expected one test timeline");
    fs::copy(&projects[0], output)?;
    Ok(())
}

/// The document's root video layers, in layer order.
#[cfg(feature = "ffmpeg-library")]
pub(super) fn video_layers(document: &Value) -> Vec<&Value> {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .collect()
}

pub(super) fn project_files(directory: &Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(paths
        .iter()
        .all(|path| path.extension().is_some_and(|ext| ext == "tsrct")));
    paths.sort();
    paths
}

pub(super) fn first_project(directory: &Path) -> std::path::PathBuf {
    project_files(directory).into_iter().next().unwrap()
}

/// The GUID of the one root sequence of an exported project, for a reimport
/// that must select it: exported groups are nested sequences, which count as
/// selectable targets.
pub(super) fn exported_root_sequence(native: &Path) -> String {
    let (project, _) = premiere_file::PrProjectFile::load(native).unwrap();
    let roots: Vec<_> = project.sequences().collect();
    assert_eq!(roots.len(), 1, "expected one root sequence");
    roots[0].id().unwrap().to_owned()
}

pub(super) fn fixture(directory: &Path, xml: &str) -> std::path::PathBuf {
    fs::create_dir_all(directory.join("media")).unwrap();
    fs::write(directory.join("media/source.mp4"), MEDIA).unwrap();
    let native = directory.join("project.prproj");
    write_prproj(&native, xml);
    native
}

#[cfg(feature = "ffmpeg-library")]
pub(super) fn track_item_ticks(document: &roxmltree::Document<'_>, tag: &str) -> Vec<i64> {
    document
        .descendants()
        .filter(|node| {
            node.has_tag_name("TrackItem")
                && node
                    .parent()
                    .is_some_and(|parent| parent.has_tag_name("ClipTrackItem"))
        })
        .map(|node| {
            node.children()
                .find(|child| child.has_tag_name(tag))
                .map_or(0, |child| child.text().unwrap().parse().unwrap())
        })
        .collect()
}

/// `xml` with `edit` applied to the record that opens with `open`, up to the
/// first `close` after it.
pub(super) fn edit_record(
    xml: &mut String,
    open: &str,
    close: &str,
    edit: impl Fn(&str) -> String,
) {
    let start = xml.find(open).unwrap();
    let end = start + xml[start..].find(close).unwrap() + close.len();
    let record = edit(&xml[start..end]);
    xml.replace_range(start..end, &record);
}

pub(super) fn read_xml(path: &Path) -> String {
    let mut xml = String::new();
    flate2::read::GzDecoder::new(fs::File::open(path).unwrap())
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

#[cfg(feature = "ffmpeg-library")]
pub(super) fn document(dir: &Path) -> Value {
    fs::write(dir.join("source.mp4"), MEDIA).unwrap();
    crate::test_support::editable_document()
}

pub(super) fn archive(dir: &Path, doc: &Value, media: &Path) -> std::path::PathBuf {
    let path = dir.join("edited.tsrct");
    write_archive(&path, doc, media);
    path
}
