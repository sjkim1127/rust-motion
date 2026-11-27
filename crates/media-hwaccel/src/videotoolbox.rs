//! Direct macOS Apple VideoToolbox hardware encoder and decoder FFI.

#![allow(non_camel_case_types, non_upper_case_globals)]

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::Mutex;

use rust_motion_media_core::{
    CodecCapabilities, ColorPrimaries, ColorRange, EncodedPacket, MatrixCoefficients, MediaError,
    PixelFormat, TransferCharacteristics, VideoCodecId, VideoEncodeConfig, VideoFrame,
    VideoFrameBuffer,
};

use crate::DecodedVideoFrame;

type CFTypeRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFStringRef = *const c_void;
type CFDataRef = *const c_void;
type CFArrayRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFNumberRef = *const c_void;
type CFBooleanRef = *const c_void;
type CFPropertyListRef = *const c_void;
type CFIndex = isize;
type CFNumberType = isize;
type CFStringEncoding = u32;
type Boolean = u8;

type OSStatus = i32;
type CVReturn = i32;
type OSType = u32;
type CVOptionFlags = u64;
type CMItemCount = isize;
type FourCharCode = u32;
type CMVideoCodecType = FourCharCode;

type CVImageBufferRef = *mut c_void;
type CVPixelBufferRef = *mut c_void;
type CVPixelBufferPoolRef = *mut c_void;
type CMFormatDescriptionRef = *const c_void;
type CMVideoFormatDescriptionRef = *const c_void;
type CMBlockBufferRef = *mut c_void;
type CMSampleBufferRef = *mut c_void;
type VTSessionRef = *mut c_void;
type VTCompressionSessionRef = *mut c_void;
type VTDecompressionSessionRef = *mut c_void;
type VTDecodeFrameFlags = u32;
type VTDecodeInfoFlags = u32;
type VTEncodeInfoFlags = u32;

const K_CF_STRING_ENCODING_UTF8: CFStringEncoding = 0x0800_0100;
const K_CF_NUMBER_SINT32_TYPE: CFNumberType = 3;
const K_CF_NUMBER_FLOAT32_TYPE: CFNumberType = 5;
const K_CF_NUMBER_FLOAT64_TYPE: CFNumberType = 6;
const K_CV_PIXEL_FORMAT_TYPE_32BGRA: OSType = u32::from_be_bytes(*b"BGRA");
const K_CM_VIDEO_CODEC_TYPE_H264: CMVideoCodecType = u32::from_be_bytes(*b"avc1");
const K_CM_VIDEO_CODEC_TYPE_HEVC: CMVideoCodecType = u32::from_be_bytes(*b"hvc1");
const K_CM_BLOCK_BUFFER_ASSURE_MEMORY_NOW_FLAG: u32 = 1 << 0;
const K_CV_PIXEL_BUFFER_LOCK_READ_ONLY: CVOptionFlags = 1;
const K_CM_TIME_FLAGS_VALID: u32 = 1 << 0;

#[repr(C)]
#[derive(Clone, Copy)]
struct CMTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

impl CMTime {
    const fn new(value: i64, timescale: i32) -> Self {
        Self {
            value,
            timescale,
            flags: K_CM_TIME_FLAGS_VALID,
            epoch: 0,
        }
    }

    fn is_valid(self) -> bool {
        (self.flags & K_CM_TIME_FLAGS_VALID) != 0
    }
}

#[repr(C)]
struct CMSampleTimingInfo {
    duration: CMTime,
    presentation_time_stamp: CMTime,
    decode_time_stamp: CMTime,
}

#[repr(C)]
struct VTDecompressionOutputCallbackRecord {
    decompression_output_callback: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            OSStatus,
            VTDecodeInfoFlags,
            CVImageBufferRef,
            CMTime,
            CMTime,
        ),
    >,
    decompression_output_ref_con: *mut c_void,
}

