//! Readback and timing collection once a submitted frame has finished.

use super::*;

impl WgpuBackend {
    #[allow(clippy::type_complexity)]
    pub(crate) fn drain_slot(
        &self,
        in_flight: InFlight,
        res: &GpuFrameResources,
        width: u32,
        height: u32,
        scratch: &mut Vec<u8>,
        sink: &mut dyn FrameSink,
    ) -> Result<(), RasterError> {
        let is_profiling = self.is_profiling_enabled();

        let fence_start = if is_profiling {
            Some(Instant::now())
        } else {
            None
        };
        self.ctx
            .device
            .poll(wgpu::Maintain::wait_for(in_flight.submission_index));
        let submission_wait_ns = fence_start
            .map(|t| t.elapsed().as_nanos() as u64)
            .unwrap_or(0);

        let slot = &res.slots[in_flight.slot_idx];

        let gpu_exec_ns = if is_profiling {
            if let Some(query_rx) = in_flight.query_rx {
                if let Ok(Ok(())) = query_rx.recv() {
                    if let Some(ref q_buf) = slot.query_readback_buffer {
                        let slice = q_buf.slice(..);
                        let q_data = slice.get_mapped_range();
                        let t0 = u64::from_ne_bytes(q_data[0..8].try_into().unwrap_or_default());
                        let t1 = u64::from_ne_bytes(q_data[8..16].try_into().unwrap_or_default());
                        drop(q_data);
                        q_buf.unmap();
                        if t1 >= t0 && t0 > 0 {
                            let ticks = t1 - t0;
                            Some((ticks as f64 * self.ctx.timestamp_period as f64) as u64)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let readback_start = if is_profiling {
            Some(Instant::now())
        } else {
            None
        };

        in_flight
            .rx
            .recv()
            .map_err(|_| RasterError::Frame {
                frame: in_flight.frame_idx,
                reason: "GPU readback channel error".into(),
            })?
            .map_err(|e| RasterError::Frame {
                frame: in_flight.frame_idx,
                reason: format!("GPU map error: {e:?}"),
            })?;

        let bytes_per_row = slot.bytes_per_row;
        let expected_row_bytes = (width * 4) as usize;
        let total_bytes = expected_row_bytes * height as usize;
        let slice = slot.readback.slice(..);
        let data = slice.get_mapped_range();

        let encode_start = if is_profiling {
            Some(Instant::now())
        } else {
            None
        };
        let res = if bytes_per_row as usize == expected_row_bytes {
            sink.consume(in_flight.frame_idx, &data[..total_bytes])
        } else {
            scratch.clear();
            scratch.reserve_exact(total_bytes);
            for row in 0..height {
                let start = (row * bytes_per_row) as usize;
                let end = start + expected_row_bytes;
                scratch.extend_from_slice(&data[start..end]);
            }
            sink.consume(in_flight.frame_idx, scratch)
        };
        let video_encode_ns = encode_start
            .map(|t| t.elapsed().as_nanos() as u64)
            .unwrap_or(0);

        drop(data);
        slot.readback.unmap();

        if is_profiling {
            let readback_total_ns = readback_start
                .map(|t| t.elapsed().as_nanos() as u64)
                .unwrap_or(0);
            let readback_ns = readback_total_ns.saturating_sub(video_encode_ns);
            let total_frame_ns = in_flight.frame_started.elapsed().as_nanos() as u64;
            self.record_profile_sample(FrameProfileSample {
                frame_idx: in_flight.frame_idx,
                scene_eval_ns: in_flight.scene_eval_ns,
                decode_ns: in_flight.decode_ns,
                upload_ns: in_flight.upload_ns,
                upload_bytes: in_flight.upload_bytes,
                compile_encode_ns: in_flight.compile_encode_ns,
                submission_wait_ns,
                gpu_exec_ns,
                readback_ns,
                readback_bytes: total_bytes as u64,
                video_encode_ns,
                total_frame_ns,
                cpu_fallback: false,
            });
        }

        res
    }
}
