#[cfg(test)]
mod tests {
    use erd_decode::Nv12Frame;
    use erd_mobile::{TouchGestureHandler, TouchMode, TouchPhase, TouchPoint, ViewportState};
    use erd_proto::{InputEventType, Modifiers};
    use crate::frame::repack_nv12_frame;
    use crate::state::{AppState, SessionStats};

    #[test]
    fn frame_repacking_exact_layout_and_header() {
        let frame = Nv12Frame {
            width: 16,
            height: 16,
            y_stride: 16,
            uv_stride: 16,
            y_plane: vec![10u8; 256],
            uv_plane: vec![20u8; 128],
            timestamp_ms: 100,
        };

        let sequence = 42u64;
        let repacked = repack_nv12_frame(&frame, sequence);

        assert_eq!(repacked.len(), 16 + 256 + 128);

        let w = u32::from_le_bytes(repacked[0..4].try_into().unwrap());
        let h = u32::from_le_bytes(repacked[4..8].try_into().unwrap());
        let seq = u64::from_le_bytes(repacked[8..16].try_into().unwrap());

        assert_eq!(w, 16);
        assert_eq!(h, 16);
        assert_eq!(seq, 42);

        assert_eq!(&repacked[16..16 + 256], &vec![10u8; 256][..]);
        assert_eq!(&repacked[16 + 256..16 + 256 + 128], &vec![20u8; 128][..]);
    }

    #[test]
    fn frame_repacking_stride_repack() {
        let width = 4usize;
        let height = 4usize;
        let y_stride = 8usize;
        let uv_stride = 8usize;

        let mut y_plane = vec![0u8; y_stride * height];
        for row in 0..height {
            for col in 0..width {
                y_plane[row * y_stride + col] = (row * 10 + col) as u8;
            }
        }

        let uv_height = 2usize;
        let mut uv_plane = vec![0u8; uv_stride * uv_height];
        for row in 0..uv_height {
            for col in 0..width {
                uv_plane[row * uv_stride + col] = (row * 20 + col) as u8;
            }
        }

        let frame = Nv12Frame {
            width: width as u32,
            height: height as u32,
            y_stride,
            uv_stride,
            y_plane,
            uv_plane,
            timestamp_ms: 200,
        };

        let repacked = repack_nv12_frame(&frame, 1);
        assert_eq!(repacked.len(), 16 + 16 + 8);

        for row in 0..height {
            for col in 0..width {
                let expected = (row * 10 + col) as u8;
                assert_eq!(repacked[16 + row * width + col], expected);
            }
        }

        for row in 0..uv_height {
            for col in 0..width {
                let expected = (row * 20 + col) as u8;
                assert_eq!(repacked[16 + 16 + row * width + col], expected);
            }
        }
    }

    #[test]
    fn frame_repacking_odd_height() {
        let frame = Nv12Frame {
            width: 4,
            height: 3,
            y_stride: 4,
            uv_stride: 4,
            y_plane: vec![1u8; 12],
            uv_plane: vec![2u8; 8],
            timestamp_ms: 300,
        };

        let repacked = repack_nv12_frame(&frame, 99);
        assert_eq!(repacked.len(), 16 + 12 + 8);
    }

    #[test]
    fn touch_unit_viewport_normalization() {
        let viewport = ViewportState::new(1.0, 1.0).unwrap();
        let mut handler = TouchGestureHandler::new(viewport, TouchMode::DirectTouch);

        let (nx, ny) = handler.viewport_mut().transform_to_host(0.0, 0.0).unwrap();
        assert!((nx - 0.0).abs() < 1e-5);
        assert!((ny - 1.0).abs() < 1e-5);

        let (nx, ny) = handler.viewport_mut().transform_to_host(0.5, 0.5).unwrap();
        assert!((nx - 0.5).abs() < 1e-5);
        assert!((ny - 0.5).abs() < 1e-5);

        let (nx, ny) = handler.viewport_mut().transform_to_host(1.0, 1.0).unwrap();
        assert!((nx - 1.0).abs() < 1e-5);
        assert!((ny - 0.0).abs() < 1e-5);
    }