#[repr(C)]
struct OpaqueCfCallBacks([u8; 64]);

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFTypeDictionaryKeyCallBacks: OpaqueCfCallBacks;
    static kCFTypeDictionaryValueCallBacks: OpaqueCfCallBacks;
    static kCFBooleanTrue: CFBooleanRef;
    static kCFBooleanFalse: CFBooleanRef;

    fn CFRelease(cf: CFTypeRef);
    fn CFStringCreateWithBytes(
        alloc: CFAllocatorRef,
        bytes: *const u8,
        num_bytes: CFIndex,
        encoding: CFStringEncoding,
        is_external_representation: Boolean,
    ) -> CFStringRef;
    fn CFDataCreate(alloc: CFAllocatorRef, bytes: *const u8, length: CFIndex) -> CFDataRef;
    fn CFDataGetLength(the_data: CFDataRef) -> CFIndex;
    fn CFDataGetBytePtr(the_data: CFDataRef) -> *const u8;
    fn CFNumberCreate(
        allocator: CFAllocatorRef,
        the_type: CFNumberType,
        value_ptr: *const c_void,
    ) -> CFNumberRef;
    fn CFDictionaryCreate(
        allocator: CFAllocatorRef,
        keys: *const *const c_void,
        values: *const *const c_void,
        num_values: CFIndex,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFDictionaryRef;
    fn CFDictionaryGetValue(the_dict: CFDictionaryRef, key: *const c_void) -> *const c_void;
    fn CFArrayGetCount(the_array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(the_array: CFArrayRef, idx: CFIndex) -> *const c_void;
}

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    static kCVPixelBufferPixelFormatTypeKey: CFStringRef;
    static kCVPixelBufferWidthKey: CFStringRef;
    static kCVPixelBufferHeightKey: CFStringRef;
    static kCVPixelBufferIOSurfacePropertiesKey: CFStringRef;
    static kCVImageBufferColorPrimaries_ITU_R_709_2: CFStringRef;
    static kCVImageBufferColorPrimaries_ITU_R_2020: CFStringRef;
    static kCVImageBufferColorPrimaries_SMPTE_C: CFStringRef;
    static kCVImageBufferTransferFunction_ITU_R_709_2: CFStringRef;
    static kCVImageBufferYCbCrMatrix_ITU_R_709_2: CFStringRef;
    static kCVImageBufferYCbCrMatrix_ITU_R_601_4: CFStringRef;
    static kCVImageBufferYCbCrMatrix_ITU_R_2020: CFStringRef;

    fn CVPixelBufferCreate(
        allocator: CFAllocatorRef,
        width: usize,
        height: usize,
        pixel_format_type: OSType,
        pixel_buffer_attributes: CFDictionaryRef,
        pixel_buffer_out: *mut CVPixelBufferRef,
    ) -> CVReturn;
    fn CVPixelBufferPoolCreatePixelBuffer(
        allocator: CFAllocatorRef,
        pixel_buffer_pool: CVPixelBufferPoolRef,
        pixel_buffer_out: *mut CVPixelBufferRef,
    ) -> CVReturn;
    fn CVPixelBufferLockBaseAddress(
        pixel_buffer: CVPixelBufferRef,
        lock_flags: CVOptionFlags,
    ) -> CVReturn;
    fn CVPixelBufferUnlockBaseAddress(
        pixel_buffer: CVPixelBufferRef,
        unlock_flags: CVOptionFlags,
    ) -> CVReturn;
    fn CVPixelBufferGetWidth(pixel_buffer: CVPixelBufferRef) -> usize;
    fn CVPixelBufferGetHeight(pixel_buffer: CVPixelBufferRef) -> usize;
    fn CVPixelBufferGetBytesPerRow(pixel_buffer: CVPixelBufferRef) -> usize;
    fn CVPixelBufferGetBaseAddress(pixel_buffer: CVPixelBufferRef) -> *mut c_void;
}

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    static kCMTimeInvalid: CMTime;
    static kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms: CFStringRef;
    static kCMSampleAttachmentKey_NotSync: CFStringRef;

    fn CMVideoFormatDescriptionCreate(
        allocator: CFAllocatorRef,
        codec_type: CMVideoCodecType,
        width: i32,
        height: i32,
        extensions: CFDictionaryRef,
        format_description_out: *mut CMVideoFormatDescriptionRef,
    ) -> OSStatus;
    fn CMFormatDescriptionGetExtension(
        desc: CMFormatDescriptionRef,
        extension_key: CFStringRef,
    ) -> CFPropertyListRef;
    fn CMBlockBufferCreateWithMemoryBlock(
        structure_allocator: CFAllocatorRef,
        memory_block: *mut c_void,
        block_length: usize,
        block_allocator: CFAllocatorRef,
        custom_block_source: *const c_void,
        offset_to_data: usize,
        data_length: usize,
        flags: u32,
        block_buffer_out: *mut CMBlockBufferRef,
    ) -> OSStatus;
    fn CMBlockBufferReplaceDataBytes(
        source_bytes: *const c_void,
        destination_buffer: CMBlockBufferRef,
        offset_into_destination: usize,
        data_length: usize,
    ) -> OSStatus;
    fn CMBlockBufferGetDataLength(the_buffer: CMBlockBufferRef) -> usize;
    fn CMBlockBufferCopyDataBytes(
        the_source_buffer: CMBlockBufferRef,
        offset_to_data: usize,
        data_length: usize,
        destination: *mut c_void,
    ) -> OSStatus;
    fn CMSampleBufferCreateReady(
        allocator: CFAllocatorRef,
        data_buffer: CMBlockBufferRef,
        format_description: CMFormatDescriptionRef,
        num_samples: CMItemCount,
        num_sample_timing_entries: CMItemCount,
        sample_timing_array: *const CMSampleTimingInfo,
        num_sample_size_entries: CMItemCount,
        sample_size_array: *const usize,
        sample_buffer_out: *mut CMSampleBufferRef,
    ) -> OSStatus;
    fn CMSampleBufferGetFormatDescription(sbuf: CMSampleBufferRef) -> CMFormatDescriptionRef;
    fn CMSampleBufferGetDataBuffer(sbuf: CMSampleBufferRef) -> CMBlockBufferRef;
    fn CMSampleBufferGetPresentationTimeStamp(sbuf: CMSampleBufferRef) -> CMTime;
    fn CMSampleBufferGetDecodeTimeStamp(sbuf: CMSampleBufferRef) -> CMTime;
    fn CMSampleBufferGetDuration(sbuf: CMSampleBufferRef) -> CMTime;
    fn CMSampleBufferGetSampleAttachmentsArray(
        sbuf: CMSampleBufferRef,
        create_if_necessary: Boolean,
    ) -> CFArrayRef;
}

