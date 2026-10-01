//! Bounded, policy-free RIFX framing.

use thiserror::Error;

const HEADER_LEN: usize = 8;

/// An error reading or writing RIFX framing.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RifxError {
    /// The container is truncated, inconsistent, or has invalid framing.
    #[error("invalid RIFX: {0}")]
    Invalid(&'static str),
    /// The container exceeds one of the reader's safety bounds.
    #[error("RIFX limit exceeded: {0}")]
    Limit(&'static str),
}

/// An uninterpreted four-byte RIFX identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FourCc([u8; 4]);

impl FourCc {
    /// Creates an identifier from its exact bytes.
    pub const fn new(bytes: [u8; 4]) -> Self {
        Self(bytes)
    }
    /// Returns the exact identifier bytes.
    pub const fn bytes(self) -> [u8; 4] {
        self.0
    }
}

/// A structurally valid data or LIST chunk.
#[derive(Debug, PartialEq, Eq)]
pub enum Chunk {
    /// An ordinary data chunk.
    Data(DataChunk),
    /// A LIST with either parsed children or a policy-selected opaque payload.
    List(ListChunk),
}

impl Clone for Chunk {
    fn clone(&self) -> Self {
        struct Frame<'a> {
            kind: FourCc,
            children: &'a [Chunk],
            next: usize,
            cloned: Vec<Chunk>,
        }

        fn leaf(chunk: &Chunk) -> Option<Chunk> {
            match chunk {
                Chunk::Data(data) => Some(Chunk::Data(data.clone())),
                Chunk::List(ListChunk {
                    kind,
                    body: ListBody::Opaque(bytes),
                }) => Some(Chunk::List(ListChunk {
                    kind: *kind,
                    body: ListBody::Opaque(bytes.clone()),
                })),
                Chunk::List(ListChunk {
                    body: ListBody::Children(_),
                    ..
                }) => None,
            }
        }

        if let Some(cloned) = leaf(self) {
            return cloned;
        }
        let Chunk::List(ListChunk {
            kind,
            body: ListBody::Children(children),
        }) = self
        else {
            unreachable!("leaf chunks returned above");
        };
        let mut stack = vec![Frame {
            kind: *kind,
            children,
            next: 0,
            cloned: Vec::with_capacity(children.len()),
        }];
        loop {
            let frame = stack.last_mut().expect("root clone frame remains present");
            if let Some(child) = frame.children.get(frame.next) {
                frame.next += 1;
                if let Some(cloned) = leaf(child) {
                    frame.cloned.push(cloned);
                } else if let Chunk::List(ListChunk {
                    kind,
                    body: ListBody::Children(children),
                }) = child
                {
                    stack.push(Frame {
                        kind: *kind,
                        children,
                        next: 0,
                        cloned: Vec::with_capacity(children.len()),
                    });
                }
                continue;
            }
            let frame = stack.pop().expect("completed clone frame exists");
            let cloned = Chunk::List(ListChunk {
                kind: frame.kind,
                body: ListBody::Children(frame.cloned),
            });
            if let Some(parent) = stack.last_mut() {
                parent.cloned.push(cloned);
            } else {
                return cloned;
            }
        }
    }
}

impl Drop for Chunk {
    fn drop(&mut self) {
        fn take_children(chunk: &mut Chunk) -> Option<Vec<Chunk>> {
            match chunk {
                Chunk::List(ListChunk {
                    body: ListBody::Children(children),
                    ..
                }) => Some(std::mem::take(children)),
                _ => None,
            }
        }

        let Some(children) = take_children(self) else {
            return;
        };
        let mut stack = vec![children];
        while let Some(children) = stack.last_mut() {
            if let Some(mut child) = children.pop() {
                if let Some(grandchildren) = take_children(&mut child) {
                    stack.push(grandchildren);
                }
            } else {
                stack.pop();
            }
        }
    }
}

/// An ordinary data chunk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataChunk {
    id: FourCc,
    payload: Vec<u8>,
}

/// The contents of a LIST after its four-byte list kind.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ListBody {
    Children(Vec<Chunk>),
    Opaque(Vec<u8>),
}

/// A LIST chunk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListChunk {
    kind: FourCc,
    body: ListBody,
}

