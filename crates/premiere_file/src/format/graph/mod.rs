//! Native record graph and typed reference traversal.

mod decode;
mod sequences;

use crate::format::{invalid, FormatError, Result};
use crate::schema::{native::Reference, records};
use roxmltree::{Document, Node, NodeId, ParsingOptions};
use serde::de::DeserializeOwned;
use std::{collections::HashMap, fmt};

pub(crate) use sequences::cyclic_sequences;
pub(crate) use sequences::nested_sequence;
pub(crate) use sequences::sequences;

/// Dispatches a selected graph node to its shared native record type.
pub(crate) trait InputRecord: DeserializeOwned {
    const TAG: &'static str;
}

const PROPERTIES: &str = "Properties";
const TRACK_GROUP: &str = "TrackGroup";
const TRACKS: &str = "Tracks";
const TRACK_ITEMS: &str = "TrackItems";
const TRACK_ITEM: &str = "TrackItem";
const CLIP_TRACK: &str = "ClipTrack";
const CLIP_ITEMS: &str = "ClipItems";
const INDEX: &str = "Index";
const BINARY_HASH: &str = "BinaryHash";

// Premiere 26 accepts and preserves these four unresolved default slots in a
// from-scratch project, then supplies the effective compile settings
// internally. This is the only admitted dangling reference shape; every
// semantic graph edge remains strict. The writer's allocation order produces
// exactly these IDs (writer/graph.rs).
const REGENERATED_PROJECT_DEFAULTS: [(&str, &str); 4] = [
    ("VideoSettings", "12"),
    ("AudioSettings", "13"),
    ("VideoCompileSettings", "14"),
    ("AudioCompileSettings", "15"),
];

/// One XML element for lenient topology reads, without record decoding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Element<'a> {
    node: Node<'a, 'a>,
}

impl<'a> Element<'a> {
    pub(crate) fn tag(&self) -> &'a str {
        self.node.tag_name().name()
    }

    pub(crate) fn attribute(self, name: &str) -> Option<&'a str> {
        self.node.attribute(name)
    }

    pub(crate) fn attributes(self) -> impl Iterator<Item = &'a str> + 'a {
        self.node.attributes().map(|attribute| attribute.name())
    }

    /// The first element child named `tag`.
    pub(crate) fn child(self, tag: &str) -> Option<Self> {
        self.children().find(|child| child.tag() == tag)
    }

    /// Element children in document order.
    pub(crate) fn children(self) -> impl Iterator<Item = Self> + 'a {
        self.node
            .children()
            .filter(Node::is_element)
            .map(|node| Self { node })
    }

    /// Whether every child node is text. A strict static parameter reader must
    /// reject comments/PIs that can hide additional text from `text()`.
    pub(crate) fn is_text_only(self) -> bool {
        self.node.children().all(|node| node.is_text())
    }

    /// The element's text content, with entities and CDATA resolved.
    pub(crate) fn text(self) -> Option<&'a str> {
        self.node.text()
    }

    /// The `ObjectRef`/`ObjectURef` edge this element carries.
    pub(crate) fn reference(self) -> Reference {
        Reference {
            id: self.attribute(records::OBJECT_REF).map(str::to_owned),
            uid: self.attribute(records::OBJECT_UREF).map(str::to_owned),
            index: self.attribute(INDEX).map(str::to_owned),
        }
    }
}

/// A root element returned by graph lookup or enumeration, eligible for decoding.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Record<'a> {
    element: Element<'a>,
}