#[link(name = "VideoToolbox", kind = "framework")]
extern "C" {
    static kVTCompressionPropertyKey_RealTime: CFStringRef;
    static kVTCompressionPropertyKey_AllowFrameReordering: CFStringRef;
    static kVTCompressionPropertyKey_ExpectedFrameRate: CFStringRef;
    static kVTCompressionPropertyKey_MaxKeyFrameInterval: CFStringRef;
    static kVTCompressionPropertyKey_Quality: CFStringRef;
    static kVTCompressionPropertyKey_ColorPrimaries: CFStringRef;
    static kVTCompressionPropertyKey_TransferFunction: CFStringRef;
    static kVTCompressionPropertyKey_YCbCrMatrix: CFStringRef;

    fn VTSessionSetProperty(
        session: VTSessionRef,
        property_key: CFStringRef,
        property_value: CFTypeRef,
    ) -> OSStatus;
    fn VTCompressionSessionCreate(
        allocator: CFAllocatorRef,
        width: i32,
        height: i32,
        codec_type: CMVideoCodecType,
        encoder_specification: CFDictionaryRef,
        source_image_buffer_attributes: CFDictionaryRef,
        compressed_data_allocator: CFAllocatorRef,
        output_callback: Option<
            unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                OSStatus,
                VTEncodeInfoFlags,
                CMSampleBufferRef,
            ),
        >,
        output_callback_ref_con: *mut c_void,
        compression_session_out: *mut VTCompressionSessionRef,
    ) -> OSStatus;
    fn VTCompressionSessionPrepareToEncodeFrames(session: VTCompressionSessionRef) -> OSStatus;
    fn VTCompressionSessionGetPixelBufferPool(
        session: VTCompressionSessionRef,
    ) -> CVPixelBufferPoolRef;
    fn VTCompressionSessionEncodeFrame(
        session: VTCompressionSessionRef,
        image_buffer: CVImageBufferRef,
        presentation_time_stamp: CMTime,
        duration: CMTime,
        frame_properties: CFDictionaryRef,
        source_frame_refcon: *mut c_void,
        info_flags_out: *mut VTEncodeInfoFlags,
    ) -> OSStatus;
    fn VTCompressionSessionCompleteFrames(
        session: VTCompressionSessionRef,
        complete_until_pts: CMTime,
    ) -> OSStatus;
    fn VTCompressionSessionInvalidate(session: VTCompressionSessionRef);

    fn VTDecompressionSessionCreate(
        allocator: CFAllocatorRef,
        video_format_description: CMVideoFormatDescriptionRef,
        video_decoder_specification: CFDictionaryRef,
        destination_image_buffer_attributes: CFDictionaryRef,
        output_callback: *const VTDecompressionOutputCallbackRecord,
        decompression_session_out: *mut VTDecompressionSessionRef,
    ) -> OSStatus;
    fn VTDecompressionSessionDecodeFrame(
        session: VTDecompressionSessionRef,
        sample_buffer: CMSampleBufferRef,
        decode_flags: VTDecodeFrameFlags,
        source_frame_ref_con: *mut c_void,
        info_flags_out: *mut VTDecodeInfoFlags,
    ) -> OSStatus;
    fn VTDecompressionSessionWaitForAsynchronousFrames(
        session: VTDecompressionSessionRef,
    ) -> OSStatus;
    fn VTDecompressionSessionInvalidate(session: VTDecompressionSessionRef);
}

struct CfGuard(CFTypeRef);

impl CfGuard {
    fn from_owned(ptr: CFTypeRef) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            Some(Self(ptr))
        }
    }

    fn as_ptr(&self) -> CFTypeRef {
        self.0
    }
}

impl Drop for CfGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: `CfGuard` strictly owns a non-null CoreFoundation retained object.
            unsafe { CFRelease(self.0) };
        }
    }
}

fn cf_string(s: &str) -> Result<CfGuard, MediaError> {
    // SAFETY: `s.as_ptr()` is valid for `s.len()` bytes with UTF-8 encoding.
    let ptr = unsafe {
        CFStringCreateWithBytes(
            null(),
            s.as_ptr(),
            s.len() as CFIndex,
            K_CF_STRING_ENCODING_UTF8,
            0,
        )
    };
    CfGuard::from_owned(ptr)
        .ok_or_else(|| MediaError::Codec("CFStringCreateWithBytes returned null".into()))
}

fn cf_data(bytes: &[u8]) -> Result<CfGuard, MediaError> {
    // SAFETY: `bytes.as_ptr()` is valid for `bytes.len()` bytes and copied by CoreFoundation.
    let ptr = unsafe { CFDataCreate(null(), bytes.as_ptr(), bytes.len() as CFIndex) };
    CfGuard::from_owned(ptr).ok_or_else(|| MediaError::Codec("CFDataCreate returned null".into()))
}

fn cf_num_i32(val: i32) -> Result<CfGuard, MediaError> {
    // SAFETY: `val` is initialized on the stack and read synchronously by `CFNumberCreate`.
    let ptr =
        unsafe { CFNumberCreate(null(), K_CF_NUMBER_SINT32_TYPE, (&val as *const i32).cast()) };
    CfGuard::from_owned(ptr)
        .ok_or_else(|| MediaError::Codec("CFNumberCreate(i32) returned null".into()))
}

fn cf_num_f32(val: f32) -> Result<CfGuard, MediaError> {
    // SAFETY: `val` is initialized on the stack and read synchronously by `CFNumberCreate`.
    let ptr = unsafe {
        CFNumberCreate(
            null(),
            K_CF_NUMBER_FLOAT32_TYPE,
            (&val as *const f32).cast(),
        )
    };
    CfGuard::from_owned(ptr)
        .ok_or_else(|| MediaError::Codec("CFNumberCreate(f32) returned null".into()))
}

fn cf_num_f64(val: f64) -> Result<CfGuard, MediaError> {
    // SAFETY: `val` is initialized on the stack and read synchronously by `CFNumberCreate`.
    let ptr = unsafe {
        CFNumberCreate(
            null(),
            K_CF_NUMBER_FLOAT64_TYPE,
            (&val as *const f64).cast(),
        )
    };
    CfGuard::from_owned(ptr)
        .ok_or_else(|| MediaError::Codec("CFNumberCreate(f64) returned null".into()))
}

fn cf_dict(pairs: &[(CFTypeRef, CFTypeRef)]) -> Result<CfGuard, MediaError> {
    let keys: Vec<*const c_void> = pairs.iter().map(|(k, _)| *k).collect();
    let vals: Vec<*const c_void> = pairs.iter().map(|(_, v)| *v).collect();
    // SAFETY: keys/vals slices and standard CoreFoundation callbacks outlive this call.
    let ptr = unsafe {
        CFDictionaryCreate(
            null(),
            keys.as_ptr(),
            vals.as_ptr(),
            pairs.len() as CFIndex,
            (&raw const kCFTypeDictionaryKeyCallBacks).cast(),
            (&raw const kCFTypeDictionaryValueCallBacks).cast(),
        )
    };
    CfGuard::from_owned(ptr)
        .ok_or_else(|| MediaError::Codec("CFDictionaryCreate returned null".into()))
}

fn vt_codec_type(codec: VideoCodecId) -> Result<(CMVideoCodecType, &'static str), MediaError> {
    match codec {
        VideoCodecId::H264 => Ok((K_CM_VIDEO_CODEC_TYPE_H264, "avcC")),
        VideoCodecId::Hevc => Ok((K_CM_VIDEO_CODEC_TYPE_HEVC, "hvcC")),
        other => Err(MediaError::Unsupported(format!(
            "VideoToolbox does not support codec {other:?}"
        ))),
    }
}

