use super::{Chunk, Rifx, RifxError, bounded_file_len, encoded_chunks_len, read_chunks};

fn parse(bytes: &[u8]) -> Result<Rifx, RifxError> {
    Rifx::parse_with(bytes, |_| false)
}

fn envelope(body: &[u8]) -> Vec<u8> {
    let mut bytes = b"RIFX".to_vec();
    bytes.extend_from_slice(&u32::try_from(4 + body.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(b"TEST");
    bytes.extend_from_slice(body);
    bytes
}

#[test]
fn data_constructor_cannot_impersonate_a_list() {
    assert_eq!(
        Chunk::data(*b"LIST", vec![]),
        Err(RifxError::Invalid("data chunk uses LIST name"))
    );
    let list = Chunk::list(*b"Nest", vec![]);
    assert_eq!(list.id(), *b"LIST");
    assert_eq!(list.list_kind(), Some(*b"Nest"));
    assert!(list.data_payload().is_none());
}

#[test]
fn generic_form_unknown_bytes_duplicates_and_tail_roundtrip() {
    let rifx = Rifx::new(
        *b"TEST",
        vec![
            Chunk::data(*b"same", vec![1, 2, 3]).unwrap(),
            Chunk::list(
                *b"Nest",
                vec![Chunk::data([0xff, 0, 1, 2], vec![7]).unwrap()],
            ),
            Chunk::data(*b"same", vec![4]).unwrap(),
        ],
        vec![9, 8, 7],
    );
    let bytes = rifx.encode().unwrap();
    assert_eq!(parse(&bytes).unwrap(), rifx);
    assert_eq!(parse(&bytes).unwrap().encode().unwrap(), bytes);
    assert_eq!(&bytes[12..24], b"same\0\0\0\x03\x01\x02\x03\0");
    assert_eq!(rifx.form(), *b"TEST");
    assert_eq!(rifx.trailing(), [9, 8, 7]);
}

#[test]
fn opacity_is_a_caller_policy_not_an_ae_name_in_framing() {
    let bytes = Rifx::new(
        *b"TEST",
        vec![Chunk::opaque_list(*b"Cust", b"not chunks!".to_vec())],
        vec![],
    )
    .encode()
    .unwrap();
    assert!(parse(&bytes).is_err());
    let parsed = Rifx::parse_with(&bytes, |kind| kind == *b"Cust").unwrap();
    assert_eq!(
        parsed.chunks()[0].opaque_payload(),
        Some(&b"not chunks!"[..])
    );
    assert!(parsed.chunks()[0].children().is_none());
    assert_eq!(parsed.encode().unwrap(), bytes);
}

#[test]
fn rejects_truncation_parent_overrun_short_header_and_short_list() {
    let bytes = Rifx::new(
        *b"TEST",
        vec![Chunk::data(*b"data", vec![7]).unwrap()],
        vec![],
    )
    .encode()
    .unwrap();
    for end in 0..bytes.len() {
        assert!(parse(&bytes[..end]).is_err(), "accepted prefix {end}");
    }
    let mut invalid = bytes;
    invalid[16..20].copy_from_slice(&50_u32.to_be_bytes());
    assert_eq!(
        parse(&invalid),
        Err(RifxError::Invalid("chunk exceeds its parent"))
    );
    assert_eq!(
        parse(&envelope(b"short!")),
        Err(RifxError::Invalid("short chunk header"))
    );
    assert_eq!(
        parse(&envelope(b"LIST\0\0\0\0")),
        Err(RifxError::Invalid("short LIST body"))
    );
    assert!(parse(b"RIFF\x04\0\0\0TEST").is_err());
    assert!(parse(b"RIFX\0\0\0\x03TEST").is_err());
}

#[test]
fn rejects_missing_and_nonzero_chunk_padding() {
    assert_eq!(
        parse(&envelope(b"data\0\0\0\x01x")),
        Err(RifxError::Invalid("missing chunk pad byte"))
    );
    assert_eq!(
        parse(&envelope(b"data\0\0\0\x01x\x01")),
        Err(RifxError::Invalid("nonzero chunk pad byte"))
    );
    // An even root envelope may still hide a nested odd payload with bad padding.
    assert_eq!(
        parse(&envelope(b"LIST\0\0\0\x0eNestdata\0\0\0\x01x\x01")),
        Err(RifxError::Invalid("nonzero chunk pad byte"))
    );
}

#[test]
fn deeply_nested_lists_parse_clone_encode_and_drop_on_a_small_stack() {
    const DEPTH: usize = 10_000;
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut raw = Vec::with_capacity(DEPTH * 12);
            for remaining in (0..DEPTH).rev() {
                raw.extend_from_slice(b"LIST");
                let length = u32::try_from(4 + remaining * 12).unwrap();
                raw.extend_from_slice(&length.to_be_bytes());
                raw.extend_from_slice(b"Nest");
            }
            let source = envelope(&raw);

            let mut chunks = Vec::new();
            for _ in 0..DEPTH {
                chunks = vec![Chunk::list(*b"Nest", chunks)];
            }
            let cloned = chunks[0].clone();
            assert_eq!(
                Rifx::new(*b"TEST", vec![cloned], vec![]).encode().unwrap(),
                source
            );

            let decoded = parse(&source).unwrap();
            let mut children = decoded.chunks();
            for _ in 0..DEPTH {
                assert_eq!(children.len(), 1);
                children = children[0].children().unwrap();
            }
            assert!(children.is_empty());
            assert_eq!(decoded.encode().unwrap(), source);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn chunk_count_above_former_limit_roundtrips_across_lists() {
    const CHILDREN: usize = 200_001;
    let child = Chunk::data(*b"data", vec![]).unwrap();
    let chunks = vec![Chunk::list(*b"Nest", vec![child.clone(); CHILDREN]), child];
    // Build source bytes independently so the reader test cannot merely accept
    // a writer that accidentally drops chunks to stay below the former cap.
    let mut body = b"LIST".to_vec();
    body.extend_from_slice(&u32::try_from(4 + CHILDREN * 8).unwrap().to_be_bytes());
    body.extend_from_slice(b"Nest");
    for _ in 0..=CHILDREN {
        body.extend_from_slice(b"data\0\0\0\0");
    }
    let source = envelope(&body);
    let decoded = parse(&source).unwrap();
    assert_eq!(decoded.chunks().len(), 2);
    assert_eq!(decoded.chunks()[0].children().unwrap().len(), CHILDREN);
    assert_eq!(decoded.encode().unwrap(), source);
    assert_eq!(
        Rifx::new(*b"TEST", chunks, vec![]).encode().unwrap(),
        source
    );
}

#[test]
fn large_chunk_count_does_not_hide_a_truncated_final_chunk() {
    let mut body = b"data\0\0\0\0".repeat(200_001);
    body.extend_from_slice(b"data\0\0\0\x02x");
    assert_eq!(
        parse(&envelope(&body)),
        Err(RifxError::Invalid("chunk exceeds its parent"))
    );
}

#[test]
fn nested_and_sibling_chunks_have_exact_encoded_lengths() {
    let leaf = Chunk::data(*b"data", vec![]).unwrap();
    let chunks = vec![
        Chunk::list(*b"Nest", vec![leaf.clone(), leaf.clone()]),
        Chunk::list(*b"Nest", vec![leaf]),
    ];
    let bytes = Rifx::new(*b"TEST", chunks.clone(), vec![])
        .encode()
        .unwrap();
    assert_eq!(parse(&bytes).unwrap().chunks(), chunks);

    assert_eq!(read_chunks(&bytes[12..], &|_| false).unwrap(), chunks);
    assert_eq!(encoded_chunks_len(&chunks).unwrap(), bytes.len() - 12);
}

#[test]
fn preflight_includes_tail_and_catches_overflow_without_large_allocations() {
    let former_limit = 256 * 1024 * 1024;
    assert_eq!(bounded_file_len(former_limit, 1), Ok(former_limit + 1));
    assert_eq!(
        bounded_file_len(12, usize::MAX),
        Err(RifxError::Limit("file bytes"))
    );
}