impl Chunk {
    /// Creates a data chunk. LIST is reserved for structured construction.
    pub fn data(id: [u8; 4], payload: impl Into<Vec<u8>>) -> Result<Self, RifxError> {
        if id == *b"LIST" {
            return Err(RifxError::Invalid("data chunk uses LIST name"));
        }
        Ok(Self::Data(DataChunk {
            id: FourCc::new(id),
            payload: payload.into(),
        }))
    }
    /// Creates a recursively framed LIST.
    pub fn list(kind: [u8; 4], children: Vec<Self>) -> Self {
        Self::List(ListChunk {
            kind: FourCc::new(kind),
            body: ListBody::Children(children),
        })
    }
    /// Creates a LIST whose payload is deliberately opaque after its kind.
    pub(crate) fn opaque_list(kind: [u8; 4], payload: Vec<u8>) -> Self {
        Self::List(ListChunk {
            kind: FourCc::new(kind),
            body: ListBody::Opaque(payload),
        })
    }
    /// Returns LIST for lists and the data identifier otherwise.
    pub const fn id(&self) -> [u8; 4] {
        match self {
            Self::Data(data) => data.id.bytes(),
            Self::List(_) => *b"LIST",
        }
    }
    /// Returns a LIST's kind.
    pub const fn list_kind(&self) -> Option<[u8; 4]> {
        match self {
            Self::List(list) => Some(list.kind.bytes()),
            Self::Data(_) => None,
        }
    }
    /// Returns an ordinary data payload.
    pub fn data_payload(&self) -> Option<&[u8]> {
        match self {
            Self::Data(data) => Some(&data.payload),
            Self::List(_) => None,
        }
    }
    /// Returns the uninterpreted bytes after an opaque LIST's kind.
    pub fn opaque_payload(&self) -> Option<&[u8]> {
        match self {
            Self::List(ListChunk {
                body: ListBody::Opaque(bytes),
                ..
            }) => Some(bytes),
            _ => None,
        }
    }
    /// Returns parsed LIST children.
    pub fn children(&self) -> Option<&[Self]> {
        match self {
            Self::List(ListChunk {
                body: ListBody::Children(children),
                ..
            }) => Some(children),
            _ => None,
        }
    }
    /// Returns mutable parsed LIST children.
    pub fn children_mut(&mut self) -> Option<&mut Vec<Self>> {
        match self {
            Self::List(ListChunk {
                body: ListBody::Children(children),
                ..
            }) => Some(children),
            _ => None,
        }
    }
}

/// A generic RIFX container and bytes following its declared envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rifx {
    form: FourCc,
    chunks: Vec<Chunk>,
    trailing: Vec<u8>,
}

impl Rifx {
    /// Parses a bounded RIFX container. The predicate selects opaque LIST kinds.
    pub fn parse_with(input: &[u8], opaque: impl Fn([u8; 4]) -> bool) -> Result<Self, RifxError> {
        if input.len() < 12 || &input[..4] != b"RIFX" {
            return Err(RifxError::Invalid("missing RIFX header"));
        }
        let size = read_size(&input[4..8])?;
        if size < 4 {
            return Err(RifxError::Invalid("RIFX body too short"));
        }
        let end = HEADER_LEN
            .checked_add(size)
            .ok_or(RifxError::Invalid("RIFX body overflow"))?;
        if end > input.len() {
            return Err(RifxError::Invalid("truncated RIFX body"));
        }
        let form = FourCc::new(
            input[8..12]
                .try_into()
                .map_err(|_| RifxError::Invalid("short RIFX form"))?,
        );
        let chunks = read_chunks(&input[12..end], &opaque)?;
        let trailing_start = end
            .checked_add(size & 1)
            .ok_or(RifxError::Invalid("RIFX pad overflow"))?;
        if trailing_start > input.len() {
            return Err(RifxError::Invalid("missing RIFX pad byte"));
        }
        if size & 1 != 0 && input[end] != 0 {
            return Err(RifxError::Invalid("nonzero RIFX pad byte"));
        }
        Ok(Self {
            form,
            chunks,
            trailing: input[trailing_start..].to_vec(),
        })
    }
    /// Creates a generic container.
    pub fn new(form: [u8; 4], chunks: Vec<Chunk>, trailing: Vec<u8>) -> Self {
        Self {
            form: FourCc::new(form),
            chunks,
            trailing,
        }
    }
    /// Returns the form identifier.
    pub const fn form(&self) -> [u8; 4] {
        self.form.bytes()
    }
    /// Returns top-level chunks.
    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }
    /// Returns the bytes following the envelope.
    pub fn trailing(&self) -> &[u8] {
        &self.trailing
    }
    /// Decomposes the container.
    pub fn into_parts(self) -> ([u8; 4], Vec<Chunk>, Vec<u8>) {
        (self.form.bytes(), self.chunks, self.trailing)
    }
    /// Encodes after a bounded preflight, before allocating the output.
    pub fn encode(&self) -> Result<Vec<u8>, RifxError> {
        encode(self.form.bytes(), &self.chunks, &self.trailing)
    }
}