#[derive(Default)]
struct EncodeOutputState {
    atom_name: &'static str,
    codec_config: Vec<u8>,
    packets: Vec<EncodedPacket>,
    first_error: Option<String>,
}

/// Apple VideoToolbox hardware video encoder (`H.264` and `HEVC`).
pub struct VtEncoder {
    codec: VideoCodecId,
    atom_name: &'static str,
    session: Option<VTCompressionSessionRef>,
    state: *mut Mutex<EncodeOutputState>,
    timescale: i32,
    width: u32,
    height: u32,
}

// SAFETY: `VTCompressionSessionRef` supports serialized access via `&mut self`,
// and callback state is synchronized under a `Mutex`.
unsafe impl Send for VtEncoder {}

impl VtEncoder {
    pub fn new(codec: VideoCodecId) -> Result<Self, MediaError> {
        let (_, atom_name) = vt_codec_type(codec)?;
        let state = Box::into_raw(Box::new(Mutex::new(EncodeOutputState {
            atom_name,
            ..EncodeOutputState::default()
        })));
        Ok(Self {
            codec,
            atom_name,
            session: None,
            state,
            timescale: 0,
            width: 0,
            height: 0,
        })
    }

    pub fn capabilities(&self) -> CodecCapabilities {
        CodecCapabilities {
            codec: self.codec,
            hardware_accelerated: true,
            supports_bframes: false,
            supported_pixel_formats: vec![PixelFormat::Rgba8, PixelFormat::Yuv420p],
        }
    }

    pub fn configure(&mut self, config: &VideoEncodeConfig) -> Result<(), MediaError> {
        let (codec_type, atom_name) = vt_codec_type(config.codec)?;
        if config.width == 0 || config.height == 0 || config.fps.num == 0 {
            return Err(MediaError::InvalidConfig(
                "VideoToolbox encoder width, height, and fps must be > 0".into(),
            ));
        }

        self.teardown_session();
        self.codec = config.codec;
        self.atom_name = atom_name;
        self.width = config.width;
        self.height = config.height;
        self.timescale = i32::try_from(config.fps.num).map_err(|_| {
            MediaError::InvalidConfig("VideoToolbox timescale exceeds i32::MAX".into())
        })?;
        // SAFETY: `self.state` remains uniquely owned until `Drop`.
        unsafe {
            let mut guard = (*self.state)
                .lock()
                .map_err(|_| MediaError::Codec("encoder state lock poisoned".into()))?;
            guard.atom_name = atom_name;
            guard.codec_config.clear();
            guard.packets.clear();
            guard.first_error = None;
        }

        let fmt_num = cf_num_i32(K_CV_PIXEL_FORMAT_TYPE_32BGRA as i32)?;
        let width_num = cf_num_i32(config.width as i32)?;
        let height_num = cf_num_i32(config.height as i32)?;
        let empty_dict = cf_dict(&[])?;
        // SAFETY: External CVPixelBuffer attribute key symbols come from CoreVideo framework.
        let src_attrs = unsafe {
            cf_dict(&[
                (kCVPixelBufferPixelFormatTypeKey, fmt_num.as_ptr()),
                (kCVPixelBufferWidthKey, width_num.as_ptr()),
                (kCVPixelBufferHeightKey, height_num.as_ptr()),
                (kCVPixelBufferIOSurfacePropertiesKey, empty_dict.as_ptr()),
            ])?
        };

        let mut session: VTCompressionSessionRef = null_mut();
        // SAFETY: Creating session with valid callback and pinned `state` pointer.
        let status = unsafe {
            VTCompressionSessionCreate(
                null(),
                config.width as i32,
                config.height as i32,
                codec_type,
                null(),
                src_attrs.as_ptr(),
                null(),
                Some(vt_compression_output_callback),
                self.state.cast(),
                &mut session,
            )
        };
        if status != 0 || session.is_null() {
            return Err(MediaError::Codec(format!(
                "VTCompressionSessionCreate failed with OSStatus {status}"
            )));
        }
        self.session = Some(session);

        // Configure deterministic offline encoding properties on the session.
        let fps_val = config.fps.as_f64().max(1.0);
        let fps_cf = cf_num_f64(fps_val)?;
        let gop_frames = (fps_val.round() as i32 * 2).max(1);
        let gop_cf = cf_num_i32(gop_frames)?;
        let quality = ((51.0 - f32::from(config.crf.min(51) as u16)) / 51.0).clamp(0.1, 1.0);
        let quality_cf = cf_num_f32(quality)?;

        // SAFETY: All `kVTCompressionPropertyKey_*`, `kCFBoolean*`, and `kCVImageBuffer*` symbols are
        // valid framework constants; return statuses for optional hints do not leave invalid state.
        unsafe {
            let _ =
                VTSessionSetProperty(session, kVTCompressionPropertyKey_RealTime, kCFBooleanFalse);
            let _ = VTSessionSetProperty(
                session,
                kVTCompressionPropertyKey_AllowFrameReordering,
                kCFBooleanFalse,
            );
            let _ = VTSessionSetProperty(
                session,
                kVTCompressionPropertyKey_ExpectedFrameRate,
                fps_cf.as_ptr(),
            );
            let _ = VTSessionSetProperty(
                session,
                kVTCompressionPropertyKey_MaxKeyFrameInterval,
                gop_cf.as_ptr(),
            );
            let _ = VTSessionSetProperty(
                session,
                kVTCompressionPropertyKey_Quality,
                quality_cf.as_ptr(),
            );
            let primaries = match config.color.primaries {
                ColorPrimaries::Bt2020 => kCVImageBufferColorPrimaries_ITU_R_2020,
                ColorPrimaries::Bt601Ntsc | ColorPrimaries::Bt601Pal => {
                    kCVImageBufferColorPrimaries_SMPTE_C
                }
                _ => kCVImageBufferColorPrimaries_ITU_R_709_2,
            };
            let matrix = match config.color.matrix {
                MatrixCoefficients::Bt2020Ncl => kCVImageBufferYCbCrMatrix_ITU_R_2020,
                MatrixCoefficients::Bt601 => kCVImageBufferYCbCrMatrix_ITU_R_601_4,
                _ => kCVImageBufferYCbCrMatrix_ITU_R_709_2,
            };
            let transfer = match config.color.transfer {
                TransferCharacteristics::Bt709 | TransferCharacteristics::Srgb => {
                    kCVImageBufferTransferFunction_ITU_R_709_2
                }
                _ => kCVImageBufferTransferFunction_ITU_R_709_2,
            };
            let _ =
                VTSessionSetProperty(session, kVTCompressionPropertyKey_ColorPrimaries, primaries);
            let _ = VTSessionSetProperty(
                session,
                kVTCompressionPropertyKey_TransferFunction,
                transfer,
            );
            let _ = VTSessionSetProperty(session, kVTCompressionPropertyKey_YCbCrMatrix, matrix);

            let prep = VTCompressionSessionPrepareToEncodeFrames(session);
            if prep != 0 {
                return Err(MediaError::Codec(format!(
                    "VTCompressionSessionPrepareToEncodeFrames failed with OSStatus {prep}"
                )));
            }
        }

        Ok(())
    }