    #[test]
    fn touch_mode_switching_emits_release() {
        let viewport = ViewportState::new(1.0, 1.0).unwrap();
        let mut handler = TouchGestureHandler::new(viewport, TouchMode::DirectTouch);

        let touch = TouchPoint {
            id: 1,
            x: 0.5,
            y: 0.5,
            phase: TouchPhase::Began,
        };
        let evt = handler.process_touch(touch).unwrap().unwrap();
        assert_eq!(evt.event_type, InputEventType::LeftMouseDown);

        let release = handler.set_mode(TouchMode::TrackpadRelative);
        assert!(release.is_some());
        let rel_evt = release.unwrap();
        assert_eq!(rel_evt.event_type, InputEventType::LeftMouseUp);
        assert!((rel_evt.x - 0.5).abs() < 1e-5);
        assert!((rel_evt.y - 0.5).abs() < 1e-5);
    }

    #[test]
    fn touch_cancellation_emits_release() {
        let viewport = ViewportState::new(1.0, 1.0).unwrap();
        let mut handler = TouchGestureHandler::new(viewport, TouchMode::DirectTouch);

        let touch_began = TouchPoint {
            id: 1,
            x: 0.25,
            y: 0.75,
            phase: TouchPhase::Began,
        };
        let _ = handler.process_touch(touch_began).unwrap();

        let touch_cancel = TouchPoint {
            id: 1,
            x: 0.25,
            y: 0.75,
            phase: TouchPhase::Cancelled,
        };
        let evt = handler.process_touch(touch_cancel).unwrap().unwrap();
        assert_eq!(evt.event_type, InputEventType::LeftMouseUp);
        assert!((evt.x - 0.25).abs() < 1e-5);
        assert!((evt.y - 0.25).abs() < 1e-5);
    }

    #[test]
    fn key_validation_rejects_invalid_inputs() {
        let state = AppState::new();

        assert!(state.handle_key(0, true, 0).is_err());
        assert!(state.handle_key(53, true, 0x8000).is_err());

        let valid_res = state.handle_key(53, true, Modifiers::SHIFT.bits());
        assert!(valid_res.is_ok());
    }