// Borrow both the tree and tail so envelope adapters need not clone an entire file.
pub(crate) fn encode(
    form: [u8; 4],
    chunks: &[Chunk],
    trailing: &[u8],
) -> Result<Vec<u8>, RifxError> {
    let chunks_len = encoded_chunks_len(chunks)?;
    let body_len = 4usize
        .checked_add(chunks_len)
        .ok_or(RifxError::Limit("RIFX body bytes"))?;
    let envelope_len = HEADER_LEN
        .checked_add(body_len)
        .and_then(|n| n.checked_add(body_len & 1))
        .ok_or(RifxError::Limit("file bytes"))?;
    let total = bounded_file_len(envelope_len, trailing.len())?;
    let size = u32::try_from(body_len).map_err(|_| RifxError::Limit("RIFX body bytes"))?;
    let mut out = Vec::new();
    out.try_reserve_exact(total)
        .map_err(|_| RifxError::Limit("output allocation"))?;
    out.extend_from_slice(b"RIFX");
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(&form);
    write_chunks(chunks, &mut out);
    if body_len & 1 != 0 {
        out.push(0);
    }
    out.extend_from_slice(trailing);
    Ok(out)
}

fn bounded_file_len(envelope: usize, trailing: usize) -> Result<usize, RifxError> {
    envelope
        .checked_add(trailing)
        .ok_or(RifxError::Limit("file bytes"))
}

fn read_size(bytes: &[u8]) -> Result<usize, RifxError> {
    Ok(u32::from_be_bytes(
        bytes
            .try_into()
            .map_err(|_| RifxError::Invalid("short chunk length"))?,
    ) as usize)
}

fn read_chunks(input: &[u8], opaque: &impl Fn([u8; 4]) -> bool) -> Result<Vec<Chunk>, RifxError> {
    struct Frame<'a> {
        input: &'a [u8],
        offset: usize,
        kind: Option<[u8; 4]>,
        chunks: Vec<Chunk>,
    }

    enum Next<'a> {
        Chunk(Chunk),
        List([u8; 4], &'a [u8]),
    }

    let mut stack = vec![Frame {
        input,
        offset: 0,
        kind: None,
        chunks: Vec::new(),
    }];
    loop {
        let frame = stack.last_mut().expect("root read frame remains present");
        if frame.offset == frame.input.len() {
            let frame = stack.pop().expect("completed read frame exists");
            let Some(kind) = frame.kind else {
                return Ok(frame.chunks);
            };
            stack
                .last_mut()
                .expect("nested LIST has a parent frame")
                .chunks
                .push(Chunk::list(kind, frame.chunks));
            continue;
        }

        let next = {
            let header = frame
                .input
                .get(frame.offset..frame.offset + HEADER_LEN)
                .ok_or(RifxError::Invalid("short chunk header"))?;
            let id: [u8; 4] = header[..4]
                .try_into()
                .map_err(|_| RifxError::Invalid("short chunk name"))?;
            let length = read_size(&header[4..])?;
            let start = frame
                .offset
                .checked_add(HEADER_LEN)
                .ok_or(RifxError::Invalid("chunk length overflow"))?;
            let end = start
                .checked_add(length)
                .ok_or(RifxError::Invalid("chunk length overflow"))?;
            let body = frame
                .input
                .get(start..end)
                .ok_or(RifxError::Invalid("chunk exceeds its parent"))?;
            frame.offset = end
                .checked_add(length & 1)
                .ok_or(RifxError::Invalid("chunk pad overflow"))?;
            if frame.offset > frame.input.len() {
                return Err(RifxError::Invalid("missing chunk pad byte"));
            }
            if length & 1 != 0 && frame.input[end] != 0 {
                return Err(RifxError::Invalid("nonzero chunk pad byte"));
            }
            if id != *b"LIST" {
                Next::Chunk(Chunk::data(id, body.to_vec())?)
            } else {
                let kind: [u8; 4] = body
                    .get(..4)
                    .ok_or(RifxError::Invalid("short LIST body"))?
                    .try_into()
                    .map_err(|_| RifxError::Invalid("short LIST body"))?;
                if opaque(kind) {
                    Next::Chunk(Chunk::opaque_list(kind, body[4..].to_vec()))
                } else {
                    Next::List(kind, &body[4..])
                }
            }
        };
        match next {
            Next::Chunk(chunk) => stack
                .last_mut()
                .expect("current read frame remains present")
                .chunks
                .push(chunk),
            Next::List(kind, input) => stack.push(Frame {
                input,
                offset: 0,
                kind: Some(kind),
                chunks: Vec::new(),
            }),
        }
    }
}