    pub fn codec_config(&self) -> Vec<u8> {
        // SAFETY: `self.state` is alive for the lifetime of `self`.
        unsafe {
            (*self.state)
                .lock()
                .map(|g| g.codec_config.clone())
                .unwrap_or_default()
        }
    }

    pub fn encode(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, MediaError> {
        let session = self.session.ok_or_else(|| {
            MediaError::InvalidConfig("VtEncoder::configure must be called first".into())
        })?;
        if frame.width != self.width || frame.height != self.height {
            return Err(MediaError::InvalidConfig(format!(
                "frame dimensions {}x{} do not match configured {}x{}",
                frame.width, frame.height, self.width, self.height
            )));
        }

        let pixel_buf = self.acquire_bgra_pixel_buffer(session)?;
        let _pb_guard = CfGuard::from_owned(pixel_buf.cast());
        self.write_video_frame_to_bgra_pixel_buffer(pixel_buf, frame)?;

        let pts = CMTime::new(frame.pts, self.timescale);
        let dur = CMTime::new(frame.duration.max(1), self.timescale);
        // SAFETY: Passing valid session and locked/unlocked `CVPixelBuffer` with finite CMTime.
        let status = unsafe {
            VTCompressionSessionEncodeFrame(
                session,
                pixel_buf,
                pts,
                dur,
                null(),
                null_mut(),
                null_mut(),
            )
        };
        if status != 0 {
            return Err(MediaError::Codec(format!(
                "VTCompressionSessionEncodeFrame failed with OSStatus {status}"
            )));
        }

        self.drain_packets()
    }

    pub fn flush(&mut self) -> Result<Vec<EncodedPacket>, MediaError> {
        let Some(session) = self.session else {
            return Ok(Vec::new());
        };
        // SAFETY: Passing `kCMTimeInvalid` flushes every pending frame synchronously before returning.
        let status = unsafe { VTCompressionSessionCompleteFrames(session, kCMTimeInvalid) };
        if status != 0 {
            return Err(MediaError::Codec(format!(
                "VTCompressionSessionCompleteFrames failed with OSStatus {status}"
            )));
        }
        self.drain_packets()
    }

    fn drain_packets(&self) -> Result<Vec<EncodedPacket>, MediaError> {
        // SAFETY: `self.state` is pinned and synchronized with `Mutex`.
        let mut guard = unsafe {
            (*self.state)
                .lock()
                .map_err(|_| MediaError::Codec("encoder state lock poisoned".into()))?
        };
        if let Some(err) = guard.first_error.take() {
            return Err(MediaError::Codec(err));
        }
        Ok(std::mem::take(&mut guard.packets))
    }

    fn acquire_bgra_pixel_buffer(
        &self,
        session: VTCompressionSessionRef,
    ) -> Result<CVPixelBufferRef, MediaError> {
        let mut pb: CVPixelBufferRef = null_mut();
        // SAFETY: Obtaining a pixel buffer from VideoToolbox's IOSurface-backed pool when available.
        unsafe {
            let pool = VTCompressionSessionGetPixelBufferPool(session);
            if !pool.is_null() {
                let rc = CVPixelBufferPoolCreatePixelBuffer(null(), pool, &mut pb);
                if rc == 0 && !pb.is_null() {
                    return Ok(pb);
                }
            }
            let empty_dict = cf_dict(&[])?;
            let rc = CVPixelBufferCreate(
                null(),
                self.width as usize,
                self.height as usize,
                K_CV_PIXEL_FORMAT_TYPE_32BGRA,
                empty_dict.as_ptr(),
                &mut pb,
            );
            if rc != 0 || pb.is_null() {
                return Err(MediaError::Codec(format!(
                    "CVPixelBufferCreate failed with CVReturn {rc}"
                )));
            }
        }
        Ok(pb)
    }

    fn write_video_frame_to_bgra_pixel_buffer(
        &self,
        pb: CVPixelBufferRef,
        frame: &VideoFrame,
    ) -> Result<(), MediaError> {
        let w = self.width as usize;
        let h = self.height as usize;

        // SAFETY: Lock pixel buffer base address exclusively for CPU write, then always unlock.
        unsafe {
            let lock_rc = CVPixelBufferLockBaseAddress(pb, 0);
            if lock_rc != 0 {
                return Err(MediaError::Codec(format!(
                    "CVPixelBufferLockBaseAddress failed with CVReturn {lock_rc}"
                )));
            }
            let bpr = CVPixelBufferGetBytesPerRow(pb);
            let base = CVPixelBufferGetBaseAddress(pb) as *mut u8;
            if base.is_null() || bpr < w * 4 {
                CVPixelBufferUnlockBaseAddress(pb, 0);
                return Err(MediaError::Codec(
                    "invalid CVPixelBuffer base address or stride".into(),
                ));
            }
            let dst = std::slice::from_raw_parts_mut(base, bpr * h);

            match &frame.buffer {
                VideoFrameBuffer::CpuRgba(rgba) => {
                    let expected = w * h * 4;
                    if rgba.len() < expected {
                        CVPixelBufferUnlockBaseAddress(pb, 0);
                        return Err(MediaError::InvalidConfig(
                            "RGBA frame buffer smaller than configured dimensions".into(),
                        ));
                    }
                    for y in 0..h {
                        let src_row = &rgba[y * w * 4..(y + 1) * w * 4];
                        let dst_row = &mut dst[y * bpr..y * bpr + w * 4];
                        for (s, d) in src_row
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .zip(dst_row.as_chunks_mut::<4>().0.iter_mut())
                        {
                            let a = s[3];
                            if a == 255 {
                                d[0] = s[2];
                                d[1] = s[1];
                                d[2] = s[0];
                            } else {
                                let af = u16::from(a);
                                d[0] = ((u16::from(s[2]) * af + 127) / 255) as u8;
                                d[1] = ((u16::from(s[1]) * af + 127) / 255) as u8;
                                d[2] = ((u16::from(s[0]) * af + 127) / 255) as u8;
                            }
                            d[3] = 255;
                        }
                    }
                }
                VideoFrameBuffer::CpuYuv420 {
                    y: y_plane,
                    u: u_plane,
                    v: v_plane,
                    y_stride,
                    uv_stride,
                } => {
                    let limited = frame.color.range != ColorRange::Full;
                    let (kr, kb) = match frame.color.matrix {
                        MatrixCoefficients::Bt601 => (0.2990f32, 0.1140f32),
                        MatrixCoefficients::Bt2020Ncl => (0.2627f32, 0.0593f32),
                        _ => (0.2126f32, 0.0722f32),
                    };
                    let kg = 1.0 - kr - kb;
                    let cr_r = 2.0 * (1.0 - kr);
                    let cb_b = 2.0 * (1.0 - kb);
                    let cr_g = -2.0 * kr * (1.0 - kr) / kg;
                    let cb_g = -2.0 * kb * (1.0 - kb) / kg;

                    for row in 0..h {
                        let cy = row >> 1;
                        let dst_row = &mut dst[row * bpr..row * bpr + w * 4];
                        for col in 0..w {
                            let cx = col >> 1;
                            let yv = f32::from(y_plane[row * *y_stride + col]);
                            let uv = f32::from(u_plane[cy * *uv_stride + cx]);
                            let vv = f32::from(v_plane[cy * *uv_stride + cx]);
                            let (yl, cb, cr) = if limited {
                                (
                                    (yv - 16.0) / 219.0,
                                    (uv - 128.0) / 224.0,
                                    (vv - 128.0) / 224.0,
                                )
                            } else {
                                (yv / 255.0, (uv - 128.0) / 255.0, (vv - 128.0) / 255.0)
                            };
                            let r = ((yl + cr_r * cr).clamp(0.0, 1.0) * 255.0).round() as u8;
                            let g = ((yl + cr_g * cr + cb_g * cb).clamp(0.0, 1.0) * 255.0).round()
                                as u8;
                            let b = ((yl + cb_b * cb).clamp(0.0, 1.0) * 255.0).round() as u8;
                            let d = &mut dst_row[col * 4..col * 4 + 4];
                            d[0] = b;
                            d[1] = g;
                            d[2] = r;
                            d[3] = 255;
                        }
                    }
                }
            }

            CVPixelBufferUnlockBaseAddress(pb, 0);
        }
        Ok(())
    }

    fn teardown_session(&mut self) {
        if let Some(session) = self.session.take() {
            // SAFETY: Invalidate waits for asynchronous callbacks to complete before returning.
            unsafe {
                VTCompressionSessionInvalidate(session);
                CFRelease(session.cast());
            }
        }
    }
}

impl Drop for VtEncoder {
    fn drop(&mut self) {
        self.teardown_session();
        if !self.state.is_null() {
            // SAFETY: Session is invalidated above, so no VideoToolbox callback can touch `state`.
            unsafe {
                drop(Box::from_raw(self.state));
            }
            self.state = null_mut();
        }
    }
}

unsafe extern "C" fn vt_compression_output_callback(
    output_callback_ref_con: *mut c_void,
    _source_frame_refcon: *mut c_void,
    status: OSStatus,
    _info_flags: VTEncodeInfoFlags,
    sample_buffer: CMSampleBufferRef,
) {
    if output_callback_ref_con.is_null() {
        return;
    }
    let state_mutex = &*(output_callback_ref_con as *const Mutex<EncodeOutputState>);
    let Ok(mut state) = state_mutex.lock() else {
        return;
    };

    if status != 0 {
        state
            .first_error
            .get_or_insert_with(|| format!("VideoToolbox compression error: OSStatus {status}"));
        return;
    }
    if sample_buffer.is_null() {
        return;
    }

    if state.codec_config.is_empty() {
        let fmt_desc = CMSampleBufferGetFormatDescription(sample_buffer);
        if !fmt_desc.is_null() {
            let atoms = CMFormatDescriptionGetExtension(
                fmt_desc,
                kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms,
            );
            if !atoms.is_null() {
                if let Ok(key) = cf_string(state.atom_name) {
                    let atom_data = CFDictionaryGetValue(atoms, key.as_ptr());
                    if !atom_data.is_null() {
                        let len = CFDataGetLength(atom_data) as usize;
                        let ptr = CFDataGetBytePtr(atom_data);
                        if len > 0 && !ptr.is_null() {
                            state.codec_config = std::slice::from_raw_parts(ptr, len).to_vec();
                        }
                    }
                }
            }
        }
    }

    let data_buf = CMSampleBufferGetDataBuffer(sample_buffer);
    if data_buf.is_null() {
        return;
    }
    let len = CMBlockBufferGetDataLength(data_buf);
    let mut payload = vec![0u8; len];
    if len > 0 {
        let rc = CMBlockBufferCopyDataBytes(data_buf, 0, len, payload.as_mut_ptr().cast());
        if rc != 0 {
            state
                .first_error
                .get_or_insert_with(|| format!("CMBlockBufferCopyDataBytes failed: {rc}"));
            return;
        }
    }

    let pts_t = CMSampleBufferGetPresentationTimeStamp(sample_buffer);
    let dts_t = CMSampleBufferGetDecodeTimeStamp(sample_buffer);
    let dur_t = CMSampleBufferGetDuration(sample_buffer);
    let pts = if pts_t.is_valid() { pts_t.value } else { 0 };
    let dts = if dts_t.is_valid() { dts_t.value } else { pts };
    let duration = if dur_t.is_valid() && dur_t.value > 0 {
        dur_t.value
    } else {
        1
    };

    let mut is_keyframe = true;
    let attachments = CMSampleBufferGetSampleAttachmentsArray(sample_buffer, 0);
    if !attachments.is_null() && CFArrayGetCount(attachments) > 0 {
        let dict = CFArrayGetValueAtIndex(attachments, 0);
        if !dict.is_null() {
            let not_sync = CFDictionaryGetValue(dict, kCMSampleAttachmentKey_NotSync);
            if !not_sync.is_null() && not_sync == kCFBooleanTrue {
                is_keyframe = false;
            }
        }
    }

    state.packets.push(EncodedPacket {
        stream_index: 0,
        data: payload,
        pts,
        dts,
        duration,
        is_keyframe,
    });
}

#[derive(Default)]
struct DecodeOutputState {
    frames: Vec<DecodedVideoFrame>,
    first_error: Option<String>,
}

/// Apple VideoToolbox hardware video decoder (`H.264` and `HEVC`).
pub struct VtDecoder {
    session: VTDecompressionSessionRef,
    fmt_desc: CMVideoFormatDescriptionRef,
    state: *mut Mutex<DecodeOutputState>,
    timescale: i32,
}

// SAFETY: Synchronized via `&mut self` and internal `Mutex<DecodeOutputState>`.
unsafe impl Send for VtDecoder {}

impl VtDecoder {
    pub fn new(
        codec: VideoCodecId,
        width: u32,
        height: u32,
        timescale: u32,
        codec_config: &[u8],
    ) -> Result<Self, MediaError> {
        let (codec_type, atom_name) = vt_codec_type(codec)?;
        if width == 0 || height == 0 || codec_config.is_empty() {
            return Err(MediaError::InvalidConfig(
                "VideoToolbox decoder requires non-zero dimensions and codec_config".into(),
            ));
        }

        let atom_key = cf_string(atom_name)?;
        let atom_val = cf_data(codec_config)?;
        let atoms_dict = cf_dict(&[(atom_key.as_ptr(), atom_val.as_ptr())])?;
        // SAFETY: CoreMedia extension key pointer is valid for process lifetime.
        let extensions = unsafe {
            cf_dict(&[(
                kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms,
                atoms_dict.as_ptr(),
            )])?
        };

        let mut fmt_desc: CMVideoFormatDescriptionRef = null();
        // SAFETY: Creates format description from `avcC`/`hvcC` sample description atoms.
        let fmt_rc = unsafe {
            CMVideoFormatDescriptionCreate(
                null(),
                codec_type,
                width as i32,
                height as i32,
                extensions.as_ptr(),
                &mut fmt_desc,
            )
        };
        if fmt_rc != 0 || fmt_desc.is_null() {
            return Err(MediaError::Codec(format!(
                "CMVideoFormatDescriptionCreate failed with OSStatus {fmt_rc}"
            )));
        }

        let bgra_num = cf_num_i32(K_CV_PIXEL_FORMAT_TYPE_32BGRA as i32)?;
        let empty_dict = cf_dict(&[])?;
        // SAFETY: CoreVideo attribute key pointers are valid for process lifetime.
        let dst_attrs = unsafe {
            cf_dict(&[
                (kCVPixelBufferPixelFormatTypeKey, bgra_num.as_ptr()),
                (kCVPixelBufferIOSurfacePropertiesKey, empty_dict.as_ptr()),
            ])?
        };

        let state = Box::into_raw(Box::new(Mutex::new(DecodeOutputState::default())));
        let callback_record = VTDecompressionOutputCallbackRecord {
            decompression_output_callback: Some(vt_decompression_output_callback),
            decompression_output_ref_con: state.cast(),
        };

        let mut session: VTDecompressionSessionRef = null_mut();
        // SAFETY: Creates hardware decompression session bound to `fmt_desc` and `state`.
        let sess_rc = unsafe {
            VTDecompressionSessionCreate(
                null(),
                fmt_desc,
                null(),
                dst_attrs.as_ptr(),
                &callback_record,
                &mut session,
            )
        };
        if sess_rc != 0 || session.is_null() {
            // SAFETY: Clean up `fmt_desc` and `state` on initialization failure.
            unsafe {
                CFRelease(fmt_desc);
                drop(Box::from_raw(state));
            }
            return Err(MediaError::Codec(format!(
                "VTDecompressionSessionCreate failed with OSStatus {sess_rc}"
            )));
        }

        let ts = i32::try_from(timescale.max(1)).unwrap_or(90_000);
        Ok(Self {
            session,
            fmt_desc,
            state,
            timescale: ts,
        })
    }

