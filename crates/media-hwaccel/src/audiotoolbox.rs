//! Direct macOS Apple AudioToolbox AAC-LC hardware/OS encoder FFI.

#![allow(non_camel_case_types, non_upper_case_globals)]

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use rust_motion_media_core::{EncodedPacket, MediaError};

type AudioConverterRef = *mut c_void;
type AudioFormatID = u32;
type AudioFormatFlags = u32;
type AudioConverterPropertyID = u32;
type OSStatus = i32;
type Boolean = u8;

const K_AUDIO_FORMAT_LINEAR_PCM: AudioFormatID = u32::from_be_bytes(*b"lpcm");
const K_AUDIO_FORMAT_MPEG4_AAC: AudioFormatID = u32::from_be_bytes(*b"aac ");
const K_AUDIO_FORMAT_FLAG_IS_FLOAT: AudioFormatFlags = 1 << 0;
const K_AUDIO_FORMAT_FLAG_IS_PACKED: AudioFormatFlags = 1 << 3;
const K_AUDIO_CONVERTER_ENCODE_BIT_RATE: AudioConverterPropertyID = u32::from_be_bytes(*b"brat");
const K_AUDIO_CONVERTER_COMPRESSION_MAGIC_COOKIE: AudioConverterPropertyID =
    u32::from_be_bytes(*b"cmgc");
const K_AUDIO_CONVERTER_PROPERTY_MAXIMUM_OUTPUT_PACKET_SIZE: AudioConverterPropertyID =
    u32::from_be_bytes(*b"xops");
const NEED_MORE_DATA_STATUS: OSStatus = 0x6D6F_7265; // 'more'

const AAC_FRAME_SAMPLES: usize = 1024;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct AudioStreamBasicDescription {
    m_sample_rate: f64,
    m_format_id: AudioFormatID,
    m_format_flags: AudioFormatFlags,
    m_bytes_per_packet: u32,
    m_frames_per_packet: u32,
    m_bytes_per_frame: u32,
    m_channels_per_frame: u32,
    m_bits_per_channel: u32,
    m_reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AudioBuffer {
    m_number_channels: u32,
    m_data_byte_size: u32,
    m_data: *mut c_void,
}

#[repr(C)]
struct AudioBufferList {
    m_number_buffers: u32,
    m_buffers: [AudioBuffer; 1],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct AudioStreamPacketDescription {
    m_start_offset: i64,
    m_variable_frames_in_packet: u32,
    m_data_byte_size: u32,
}

type AudioConverterComplexInputDataProc = unsafe extern "C" fn(
    in_audio_converter: AudioConverterRef,
    io_number_data_packets: *mut u32,
    io_data: *mut AudioBufferList,
    out_data_packet_description: *mut *mut AudioStreamPacketDescription,
    in_user_data: *mut c_void,
) -> OSStatus;

#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    fn AudioConverterNew(
        in_source_format: *const AudioStreamBasicDescription,
        in_destination_format: *const AudioStreamBasicDescription,
        out_audio_converter: *mut AudioConverterRef,
    ) -> OSStatus;
    fn AudioConverterDispose(in_audio_converter: AudioConverterRef) -> OSStatus;
    fn AudioConverterSetProperty(
        in_audio_converter: AudioConverterRef,
        in_property_id: AudioConverterPropertyID,
        in_property_data_size: u32,
        in_property_data: *const c_void,
    ) -> OSStatus;
    fn AudioConverterGetPropertyInfo(
        in_audio_converter: AudioConverterRef,
        in_property_id: AudioConverterPropertyID,
        out_size: *mut u32,
        out_writable: *mut Boolean,
    ) -> OSStatus;
    fn AudioConverterGetProperty(
        in_audio_converter: AudioConverterRef,
        in_property_id: AudioConverterPropertyID,
        io_property_data_size: *mut u32,
        out_property_data: *mut c_void,
    ) -> OSStatus;
    fn AudioConverterFillComplexBuffer(
        in_audio_converter: AudioConverterRef,
        in_input_data_proc: AudioConverterComplexInputDataProc,
        in_input_data_proc_user_data: *mut c_void,
        io_output_data_packet_size: *mut u32,
        out_output_data: *mut AudioBufferList,
        out_packet_description: *mut AudioStreamPacketDescription,
    ) -> OSStatus;
}

struct InputFeedState {
    ptr: *const f32,
    total_frames: usize,
    consumed_frames: usize,
    channels: usize,
    eof: bool,
}