    #[test]
    fn session_stats_serialization_conforms_to_contract() {
        let stats = SessionStats {
            state: "ready".to_string(),
            host: Some("192.168.1.100".to_string()),
            frames_received: 120,
            frames_decoded: 118,
            audio_packets_received: 50,
            audio_samples_played: 24000,
            width: 1920,
            height: 1080,
            last_error: None,
        };

        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"state\":\"ready\""));
        assert!(json.contains("\"host\":\"192.168.1.100\""));
        assert!(json.contains("\"frames_received\":120"));
        assert!(json.contains("\"frames_decoded\":118"));
        assert!(json.contains("\"audio_packets_received\":50"));
        assert!(json.contains("\"audio_samples_played\":24000"));
        assert!(json.contains("\"width\":1920"));
        assert!(json.contains("\"height\":1080"));
        assert!(json.contains("\"last_error\":null"));
    }

    #[test]
    fn trackpad_normalized_drag_produces_pixel_movement() {
        let state = AppState::new();
        {
            let inner = state.inner.lock().unwrap();
            inner.video_width.store(1920, std::sync::atomic::Ordering::Relaxed);
            inner.video_height.store(1080, std::sync::atomic::Ordering::Relaxed);
        }
        state.set_touch_mode("trackpad").unwrap();

        let _ = state.process_touch_for_test(1, 0.5, 0.5, "began").unwrap();
        let evt_opt = state.process_touch_for_test(1, 0.55, 0.52, "moved").unwrap();
        assert!(evt_opt.is_some());
        let evt = evt_opt.unwrap();
        assert_eq!(evt.event_type, InputEventType::RelativeMove);

        assert!((evt.scroll_dx - 96.0).abs() < 1e-3);
        assert!((evt.scroll_dy - 21.6).abs() < 1e-3);

        let host_dx = evt.scroll_dx.round() as i32;
        let host_dy = evt.scroll_dy.round() as i32;
        assert_eq!(host_dx, 96);
        assert_eq!(host_dy, 22);
    }

    #[test]
    fn cancellation_validation_accepts_nonfinite_coords_to_release() {
        let state = AppState::new();
        let began_evt = state.process_touch_for_test(1, 0.5, 0.5, "began").unwrap();
        assert_eq!(began_evt.unwrap().event_type, InputEventType::LeftMouseDown);

        let invalid_move = state.process_touch_for_test(1, f32::NAN, 0.5, "moved");
        assert!(invalid_move.is_err());

        let cancel_res = state.process_touch_for_test(1, f32::NAN, f32::NAN, "cancelled");
        assert!(cancel_res.is_ok());
        let cancel_evt = cancel_res.unwrap();
        assert_eq!(cancel_evt.unwrap().event_type, InputEventType::LeftMouseUp);
    }

    #[test]
    fn presentation_sequence_validation() {
        let state = AppState::new();

        assert!(state.presented(0).is_err());
        assert!(state.presented(1).is_err());

        {
            let inner = state.inner.lock().unwrap();
            inner.frame_sequence.store(5, std::sync::atomic::Ordering::Relaxed);
        }

        assert!(state.presented(3).is_ok());
        assert!(state.presented(5).is_ok());
        assert!(state.presented(6).is_err());
    }

    #[test]
    fn poisoned_mutex_returns_error_instead_of_silent_recovery() {
        let state = AppState::new();
        let inner_arc = state.inner.clone();
        let _ = std::panic::catch_unwind(|| {
            let _guard = inner_arc.lock().unwrap();
            panic!("simulated poisoning");
        });

        assert!(state.stats().is_err());
        assert!(state.poll_frame().is_err());
        assert!(state.presented(1).is_err());
        assert!(state.set_touch_mode("direct").is_err());
    }

    #[test]
    fn build_session_config_normalizes_ipv6_brackets_and_preserves_scope() {
        let c1 = crate::state::build_session_config("192.168.1.50", None, None, "iOS").unwrap();
        assert_eq!(c1.host, "192.168.1.50");
        assert_eq!(c1.tcp_port, erd_app::DEFAULT_TCP_PORT);
        assert_eq!(c1.udp_port, erd_app::DEFAULT_UDP_PORT);
        assert_eq!(c1.tcp_port, 19730);
        assert_eq!(c1.udp_port, 19731);

        let c2 = crate::state::build_session_config(
            "[2001:db8::1]",
            Some(29730),
            Some(29731),
            "iOS",
        )
        .unwrap();
        assert_eq!(c2.host, "2001:db8::1");
        assert_eq!(c2.tcp_port, 29730);
        assert_eq!(c2.udp_port, 29731);

        let c3 = crate::state::build_session_config("fe80::1%en0", None, None, "iOS").unwrap();
        assert_eq!(c3.host, "fe80::1%en0");
    }

    #[test]
    fn build_session_config_rejects_empty_bracketed_hosts_and_zero_ports() {
        assert!(crate::state::build_session_config("", None, None, "iOS").is_err());
        assert!(crate::state::build_session_config("[]", None, None, "iOS").is_err());
        assert!(crate::state::build_session_config("[   ]", None, None, "iOS").is_err());
        assert!(crate::state::build_session_config("10.0.0.1", Some(0), None, "iOS").is_err());
        assert!(crate::state::build_session_config("10.0.0.1", None, Some(0), "iOS").is_err());
    }

    #[test]
    fn pairing_lookup_by_host_address_never_infers_pairing_from_advertised_hostname() {
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join("pairing.json");
        let store = erd_app::PairingStore::new(&store_path);

        let paired_record = erd_app::PairingRecord {
            id: "192.168.1.50".to_string(),
            name: "Workstation".to_string(),
            key: vec![0x42; 32],
            added_at_unix_ms: 1000,
        };
        store.save(paired_record).unwrap();

        let discovered_ip = "192.168.1.99";
        let lookup_result = store.find_by_host(discovered_ip).unwrap();
        assert!(
            lookup_result.is_none(),
            "Discovered host IP must not inherit pairing from matching advertised name"
        );

        let authentic_lookup = store.find_by_host("192.168.1.50").unwrap();
        assert!(authentic_lookup.is_some());
    }
}
