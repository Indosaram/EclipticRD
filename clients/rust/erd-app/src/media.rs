use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    time::{Duration, Instant},
};

use erd_proto::{CursorUpdate, FrameChunk, FrameHeader, MAX_CHUNKS_PER_FRAME, MAX_FRAME_BYTES};
use thiserror::Error;

const MAX_ORPHAN_FRAMES: usize = 16;
const ASSEMBLY_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledFrame {
    pub header: FrameHeader,
    pub data: Vec<u8>,
    pub timestamp_ms: u32,
}

#[derive(Debug)]
struct FrameAssembly {
    header: FrameHeader,
    chunks: BTreeMap<u16, Vec<u8>>,
    started: Instant,
    timestamp_ms: u32,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MediaAssemblyError {
    #[error("frame header exceeds protocol caps")]
    InvalidHeader,
    #[error("frame chunk index is outside the declared frame")]
    InvalidChunkIndex,
    #[error("assembled frame size differs from its header")]
    SizeMismatch,
}

#[derive(Debug, Default)]
pub struct FrameAssembler {
    frames: HashMap<u32, FrameAssembly>,
    orphans: HashMap<u32, BTreeMap<u16, Vec<u8>>>,
    expected_frame_id: Option<u32>,
    recent_loss: VecDeque<(Instant, u64, u64)>,
    completed_frames: u64,
}

impl FrameAssembler {
    pub fn push_header(
        &mut self,
        header: FrameHeader,
        timestamp_ms: u32,
        now: Instant,
    ) -> Result<Option<AssembledFrame>, MediaAssemblyError> {
        if header.total_chunks == 0
            || header.total_chunks > MAX_CHUNKS_PER_FRAME
            || header.total_size == 0
            || header.total_size > MAX_FRAME_BYTES
        {
            return Err(MediaAssemblyError::InvalidHeader);
        }
        self.expire(now);
        self.track_loss(header.frame_id, now);
        if header.is_key_frame {
            self.frames.retain(|frame_id, assembly| {
                *frame_id >= header.frame_id || Self::complete(assembly)
            });
        }
        let chunks = self.orphans.remove(&header.frame_id).unwrap_or_default();
        let frame_id = header.frame_id;
        let assembly = FrameAssembly {
            header,
            chunks,
            started: now,
            timestamp_ms,
        };
        self.frames.insert(frame_id, assembly);
        self.finish_if_complete(frame_id)
    }

    pub fn push_chunk(
        &mut self,
        chunk: FrameChunk,
        now: Instant,
    ) -> Result<Option<AssembledFrame>, MediaAssemblyError> {
        self.expire(now);
        if let Some(assembly) = self.frames.get_mut(&chunk.frame_id) {
            if chunk.chunk_index >= assembly.header.total_chunks {
                return Err(MediaAssemblyError::InvalidChunkIndex);
            }
            assembly
                .chunks
                .entry(chunk.chunk_index)
                .or_insert(chunk.data);
            return self.finish_if_complete(chunk.frame_id);
        }

        if self.orphans.len() >= MAX_ORPHAN_FRAMES && !self.orphans.contains_key(&chunk.frame_id) {
            return Ok(None);
        }
        let orphans = self.orphans.entry(chunk.frame_id).or_default();
        if orphans.len() >= MAX_CHUNKS_PER_FRAME as usize {
            self.orphans.remove(&chunk.frame_id);
            return Ok(None);
        }
        orphans.entry(chunk.chunk_index).or_insert(chunk.data);
        Ok(None)
    }

    pub fn loss_ratio(&mut self, now: Instant) -> f64 {
        self.trim_loss(now);
        let (lost, total) = self.recent_loss.iter().fold(
            (0_u64, 0_u64),
            |(lost, total), (_, entry_lost, entry_total)| (lost + entry_lost, total + entry_total),
        );
        if total == 0 {
            0.0
        } else {
            lost as f64 / total as f64
        }
    }

    pub fn completed_frames(&self) -> u64 {
        self.completed_frames
    }

    pub fn clear(&mut self) {
        self.frames.clear();
        self.orphans.clear();
        self.expected_frame_id = None;
        self.recent_loss.clear();
        self.completed_frames = 0;
    }

    fn finish_if_complete(
        &mut self,
        frame_id: u32,
    ) -> Result<Option<AssembledFrame>, MediaAssemblyError> {
        let Some(assembly) = self.frames.get(&frame_id) else {
            return Ok(None);
        };
        if !Self::complete(assembly) {
            return Ok(None);
        }
        let assembly = self.frames.remove(&frame_id).expect("frame existed above");
        let mut data = Vec::with_capacity(assembly.header.total_size as usize);
        for index in 0..assembly.header.total_chunks {
            data.extend_from_slice(
                assembly
                    .chunks
                    .get(&index)
                    .ok_or(MediaAssemblyError::SizeMismatch)?,
            );
        }
        if data.len() != assembly.header.total_size as usize {
            return Err(MediaAssemblyError::SizeMismatch);
        }
        self.completed_frames += 1;
        Ok(Some(AssembledFrame {
            header: assembly.header,
            data,
            timestamp_ms: assembly.timestamp_ms,
        }))
    }

    fn complete(assembly: &FrameAssembly) -> bool {
        assembly.chunks.len() == assembly.header.total_chunks as usize
    }

    fn expire(&mut self, now: Instant) {
        self.frames.retain(|_, assembly| {
            now.saturating_duration_since(assembly.started) < ASSEMBLY_TIMEOUT
        });
    }

    fn track_loss(&mut self, frame_id: u32, now: Instant) {
        let lost = self
            .expected_frame_id
            .map_or(0, |expected| frame_id.saturating_sub(expected) as u64);
        self.recent_loss.push_back((now, lost, lost + 1));
        self.expected_frame_id = Some(
            self.expected_frame_id
                .map_or(frame_id.wrapping_add(1), |expected| {
                    expected.max(frame_id.wrapping_add(1))
                }),
        );
        self.trim_loss(now);
    }

    fn trim_loss(&mut self, now: Instant) {
        while self.recent_loss.front().is_some_and(|(timestamp, _, _)| {
            now.saturating_duration_since(*timestamp) > Duration::from_secs(5)
        }) {
            self.recent_loss.pop_front();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorState {
    pub x: f32,
    pub y: f32,
    pub cursor_type: u8,
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            x: -1.0,
            y: -1.0,
            cursor_type: 0,
        }
    }
}

impl CursorState {
    pub fn update(&mut self, update: CursorUpdate) {
        self.x = update.x.clamp(0.0, 1.0);
        self.y = update.y.clamp(0.0, 1.0);
        self.cursor_type = update.cursor_type;
    }
}