unsafe extern "C" fn feed_pcm_callback(
    _in_audio_converter: AudioConverterRef,
    io_number_data_packets: *mut u32,
    io_data: *mut AudioBufferList,
    _out_data_packet_description: *mut *mut AudioStreamPacketDescription,
    in_user_data: *mut c_void,
) -> OSStatus {
    if in_user_data.is_null() || io_number_data_packets.is_null() || io_data.is_null() {
        return -50; // paramErr
    }
    let state = &mut *(in_user_data as *mut InputFeedState);
    let available = state.total_frames.saturating_sub(state.consumed_frames);
    if available == 0 {
        *io_number_data_packets = 0;
        if state.eof {
            (*io_data).m_buffers[0].m_data_byte_size = 0;
            (*io_data).m_buffers[0].m_data = null_mut();
            return 0;
        }
        return NEED_MORE_DATA_STATUS;
    }

    let req = (*io_number_data_packets as usize).max(1);
    let give = req.min(available);
    let sample_offset = state.consumed_frames * state.channels;
    let byte_len = give * state.channels * std::mem::size_of::<f32>();

    (*io_data).m_number_buffers = 1;
    (*io_data).m_buffers[0].m_number_channels = state.channels as u32;
    (*io_data).m_buffers[0].m_data_byte_size = byte_len as u32;
    (*io_data).m_buffers[0].m_data = state.ptr.add(sample_offset) as *mut c_void;
    *io_number_data_packets = give as u32;
    state.consumed_frames += give;
    0
}

/// Apple AudioToolbox AAC-LC encoder (`lpcm f32` -> raw `aac ` access units).
pub struct AtAacEncoder {
    converter: AudioConverterRef,
    sample_rate: u32,
    channels: u16,
    max_packet_bytes: usize,
    audio_specific_config: Vec<u8>,
    pending_pcm: Vec<f32>,
    next_pts: i64,
}

// SAFETY: `AtAacEncoder` owns `AudioConverterRef` uniquely and requires `&mut self` for all calls.
unsafe impl Send for AtAacEncoder {}