    pub fn decode(
        &mut self,
        sample_bytes: &[u8],
        pts: i64,
        dts: i64,
        duration: i64,
    ) -> Result<Vec<DecodedVideoFrame>, MediaError> {
        if sample_bytes.is_empty() {
            return Ok(Vec::new());
        }

        let len = sample_bytes.len();
        let mut block_buf: CMBlockBufferRef = null_mut();
        // SAFETY: CoreMedia allocates and owns the block memory (`memory_block = null`, flag = 1).
        let bb_rc = unsafe {
            CMBlockBufferCreateWithMemoryBlock(
                null(),
                null_mut(),
                len,
                null(),
                null(),
                0,
                len,
                K_CM_BLOCK_BUFFER_ASSURE_MEMORY_NOW_FLAG,
                &mut block_buf,
            )
        };
        if bb_rc != 0 || block_buf.is_null() {
            return Err(MediaError::Codec(format!(
                "CMBlockBufferCreateWithMemoryBlock failed with OSStatus {bb_rc}"
            )));
        }
        let _bb_guard = CfGuard::from_owned(block_buf.cast());

        // SAFETY: Copy caller sample bytes into CoreMedia-owned block buffer.
        let copy_rc = unsafe {
            CMBlockBufferReplaceDataBytes(sample_bytes.as_ptr().cast(), block_buf, 0, len)
        };
        if copy_rc != 0 {
            return Err(MediaError::Codec(format!(
                "CMBlockBufferReplaceDataBytes failed with OSStatus {copy_rc}"
            )));
        }

        let timing = CMSampleTimingInfo {
            duration: CMTime::new(duration.max(1), self.timescale),
            presentation_time_stamp: CMTime::new(pts, self.timescale),
            decode_time_stamp: CMTime::new(dts, self.timescale),
        };
        let mut sample_buf: CMSampleBufferRef = null_mut();
        // SAFETY: Wrap `block_buf` + `fmt_desc` + timing in a `CMSampleBuffer`.
        let sb_rc = unsafe {
            CMSampleBufferCreateReady(
                null(),
                block_buf,
                self.fmt_desc,
                1,
                1,
                &timing,
                1,
                &len,
                &mut sample_buf,
            )
        };
        if sb_rc != 0 || sample_buf.is_null() {
            return Err(MediaError::Codec(format!(
                "CMSampleBufferCreateReady failed with OSStatus {sb_rc}"
            )));
        }
        let _sb_guard = CfGuard::from_owned(sample_buf.cast());

        // SAFETY: Synchronous decompression (`decode_flags = 0`) dispatches frame into callback.
        let dec_rc = unsafe {
            VTDecompressionSessionDecodeFrame(self.session, sample_buf, 0, null_mut(), null_mut())
        };
        if dec_rc != 0 {
            return Err(MediaError::Codec(format!(
                "VTDecompressionSessionDecodeFrame failed with OSStatus {dec_rc}"
            )));
        }

        self.drain_frames()
    }

