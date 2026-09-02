use crate::{
    codec::{push_f32, push_u16, push_u32, Decoder},
    CodecError, WireCodec,
};

pub const MAX_CHUNKS_PER_FRAME: u16 = 1024;
pub const MAX_FRAME_BYTES: u32 = 32 * 1024 * 1024;
pub const MAX_VIDEO_CHUNK_BYTES: usize = 1382;
pub const MAX_AUDIO_FRAGMENT_BYTES: usize = 1380;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameHeader {
    pub frame_id: u32,
    pub width: u16,
    pub height: u16,
    pub is_key_frame: bool,
    pub total_chunks: u16,
    pub total_size: u32,
}

impl FrameHeader {
    pub const SIZE: usize = 16;

    fn validate(&self) -> Result<(), CodecError> {
        if self.total_chunks > MAX_CHUNKS_PER_FRAME {
            return Err(CodecError::LengthLimit {
                field: "frame chunks",
                actual: self.total_chunks as usize,
                max: MAX_CHUNKS_PER_FRAME as usize,
            });
        }
        if self.total_size > MAX_FRAME_BYTES {
            return Err(CodecError::LengthLimit {
                field: "frame bytes",
                actual: self.total_size as usize,
                max: MAX_FRAME_BYTES as usize,
            });
        }
        Ok(())
    }
}

impl WireCodec for FrameHeader {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.validate()?;
        let mut output = Vec::with_capacity(Self::SIZE);
        push_u32(&mut output, self.frame_id);
        push_u16(&mut output, self.width);
        push_u16(&mut output, self.height);
        output.push(u8::from(self.is_key_frame));
        push_u16(&mut output, self.total_chunks);
        output.push(0);
        push_u32(&mut output, self.total_size);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            frame_id: decoder.u32("frame ID")?,
            width: decoder.u16("frame width")?,
            height: decoder.u16("frame height")?,
            is_key_frame: decoder.u8("frame key flag")? != 0,
            total_chunks: decoder.u16("frame chunk count")?,
            total_size: {
                let _padding = decoder.u8("frame padding")?;
                decoder.u32("frame total size")?
            },
        };
        decoder.finish("frame header")?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameChunk {
    pub frame_id: u32,
    pub chunk_index: u16,
    pub data: Vec<u8>,
}

impl FrameChunk {
    pub const HEADER_SIZE: usize = 6;

    fn validate(&self) -> Result<(), CodecError> {
        if self.data.len() > MAX_VIDEO_CHUNK_BYTES {
            return Err(CodecError::LengthLimit {
                field: "video chunk data",
                actual: self.data.len(),
                max: MAX_VIDEO_CHUNK_BYTES,
            });
        }
        Ok(())
    }
}

impl WireCodec for FrameChunk {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.validate()?;
        let mut output = Vec::with_capacity(Self::HEADER_SIZE + self.data.len());
        push_u32(&mut output, self.frame_id);
        push_u16(&mut output, self.chunk_index);
        output.extend_from_slice(&self.data);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let frame_id = decoder.u32("frame chunk ID")?;
        let chunk_index = decoder.u16("frame chunk index")?;
        let data = decoder.take_remaining().to_vec();
        let value = Self {
            frame_id,
            chunk_index,
            data,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorUpdate {
    pub x: f32,
    pub y: f32,
    pub cursor_type: u8,
}

impl CursorUpdate {
    pub const SIZE: usize = 9;
}

impl WireCodec for CursorUpdate {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(Self::SIZE);
        push_f32(&mut output, self.x);
        push_f32(&mut output, self.y);
        output.push(self.cursor_type);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let x = decoder.f32("cursor x")?;
        let y = decoder.f32("cursor y")?;
        let cursor_type = decoder.u8("cursor type")?;
        decoder.finish("cursor update")?;
        Ok(Self { x, y, cursor_type })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFragmentHeader {
    pub frame_id: u32,
    pub fragment_index: u16,
    pub fragment_count: u16,
}

impl AudioFragmentHeader {
    pub const SIZE: usize = 8;

    fn validate(&self) -> Result<(), CodecError> {
        if self.fragment_count == 0 {
            return Err(CodecError::InvalidValue {
                field: "audio fragment count",
                value: 0,
            });
        }
        if self.fragment_index >= self.fragment_count {
            return Err(CodecError::InvalidValue {
                field: "audio fragment index",
                value: self.fragment_index as u64,
            });
        }
        Ok(())
    }
}

impl WireCodec for AudioFragmentHeader {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.validate()?;
        let mut output = Vec::with_capacity(Self::SIZE);
        push_u32(&mut output, self.frame_id);
        push_u16(&mut output, self.fragment_index);
        push_u16(&mut output, self.fragment_count);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            frame_id: decoder.u32("audio frame ID")?,
            fragment_index: decoder.u16("audio fragment index")?,
            fragment_count: decoder.u16("audio fragment count")?,
        };
        decoder.finish("audio fragment header")?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFragment {
    pub header: AudioFragmentHeader,
    pub data: Vec<u8>,
}

impl AudioFragment {
    fn validate(&self) -> Result<(), CodecError> {
        self.header.validate()?;
        if self.data.len() > MAX_AUDIO_FRAGMENT_BYTES {
            return Err(CodecError::LengthLimit {
                field: "audio fragment data",
                actual: self.data.len(),
                max: MAX_AUDIO_FRAGMENT_BYTES,
            });
        }
        Ok(())
    }
}

impl WireCodec for AudioFragment {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.validate()?;
        let mut output = self.header.encode()?;
        output.reserve(self.data.len());
        output.extend_from_slice(&self.data);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let header = AudioFragmentHeader {
            frame_id: decoder.u32("audio frame ID")?,
            fragment_index: decoder.u16("audio fragment index")?,
            fragment_count: decoder.u16("audio fragment count")?,
        };
        let data = decoder.take_remaining().to_vec();
        let value = Self { header, data };
        value.validate()?;
        Ok(value)
    }
}