fn encoded_chunks_len(chunks: &[Chunk]) -> Result<usize, RifxError> {
    struct Frame<'a> {
        chunks: &'a [Chunk],
        next: usize,
        total: usize,
    }

    fn add_chunk(total: &mut usize, payload: usize) -> Result<(), RifxError> {
        u32::try_from(payload).map_err(|_| RifxError::Limit("chunk body bytes"))?;
        *total = total
            .checked_add(HEADER_LEN)
            .and_then(|n| n.checked_add(payload))
            .and_then(|n| n.checked_add(payload & 1))
            .ok_or(RifxError::Limit("file bytes"))?;
        Ok(())
    }

    let mut stack = vec![Frame {
        chunks,
        next: 0,
        total: 0,
    }];
    loop {
        let frame = stack.last_mut().expect("root length frame remains present");
        let Some(chunk) = frame.chunks.get(frame.next) else {
            let completed = stack.pop().expect("completed length frame exists").total;
            let Some(parent) = stack.last_mut() else {
                return Ok(completed);
            };
            let payload = 4usize
                .checked_add(completed)
                .ok_or(RifxError::Limit("chunk body bytes"))?;
            add_chunk(&mut parent.total, payload)?;
            continue;
        };
        frame.next += 1;
        match chunk {
            Chunk::Data(data) => add_chunk(&mut frame.total, data.payload.len())?,
            Chunk::List(ListChunk {
                body: ListBody::Opaque(bytes),
                ..
            }) => {
                let payload = 4usize
                    .checked_add(bytes.len())
                    .ok_or(RifxError::Limit("chunk body bytes"))?;
                add_chunk(&mut frame.total, payload)?;
            }
            Chunk::List(ListChunk {
                body: ListBody::Children(children),
                ..
            }) => stack.push(Frame {
                chunks: children,
                next: 0,
                total: 0,
            }),
        }
    }
}

fn write_chunks(chunks: &[Chunk], out: &mut Vec<u8>) {
    struct Frame<'a> {
        chunks: &'a [Chunk],
        next: usize,
        list_start: Option<usize>,
    }

    fn finish_chunk(out: &mut Vec<u8>, start: usize) {
        let size = u32::try_from(out.len() - start - HEADER_LEN)
            .expect("preflight bounded each chunk body to u32");
        out[start + 4..start + 8].copy_from_slice(&size.to_be_bytes());
        if size & 1 != 0 {
            out.push(0);
        }
    }

    let mut stack = vec![Frame {
        chunks,
        next: 0,
        list_start: None,
    }];
    while let Some(frame) = stack.last_mut() {
        let Some(chunk) = frame.chunks.get(frame.next) else {
            let completed = stack.pop().expect("completed write frame exists");
            if let Some(start) = completed.list_start {
                finish_chunk(out, start);
            }
            continue;
        };
        frame.next += 1;
        let start = out.len();
        out.extend_from_slice(&chunk.id());
        out.extend_from_slice(&[0; 4]);
        match chunk {
            Chunk::Data(data) => {
                out.extend_from_slice(&data.payload);
                finish_chunk(out, start);
            }
            Chunk::List(ListChunk {
                kind,
                body: ListBody::Opaque(bytes),
            }) => {
                out.extend_from_slice(&kind.0);
                out.extend_from_slice(bytes);
                finish_chunk(out, start);
            }
            Chunk::List(ListChunk {
                kind,
                body: ListBody::Children(children),
            }) => {
                out.extend_from_slice(&kind.0);
                stack.push(Frame {
                    chunks: children,
                    next: 0,
                    list_start: Some(start),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests;