impl<'a> Record<'a> {
    pub(crate) fn element(self) -> Element<'a> {
        self.element
    }

    pub(crate) fn tag(&self) -> &'a str {
        self.element.tag()
    }

    /// The `Tag:ObjectID` (or `Tag:ObjectUID`) error context.
    pub(crate) fn identity(&self) -> String {
        identity(self.element.node)
    }

    fn xml(&self) -> &'a str {
        let node = self.element.node;
        &node.document().input_text()[node.range()]
    }

    /// Track references of a track-group record (`TrackGroup/Tracks/*`).
    pub(crate) fn track_references(self) -> Result<Vec<Reference>> {
        let track_group = self
            .element
            .child(TRACK_GROUP)
            .ok_or_else(|| invalid(format!("{}: missing {TRACK_GROUP}", self.identity())))?;
        Ok(track_group
            .child(TRACKS)
            .into_iter()
            .flat_map(Element::children)
            .map(Element::reference)
            .collect())
    }

    pub(crate) fn track_item_references(self) -> Vec<Reference> {
        self.element
            .node
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == TRACK_ITEMS)
            .flat_map(|items| items.children().filter(Node::is_element))
            .map(|node| Element { node }.reference())
            .collect()
    }

    pub(crate) fn has_referenced_track_items(self) -> bool {
        self.element.node.descendants().any(|node| {
            node.is_element()
                && node.tag_name().name() == TRACK_ITEM
                && (node.attribute(records::OBJECT_REF).is_some()
                    || node.attribute(records::OBJECT_UREF).is_some())
        })
    }
}

impl fmt::Debug for Record<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Record")
            .field("identity", &self.identity())
            .finish_non_exhaustive()
    }
}

/// The list whose direct children the audio reader resolves one member at a
/// time: an audio track group's tracks, or an audio track's clip items.
fn audio_member_list(record: Record<'_>) -> Option<Element<'_>> {
    let path: &[&str] = match record.tag() {
        tag if tag == records::AUDIO_TRACK_GROUP.tag => &[TRACK_GROUP, TRACKS],
        tag if tag == records::AUDIO_CLIP_TRACK.tag => &[CLIP_TRACK, CLIP_ITEMS, TRACK_ITEMS],
        _ => return None,
    };
    path.iter()
        .try_fold(record.element, |element, tag| element.child(tag))
}

fn identity(node: Node<'_, '_>) -> String {
    let tag = node.tag_name().name();
    match node
        .attribute(records::OBJECT_ID)
        .or_else(|| node.attribute(records::OBJECT_UID))
    {
        Some(id) => format!("{tag}:{id}"),
        None => tag.to_owned(),
    }
}

/// A decoded value paired with the native identity that supplied it.
#[derive(Debug)]
pub(crate) struct Located<T> {
    pub(crate) identity: String,
    pub(crate) value: T,
}

pub(crate) struct Graph<'a> {
    document: Document<'a>,
    ids: HashMap<String, NodeId>,
    uids: HashMap<String, NodeId>,
    /// Premiere writes each distinct binary value once; later copies are empty
    /// elements that name the first one by `BinaryHash`.
    binaries: HashMap<String, Binary>,
}

/// The value that the nonempty definitions of one `BinaryHash` store.
#[derive(Debug, Clone, Copy)]
enum Binary {
    Stored(NodeId),
    /// The definitions disagree, so an empty copy cannot choose one.
    Conflicting,
}

/// Whether two stored binary texts hold the same base64 data. Whitespace is not data.
fn same_binary(left: &str, right: &str) -> bool {
    left.chars()
        .filter(|c| !c.is_whitespace())
        .eq(right.chars().filter(|c| !c.is_whitespace()))
}