    pub fn flush(&mut self) -> Result<Vec<DecodedVideoFrame>, MediaError> {
        // SAFETY: Wait until all asynchronous frames finish invoking output callback.
        let rc = unsafe { VTDecompressionSessionWaitForAsynchronousFrames(self.session) };
        if rc != 0 {
            return Err(MediaError::Codec(format!(
                "VTDecompressionSessionWaitForAsynchronousFrames failed with OSStatus {rc}"
            )));
        }
        self.drain_frames()
    }

    fn drain_frames(&self) -> Result<Vec<DecodedVideoFrame>, MediaError> {
        // SAFETY: `self.state` is alive and synchronized under `Mutex`.
        let mut guard = unsafe {
            (*self.state)
                .lock()
                .map_err(|_| MediaError::Codec("decoder state lock poisoned".into()))?
        };
        if let Some(err) = guard.first_error.take() {
            return Err(MediaError::Codec(err));
        }
        Ok(std::mem::take(&mut guard.frames))
    }
}

impl Drop for VtDecoder {
    fn drop(&mut self) {
        // SAFETY: Wait for pending frames and invalidate session before freeing `state`.
        unsafe {
            let _ = VTDecompressionSessionWaitForAsynchronousFrames(self.session);
            VTDecompressionSessionInvalidate(self.session);
            CFRelease(self.session.cast());
            CFRelease(self.fmt_desc);
            if !self.state.is_null() {
                drop(Box::from_raw(self.state));
                self.state = null_mut();
            }
        }
    }
}

unsafe extern "C" fn vt_decompression_output_callback(
    decompression_output_ref_con: *mut c_void,
    _source_frame_ref_con: *mut c_void,
    status: OSStatus,
    _info_flags: VTDecodeInfoFlags,
    image_buffer: CVImageBufferRef,
    presentation_time_stamp: CMTime,
    _presentation_duration: CMTime,
) {
    if decompression_output_ref_con.is_null() {
        return;
    }
    let state_mutex = &*(decompression_output_ref_con as *const Mutex<DecodeOutputState>);
    let Ok(mut state) = state_mutex.lock() else {
        return;
    };

    if status != 0 {
        state
            .first_error
            .get_or_insert_with(|| format!("VideoToolbox decompression error: OSStatus {status}"));
        return;
    }
    if image_buffer.is_null() {
        return;
    }

    let lock_rc = CVPixelBufferLockBaseAddress(image_buffer, K_CV_PIXEL_BUFFER_LOCK_READ_ONLY);
    if lock_rc != 0 {
        state
            .first_error
            .get_or_insert_with(|| format!("CVPixelBufferLockBaseAddress failed: {lock_rc}"));
        return;
    }

    let w = CVPixelBufferGetWidth(image_buffer);
    let h = CVPixelBufferGetHeight(image_buffer);
    let bpr = CVPixelBufferGetBytesPerRow(image_buffer);
    let base = CVPixelBufferGetBaseAddress(image_buffer) as *const u8;

    let frame_result = if !base.is_null() && w > 0 && h > 0 && bpr >= w * 4 {
        let src = std::slice::from_raw_parts(base, bpr * h);
        let mut rgba = vec![0u8; w * h * 4];
        for y in 0..h {
            let src_row = &src[y * bpr..y * bpr + w * 4];
            let dst_row = &mut rgba[y * w * 4..(y + 1) * w * 4];
            for (s, d) in src_row
                .as_chunks::<4>()
                .0
                .iter()
                .zip(dst_row.as_chunks_mut::<4>().0.iter_mut())
            {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = 255;
            }
        }
        Some(DecodedVideoFrame {
            width: w as u32,
            height: h as u32,
            pts: if presentation_time_stamp.is_valid() {
                presentation_time_stamp.value
            } else {
                0
            },
            rgba,
        })
    } else {
        state
            .first_error
            .get_or_insert_with(|| "decoded CVPixelBuffer had invalid dimensions or stride".into());
        None
    };

    CVPixelBufferUnlockBaseAddress(image_buffer, K_CV_PIXEL_BUFFER_LOCK_READ_ONLY);

    if let Some(frame) = frame_result {
        state.frames.push(frame);
    }
}