impl AtAacEncoder {
    pub fn new(sample_rate: u32, channels: u16, bitrate: u32) -> Result<Self, MediaError> {
        if !(1..=2).contains(&channels) {
            return Err(MediaError::InvalidConfig(format!(
                "AudioToolbox AAC encoder supports 1 or 2 channels, got {channels}"
            )));
        }
        let freq_idx = mpeg4_sample_rate_index(sample_rate).ok_or_else(|| {
            MediaError::InvalidConfig(format!(
                "unsupported AAC sample rate {sample_rate} Hz (expected e.g. 48000 or 44100)"
            ))
        })?;
        let clamped_bitrate = bitrate.max(32_000);
        let bytes_per_frame = u32::from(channels) * std::mem::size_of::<f32>() as u32;

        let in_asbd = AudioStreamBasicDescription {
            m_sample_rate: f64::from(sample_rate),
            m_format_id: K_AUDIO_FORMAT_LINEAR_PCM,
            m_format_flags: K_AUDIO_FORMAT_FLAG_IS_FLOAT | K_AUDIO_FORMAT_FLAG_IS_PACKED,
            m_bytes_per_packet: bytes_per_frame,
            m_frames_per_packet: 1,
            m_bytes_per_frame: bytes_per_frame,
            m_channels_per_frame: u32::from(channels),
            m_bits_per_channel: 32,
            m_reserved: 0,
        };
        let out_asbd = AudioStreamBasicDescription {
            m_sample_rate: f64::from(sample_rate),
            m_format_id: K_AUDIO_FORMAT_MPEG4_AAC,
            m_format_flags: 0,
            m_bytes_per_packet: 0,
            m_frames_per_packet: AAC_FRAME_SAMPLES as u32,
            m_bytes_per_frame: 0,
            m_channels_per_frame: u32::from(channels),
            m_bits_per_channel: 0,
            m_reserved: 0,
        };

        let mut converter: AudioConverterRef = null_mut();
        // SAFETY: Valid input/output ASBDs with stack output pointer.
        let status = unsafe { AudioConverterNew(&in_asbd, &out_asbd, &mut converter) };
        if status != 0 || converter.is_null() {
            return Err(MediaError::Codec(format!(
                "AudioConverterNew(AAC) failed with OSStatus {status}"
            )));
        }

        // SAFETY: Setting bitrate and querying converter properties while owning `converter`.
        unsafe {
            let _ = AudioConverterSetProperty(
                converter,
                K_AUDIO_CONVERTER_ENCODE_BIT_RATE,
                std::mem::size_of::<u32>() as u32,
                (&clamped_bitrate as *const u32).cast(),
            );
        }

        let mut max_packet_u32 = 4096u32;
        let mut prop_size = std::mem::size_of::<u32>() as u32;
        // SAFETY: Querying max output packet size into initialized `u32`.
        let xops_rc = unsafe {
            AudioConverterGetProperty(
                converter,
                K_AUDIO_CONVERTER_PROPERTY_MAXIMUM_OUTPUT_PACKET_SIZE,
                &mut prop_size,
                (&mut max_packet_u32 as *mut u32).cast(),
            )
        };
        let max_packet_bytes = if xops_rc == 0 && max_packet_u32 > 0 {
            max_packet_u32 as usize
        } else {
            4096
        };

        let asc = read_audio_specific_config(converter).unwrap_or_else(|| {
            let word = (2u16 << 11) | (u16::from(freq_idx) << 7) | (channels << 3);
            word.to_be_bytes().to_vec()
        });

        Ok(Self {
            converter,
            sample_rate,
            channels,
            max_packet_bytes,
            audio_specific_config: asc,
            pending_pcm: Vec::new(),
            next_pts: 0,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn audio_specific_config(&self) -> Vec<u8> {
        self.audio_specific_config.clone()
    }

    pub fn encode_interleaved_f32(
        &mut self,
        samples: &[f32],
    ) -> Result<Vec<EncodedPacket>, MediaError> {
        if samples.is_empty() {
            return Ok(Vec::new());
        }
        let ch = self.channels as usize;
        if !samples.len().is_multiple_of(ch) {
            return Err(MediaError::InvalidConfig(format!(
                "PCM sample count ({}) must be a multiple of channels ({ch})",
                samples.len()
            )));
        }
        self.pending_pcm.extend_from_slice(samples);
        self.drain_packets(false)
    }

    pub fn flush(&mut self) -> Result<Vec<EncodedPacket>, MediaError> {
        let ch = self.channels as usize;
        let pending_frames = self.pending_pcm.len() / ch;
        let rem = pending_frames % AAC_FRAME_SAMPLES;
        if rem != 0 {
            let pad_frames = AAC_FRAME_SAMPLES - rem;
            self.pending_pcm
                .resize(self.pending_pcm.len() + pad_frames * ch, 0.0);
        }
        self.drain_packets(true)
    }

    fn drain_packets(&mut self, eof: bool) -> Result<Vec<EncodedPacket>, MediaError> {
        let ch = self.channels as usize;
        let mut out_packets = Vec::new();
        let mut pkt_buf = vec![0u8; self.max_packet_bytes];
        let mut pkt_desc = AudioStreamPacketDescription::default();

        loop {
            let available_frames = self.pending_pcm.len() / ch;
            if !eof && available_frames < AAC_FRAME_SAMPLES {
                break;
            }
            if eof && available_frames == 0 {
                // Probe once with eof=true to drain any buffered priming packet, then stop when empty.
                let mut feed = InputFeedState {
                    ptr: null(),
                    total_frames: 0,
                    consumed_frames: 0,
                    channels: ch,
                    eof: true,
                };
                let Some(pkt) = self.pull_one_packet(&mut feed, &mut pkt_buf, &mut pkt_desc)?
                else {
                    break;
                };
                out_packets.push(pkt);
                continue;
            }

            let mut feed = InputFeedState {
                ptr: self.pending_pcm.as_ptr(),
                total_frames: available_frames,
                consumed_frames: 0,
                channels: ch,
                eof,
            };
            let maybe_pkt = self.pull_one_packet(&mut feed, &mut pkt_buf, &mut pkt_desc)?;
            if feed.consumed_frames > 0 {
                self.pending_pcm.drain(0..feed.consumed_frames * ch);
            }
            match maybe_pkt {
                Some(pkt) => out_packets.push(pkt),
                None => break,
            }
        }

        Ok(out_packets)
    }

    fn pull_one_packet(
        &mut self,
        feed: &mut InputFeedState,
        pkt_buf: &mut [u8],
        pkt_desc: &mut AudioStreamPacketDescription,
    ) -> Result<Option<EncodedPacket>, MediaError> {
        let mut io_output_packets = 1u32;
        let mut out_list = AudioBufferList {
            m_number_buffers: 1,
            m_buffers: [AudioBuffer {
                m_number_channels: u32::from(self.channels),
                m_data_byte_size: pkt_buf.len() as u32,
                m_data: pkt_buf.as_mut_ptr().cast(),
            }],
        };

        // SAFETY: `feed` outlives this synchronous FFI call and `pkt_buf` has `pkt_buf.len()` bytes.
        let status = unsafe {
            AudioConverterFillComplexBuffer(
                self.converter,
                feed_pcm_callback,
                (feed as *mut InputFeedState).cast(),
                &mut io_output_packets,
                &mut out_list,
                pkt_desc,
            )
        };
        if status != 0 && status != NEED_MORE_DATA_STATUS {
            return Err(MediaError::Codec(format!(
                "AudioConverterFillComplexBuffer failed with OSStatus {status}"
            )));
        }
        if io_output_packets == 0 {
            return Ok(None);
        }

        let start = usize::try_from(pkt_desc.m_start_offset.max(0)).unwrap_or(0);
        let size = (pkt_desc.m_data_byte_size as usize)
            .min(out_list.m_buffers[0].m_data_byte_size as usize);
        if size == 0 || start.saturating_add(size) > pkt_buf.len() {
            return Ok(None);
        }

        let pts = self.next_pts;
        self.next_pts += AAC_FRAME_SAMPLES as i64;
        Ok(Some(EncodedPacket {
            stream_index: 1,
            data: pkt_buf[start..start + size].to_vec(),
            pts,
            dts: pts,
            duration: AAC_FRAME_SAMPLES as i64,
            is_keyframe: true,
        }))
    }
}

impl Drop for AtAacEncoder {
    fn drop(&mut self) {
        if !self.converter.is_null() {
            // SAFETY: `self` uniquely owns `self.converter`.
            unsafe {
                let _ = AudioConverterDispose(self.converter);
            }
            self.converter = null_mut();
        }
    }
}

fn read_audio_specific_config(converter: AudioConverterRef) -> Option<Vec<u8>> {
    let mut size = 0u32;
    let mut writable = 0u8;
    // SAFETY: Queries size of optional compression magic cookie property.
    let info_rc = unsafe {
        AudioConverterGetPropertyInfo(
            converter,
            K_AUDIO_CONVERTER_COMPRESSION_MAGIC_COOKIE,
            &mut size,
            &mut writable,
        )
    };
    if info_rc != 0 || size == 0 {
        return None;
    }
    let mut cookie = vec![0u8; size as usize];
    // SAFETY: Reads `size` bytes into allocated cookie buffer.
    let get_rc = unsafe {
        AudioConverterGetProperty(
            converter,
            K_AUDIO_CONVERTER_COMPRESSION_MAGIC_COOKIE,
            &mut size,
            cookie.as_mut_ptr().cast(),
        )
    };
    if get_rc != 0 {
        return None;
    }
    cookie.truncate(size as usize);
    extract_asc_from_esds_payload(&cookie)
}

/// Extract tag `0x05` DecoderSpecificInfo bytes (`AudioSpecificConfig`) from an MPEG-4 ESDS blob.
fn extract_asc_from_esds_payload(esds: &[u8]) -> Option<Vec<u8>> {
    fn read_desc_size(bytes: &[u8], pos: &mut usize) -> Option<usize> {
        let mut len = 0usize;
        for _ in 0..4 {
            let b = *bytes.get(*pos)?;
            *pos += 1;
            len = (len << 7) | usize::from(b & 0x7F);
            if (b & 0x80) == 0 {
                return Some(len);
            }
        }
        Some(len)
    }

    let mut i = 0usize;
    while i < esds.len() {
        let tag = esds[i];
        i += 1;
        let len = read_desc_size(esds, &mut i)?;
        match tag {
            0x03 => {
                // Skip ES_ID (2B) + flags (1B) [+ optional depends/URL/OCR if flags set]
                let flags = *esds.get(i + 2)?;
                i += 3;
                if (flags & 0x80) != 0 {
                    i += 2;
                }
                if (flags & 0x40) != 0 {
                    let url_len = usize::from(*esds.get(i)?);
                    i += 1 + url_len;
                }
                if (flags & 0x20) != 0 {
                    i += 2;
                }
            }
            0x04 => {
                // Skip DecoderConfigDescriptor fixed 13 bytes
                i += 13;
            }
            0x05 => {
                let end = i.checked_add(len)?;
                return esds.get(i..end).map(|s| s.to_vec());
            }
            _ => {
                i = i.checked_add(len)?;
            }
        }
    }
    None
}

const fn mpeg4_sample_rate_index(rate: u32) -> Option<u8> {
    match rate {
        96_000 => Some(0),
        88_200 => Some(1),
        64_000 => Some(2),
        48_000 => Some(3),
        44_100 => Some(4),
        32_000 => Some(5),
        24_000 => Some(6),
        22_050 => Some(7),
        16_000 => Some(8),
        12_000 => Some(9),
        11_025 => Some(10),
        8_000 => Some(11),
        _ => None,
    }
}