impl<'a> Graph<'a> {
    pub(crate) fn parse(source: &'a str) -> Result<Self> {
        // roxmltree's default node limit is u32::MAX, matching its NodeId
        // representation. Keep that actual representation check rather than a
        // smaller converter-specific project quota.
        let document = Document::parse_with_options(
            source,
            ParsingOptions {
                allow_dtd: false,
                ..Default::default()
            },
        )?;
        ensure_valid!(
            document.root_element().has_tag_name(records::PREMIERE_DATA),
            "not PremiereData"
        );
        ensure_valid!(
            document
                .descendants()
                .all(|node| node.ancestors().take(129).count() <= 128),
            "XML nesting exceeds limit"
        );
        let mut ids = HashMap::new();
        let mut uids = HashMap::new();
        for node in document.root_element().children().filter(Node::is_element) {
            for (key, table) in [
                (records::OBJECT_ID, &mut ids),
                (records::OBJECT_UID, &mut uids),
            ] {
                if let Some(id) = node.attribute(key) {
                    ensure_valid!(!id.is_empty(), "duplicate/empty {key}: {id}");
                    ensure_valid!(!table.contains_key(id), "duplicate/empty {key}: {id}");
                    table
                        .try_reserve(1)
                        .map_err(|source| FormatError::Allocation {
                            context: "object identity index",
                            source,
                        })?;
                    table.insert(id.to_owned(), node.id());
                }
            }
        }
        let mut binaries = HashMap::new();
        for node in document.descendants().filter(Node::is_element) {
            let (Some(hash), Some(text)) = (node.attribute(BINARY_HASH), node.text()) else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            match binaries.get(hash).copied() {
                None => {
                    binaries
                        .try_reserve(1)
                        .map_err(|source| FormatError::Allocation {
                            context: "binary value index",
                            source,
                        })?;
                    binaries.insert(hash.to_owned(), Binary::Stored(node.id()));
                }
                // Premiere repeats a definition only with the same data.
                Some(Binary::Stored(first)) => {
                    let first = document.get_node(first).and_then(|node| node.text());
                    if !first.is_some_and(|first| same_binary(first, text)) {
                        binaries.insert(hash.to_owned(), Binary::Conflicting);
                    }
                }
                Some(Binary::Conflicting) => {}
            }
        }
        Ok(Self {
            document,
            ids,
            uids,
            binaries,
        })
    }

    /// The stored text of a deduplicated binary value, or `None` when no element
    /// stores it. Definitions with different data are an error, not a choice.
    pub(crate) fn binary_value(&self, hash: &str, from: &str) -> Result<Option<&str>> {
        match self.binaries.get(hash) {
            None => Ok(None),
            Some(Binary::Conflicting) => Err(invalid(format!(
                "{from}: BinaryHash {hash} is defined with different values"
            ))),
            Some(Binary::Stored(id)) => {
                Ok(self.document.get_node(*id).and_then(|node| node.text()))
            }
        }
    }

    /// Check references and `xsi:nil` use inside one record before it is decoded.
    ///
    /// Parsing does not check them, so records that are never decoded cannot fail.
    fn validate_record(&self, record: Record<'_>) -> Result<()> {
        let audio_members = audio_member_list(record);
        for node in record.element.node.descendants().filter(Node::is_element) {
            // quick-xml decodes an xsi:nil element as absent, which would
            // silently erase content that the input shapes must see.
            ensure_valid!(
                !node
                    .attributes()
                    .any(|attribute| attribute.namespace().is_some() && attribute.name() == "nil"),
                "xsi:nil is unsupported"
            );
            // Resolve clip-item links in the occurrence loop so one broken link
            // does not discard the other occurrences on the same track.
            if ((record.tag() == records::VIDEO_CLIP_TRACK.tag
                || record.tag() == crate::schema::caption::CAPTION_DATA_CLIP_TRACK.tag)
                && node
                    .ancestors()
                    .any(|ancestor| ancestor.has_tag_name(TRACK_ITEMS)))
                || (record.tag() == records::VIDEO_TRACK_GROUP.tag
                    && node
                        .ancestors()
                        .any(|ancestor| ancestor.has_tag_name(TRACKS)))
                || (record.tag() == records::SEQUENCE.tag
                    && node
                        .ancestors()
                        .any(|ancestor| ancestor.has_tag_name("TrackGroups")))
            {
                continue;
            }
            // The audio reader also resolves each track and clip item in its own
            // loop. Its other links, including the audio transitions it never
            // reads, stay strict.
            if audio_members.is_some_and(|members| node.parent() == Some(members.node)) {
                continue;
            }
            // UI properties can contain references in private serialization scopes
            // (e.g. Columns.List). They are not edges in the native root graph.
            if node
                .ancestors()
                .any(|ancestor| ancestor.has_tag_name(PROPERTIES))
            {
                continue;
            }
            let regenerated_project_default = node
                .parent_element()
                .is_some_and(|parent| parent.has_tag_name(records::PROJECT_SETTINGS.tag))
                && node.attribute(records::OBJECT_REF).is_some_and(|id| {
                    REGENERATED_PROJECT_DEFAULTS.contains(&(node.tag_name().name(), id))
                        && !self.ids.contains_key(id)
                });
            if regenerated_project_default {
                continue;
            }
            let target = match (
                node.attribute(records::OBJECT_REF),
                node.attribute(records::OBJECT_UREF),
            ) {
                (None, None) => continue,
                (Some(id), None) => self.ids.get(id),
                (None, Some(uid)) => self.uids.get(uid),
                _ => {
                    return Err(invalid(format!(
                        "expected exactly one reference at {}",
                        identity(node)
                    )))
                }
            };
            ensure_valid!(target.is_some(), "missing reference at {}", identity(node));
        }
        Ok(())
    }

