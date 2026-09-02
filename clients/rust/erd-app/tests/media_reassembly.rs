use std::time::Instant;

use erd_app::{CursorState, FrameAssembler, MediaAssemblyError};
use erd_proto::{CursorUpdate, FrameChunk, FrameHeader, MAX_CHUNKS_PER_FRAME, MAX_FRAME_BYTES};

#[test]
fn video_chunks_reassemble_out_of_order_and_from_orphans() {
    let now = Instant::now();
    let mut assembler = FrameAssembler::default();
    assert_eq!(
        assembler
            .push_chunk(
                FrameChunk {
                    frame_id: 9,
                    chunk_index: 1,
                    data: b"world".to_vec(),
                },
                now,
            )
            .unwrap(),
        None
    );
    let header = FrameHeader {
        frame_id: 9,
        width: 640,
        height: 360,
        is_key_frame: true,
        total_chunks: 2,
        total_size: 10,
    };
    assert_eq!(assembler.push_header(header, 123, now).unwrap(), None);
    let frame = assembler
        .push_chunk(
            FrameChunk {
                frame_id: 9,
                chunk_index: 0,
                data: b"hello".to_vec(),
            },
            now,
        )
        .unwrap()
        .unwrap();
    assert_eq!(frame.data, b"helloworld");
    assert_eq!(frame.timestamp_ms, 123);
}

#[test]
fn video_reassembly_enforces_protocol_caps() {
    let now = Instant::now();
    let mut assembler = FrameAssembler::default();
    let too_many = FrameHeader {
        frame_id: 1,
        width: 1,
        height: 1,
        is_key_frame: false,
        total_chunks: MAX_CHUNKS_PER_FRAME + 1,
        total_size: 1,
    };
    assert_eq!(
        assembler.push_header(too_many, 0, now),
        Err(MediaAssemblyError::InvalidHeader)
    );
    let too_large = FrameHeader {
        frame_id: 2,
        width: 1,
        height: 1,
        is_key_frame: false,
        total_chunks: 1,
        total_size: MAX_FRAME_BYTES + 1,
    };
    assert_eq!(
        assembler.push_header(too_large, 0, now),
        Err(MediaAssemblyError::InvalidHeader)
    );
}

#[test]
fn cursor_updates_are_clamped_for_overlay_use() {
    let mut cursor = CursorState::default();
    cursor.update(CursorUpdate {
        x: -0.2,
        y: 1.4,
        cursor_type: 3,
    });
    assert_eq!(cursor.x, 0.0);
    assert_eq!(cursor.y, 1.0);
    assert_eq!(cursor.cursor_type, 3);
}
