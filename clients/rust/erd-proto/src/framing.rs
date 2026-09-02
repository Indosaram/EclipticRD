use crate::{codec::usize_to_u32, CodecError};

pub const MAX_TCP_FRAME_SIZE: usize = 16 * 1024 * 1024;

pub struct TcpFrameWriter;

impl TcpFrameWriter {
    pub fn encode(payload: &[u8]) -> Result<Vec<u8>, CodecError> {
        if payload.is_empty() || payload.len() > MAX_TCP_FRAME_SIZE {
            return Err(CodecError::InvalidFrameLength(
                u32::try_from(payload.len()).unwrap_or(u32::MAX),
            ));
        }
        let length = usize_to_u32(payload.len(), "TCP frame")?;
        let mut output = Vec::with_capacity(4 + payload.len());
        output.extend_from_slice(&length.to_le_bytes());
        output.extend_from_slice(payload);
        Ok(output)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TcpFrameEvent {
    Frame(Vec<u8>),
    DroppedInvalidLength(u32),
}

#[derive(Debug, Default)]
pub struct TcpFrameReader {
    buffer: Vec<u8>,
}

impl TcpFrameReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    pub fn push(&mut self, input: &[u8]) -> Vec<TcpFrameEvent> {
        self.buffer.extend_from_slice(input);
        let mut events = Vec::new();

        loop {
            if self.buffer.len() < 4 {
                break;
            }
            let length = u32::from_le_bytes([
                self.buffer[0],
                self.buffer[1],
                self.buffer[2],
                self.buffer[3],
            ]);
            if length == 0 || length as usize > MAX_TCP_FRAME_SIZE {
                self.buffer.clear();
                events.push(TcpFrameEvent::DroppedInvalidLength(length));
                break;
            }
            let frame_end = 4 + length as usize;
            if self.buffer.len() < frame_end {
                break;
            }
            let payload = self.buffer[4..frame_end].to_vec();
            self.buffer.drain(..frame_end);
            events.push(TcpFrameEvent::Frame(payload));
        }

        events
    }
}