    pub(crate) fn records(&self) -> impl Iterator<Item = Record<'_>> {
        self.document
            .root_element()
            .children()
            .filter(Node::is_element)
            .map(|node| Record {
                element: Element { node },
            })
    }

    fn lookup(&self, id: Option<&str>, uid: Option<&str>, from: &str) -> Result<Record<'_>> {
        match (id, uid) {
            (Some(id), None) => self.ids.get(id),
            (None, Some(uid)) => self.uids.get(uid),
            _ => return Err(invalid(format!("expected exactly one reference at {from}"))),
        }
        .and_then(|node_id| self.document.get_node(*node_id))
        .map(|node| Record {
            element: Element { node },
        })
        .ok_or_else(|| invalid(format!("missing reference at {from}")))
    }

    pub(crate) fn locate(&self, reference: &Reference, from: &str) -> Result<Record<'_>> {
        self.lookup(reference.id.as_deref(), reference.uid.as_deref(), from)
    }

    pub(crate) fn locate_uid(&self, uid: &str, from: &str) -> Result<Record<'_>> {
        self.lookup(None, Some(uid), from)
    }

    /// Resolve a reference whose target must be a `tag` record.
    pub(crate) fn locate_as(
        &self,
        reference: &Reference,
        tag: &str,
        from: &str,
    ) -> Result<Record<'_>> {
        expect_tag(self.locate(reference, from)?, tag, from)
    }

    pub(crate) fn decode<T: DeserializeOwned>(&self, record: Record<'_>) -> Result<Located<T>> {
        self.validate_record(record)?;
        let identity = record.identity();
        let value =
            quick_xml::de::from_str(record.xml()).map_err(|source| FormatError::Decode {
                record: identity.clone(),
                source,
            })?;
        Ok(Located { identity, value })
    }

    pub(crate) fn decode_as<T: InputRecord>(
        &self,
        record: Record<'_>,
        from: &str,
    ) -> Result<Located<T>> {
        self.decode(expect_tag(record, T::TAG, from)?)
    }

    pub(crate) fn follow<T: InputRecord>(
        &self,
        reference: &Reference,
        from: &str,
    ) -> Result<Located<T>> {
        self.decode_as(self.locate(reference, from)?, from)
    }
}

fn expect_tag<'a>(record: Record<'a>, tag: &str, from: &str) -> Result<Record<'a>> {
    ensure_valid!(
        record.tag() == tag,
        "{from}: expected {tag}, found {} (unsupported or cyclic edge)",
        record.identity()
    );
    Ok(record)
}
