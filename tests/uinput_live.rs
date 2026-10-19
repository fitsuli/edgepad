use edgepad::core::{
    AxisRange, Capabilities, EdgeWidths, EngineOptions, Gesture, GestureDirection, Zone, ZoneSet,
};
use edgepad::proxy::{
    run_proxy_with_gesture_handler_and_ready, GestureHandler, NoopGestureHandler, ProxyMode,
    ProxyRunConfig, ProxyRunLimit, StopAfterFrameLimit,
};
use edgepad::raw::{RawEvent, RawOutputSink, ABS_MT_TOOL_TYPE};
use edgepad::uinput::{
    build_virtual_touchpad, PhysicalTouchpadAbsInfo, UinputRawOutputSink, VirtualAbsAxis,
    VirtualTouchpadSpec, DEFAULT_VIRTUAL_TOUCHPAD_NAME,
};
use evdev::{AbsoluteAxisCode, BusType, InputId};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn live_test_capabilities() -> Capabilities {
    Capabilities {
        slot_min: 0,
        slot_max: 4,
        x: AxisRange { min: 0, max: 1000 },
        y: AxisRange { min: 0, max: 700 },
    }
}

fn emit_frame(
    sink: &mut UinputRawOutputSink<evdev::uinput::VirtualDevice>,
    events: impl IntoIterator<Item = RawEvent>,
) -> Result<(), String> {
    for event in events {
        sink.emit(event)
            .map_err(|err| format!("failed to buffer raw event: {err:?}"))?;
    }
    sink.sync()
        .map_err(|err| format!("failed to emit uinput frame: {err:?}"))
}

#[test]
#[ignore = "requires /dev/uinput and permission to create virtual input devices"]
fn creates_virtual_touchpad_and_emits_center_contact() -> Result<(), String> {
    let spec = VirtualTouchpadSpec::named(live_test_capabilities(), "edgepad live test touchpad");
    let device = build_virtual_touchpad(&spec).map_err(|err| {
        format!(
            "failed to create virtual touchpad via /dev/uinput; load uinput and check permissions: {err}"
        )
    })?;
    let mut sink = UinputRawOutputSink::new(device);

    thread::sleep(Duration::from_millis(50));

    emit_frame(
        &mut sink,
        [
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(1),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
            RawEvent::btn_touch(true),
            RawEvent::btn_tool_finger(true),
            RawEvent::abs_x(500),
            RawEvent::abs_y(300),
        ],
    )?;

    emit_frame(
        &mut sink,
        [
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::btn_touch(false),
            RawEvent::btn_tool_finger(false),
        ],
    )
}

struct ChannelGestureHandler(mpsc::Sender<Gesture>);

impl GestureHandler for ChannelGestureHandler {
    fn handle_gesture(&mut self, gesture: Gesture) {
        let _ = self.0.send(gesture);
    }
}

fn live_double_tap_options(single_tap: bool) -> EngineOptions {
    EngineOptions {
        double_tap_zones: ZoneSet::from_zones([Zone::Left]),
        single_tap_zones: if single_tap {
            ZoneSet::from_zones([Zone::Left])
        } else {
            ZoneSet::default()
        },
        ..EngineOptions::default()
    }
}

type LiveSource = (
    UinputRawOutputSink<evdev::uinput::VirtualDevice>,
    std::path::PathBuf,
);

fn create_live_source(name: &str) -> Result<LiveSource, String> {
    create_live_source_from_spec(&VirtualTouchpadSpec::named(live_test_capabilities(), name))
}

fn create_live_source_from_spec(spec: &VirtualTouchpadSpec) -> Result<LiveSource, String> {
    let mut device = build_virtual_touchpad(spec)
        .map_err(|err| format!("failed to create live source touchpad: {err}"))?;
    let device_path = device
        .enumerate_dev_nodes_blocking()
        .map_err(|err| format!("failed to enumerate live source touchpad: {err}"))?
        .next()
        .ok_or_else(|| "live source touchpad exposed no event node".to_string())?
        .map_err(|err| format!("failed to resolve live source event node: {err}"))?;
    thread::sleep(Duration::from_millis(100));
    Ok((UinputRawOutputSink::new(device), device_path))
}

fn emit_left_tap(
    source: &mut UinputRawOutputSink<evdev::uinput::VirtualDevice>,
    tracking_id: i32,
) -> Result<(), String> {
    emit_frame(
        source,
        [
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(tracking_id),
            RawEvent::abs_mt_position_x(20),
            RawEvent::abs_mt_position_y(300),
            RawEvent::btn_touch(true),
            RawEvent::btn_tool_finger(true),
        ],
    )?;
    thread::sleep(Duration::from_millis(60));
    emit_frame(
        source,
        [
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::btn_touch(false),
            RawEvent::btn_tool_finger(false),
        ],
    )
}

#[test]
#[ignore = "requires /dev/uinput and permission to create and grab virtual input devices"]
fn live_proxy_recognizes_double_tap_from_real_uinput_frames() -> Result<(), String> {
    let (mut source, device_path) = create_live_source("edgepad live double-tap source")?;
    // Prime the kernel slot before edgepad opens the event node. The two tested
    // taps reuse this exact position, so uinput suppresses unchanged axes and
    // the proxy must seed and retain Type-B slot coordinates correctly.
    emit_left_tap(&mut source, 99)?;
    let (gesture_sender, gesture_receiver) = mpsc::channel();
    let (ready_sender, ready_receiver) = mpsc::channel();

    let proxy = thread::spawn(move || {
        let mut handler = ChannelGestureHandler(gesture_sender);
        run_proxy_with_gesture_handler_and_ready(
            &ProxyRunConfig {
                device_path,
                input_root: "/dev/input".into(),
                limit: ProxyRunLimit::Frames {
                    frame_boundaries: 4,
                    stop_after_limit: StopAfterFrameLimit::WhenIdle,
                },
                edge_widths: EdgeWidths::all(0.10),
                engine_options: live_double_tap_options(false),
                slider_specs: Vec::new(),
                mode: ProxyMode::UinputGrab,
            },
            &mut handler,
            &mut |_| {
                ready_sender
                    .send(())
                    .map_err(|_| "live test readiness receiver disappeared".to_string())
            },
        )
    });

    ready_receiver
        .recv_timeout(Duration::from_secs(3))
        .map_err(|err| format!("live proxy did not become ready: {err}"))?;
    emit_left_tap(&mut source, 100)?;
    thread::sleep(Duration::from_millis(80));
    emit_left_tap(&mut source, 101)?;

    let summary = proxy
        .join()
        .map_err(|_| "live proxy thread panicked".to_string())??;
    let gesture = gesture_receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|err| {
            format!(
                "live proxy emitted no double tap: {err}; runtime stats: {:?}",
                summary.stats
            )
        })?;

    assert_eq!(gesture.direction, GestureDirection::DoubleTap);
    assert_eq!(gesture.zone, Zone::Left);
    assert_eq!(summary.stats.gestures, vec![gesture]);
    Ok(())
}

#[test]
#[ignore = "requires /dev/uinput and permission to create and grab virtual input devices"]
fn live_proxy_releases_single_tap_on_idle_deadline() -> Result<(), String> {
    let (mut source, device_path) = create_live_source("edgepad live tap-deadline source")?;
    let (gesture_sender, gesture_receiver) = mpsc::channel();
    let (ready_sender, ready_receiver) = mpsc::channel();

    let proxy = thread::spawn(move || {
        let mut handler = ChannelGestureHandler(gesture_sender);
        run_proxy_with_gesture_handler_and_ready(
            &ProxyRunConfig {
                device_path,
                input_root: "/dev/input".into(),
                limit: ProxyRunLimit::Frames {
                    frame_boundaries: 2,
                    stop_after_limit: StopAfterFrameLimit::WhenIdle,
                },
                edge_widths: EdgeWidths::all(0.10),
                engine_options: live_double_tap_options(true),
                slider_specs: Vec::new(),
                mode: ProxyMode::UinputGrab,
            },
            &mut handler,
            &mut |_| {
                ready_sender
                    .send(())
                    .map_err(|_| "live test readiness receiver disappeared".to_string())
            },
        )
    });

    ready_receiver
        .recv_timeout(Duration::from_secs(3))
        .map_err(|err| format!("live proxy did not become ready: {err}"))?;
    emit_left_tap(&mut source, 200)?;

    let summary = proxy
        .join()
        .map_err(|_| "live proxy thread panicked".to_string())??;
    let gesture = gesture_receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|err| {
            format!(
                "idle deadline emitted no single tap: {err}; runtime stats: {:?}",
                summary.stats
            )
        })?;

    assert_eq!(gesture.direction, GestureDirection::Tap);
    assert_eq!(gesture.zone, Zone::Left);
    assert_eq!(summary.stats.gestures, vec![gesture]);
    assert!(!summary.stats.idle_drain_timed_out);
    Ok(())
}

#[test]
#[ignore = "requires /dev/uinput and permission to create and grab virtual input devices"]
fn live_proxy_mirrors_identity_and_leaves_palm_from_before_startup_to_libinput(
) -> Result<(), String> {
    let source_id = InputId::new(BusType::BUS_I2C, 0xed9e, 0x7a1d, 0x0100);
    let spec = VirtualTouchpadSpec {
        input_id: Some(source_id.clone()),
        ..VirtualTouchpadSpec::from_physical_abs_info(
            PhysicalTouchpadAbsInfo {
                mt_tool_type: Some(VirtualAbsAxis::new(ABS_MT_TOOL_TYPE, 0, 2)),
                ..PhysicalTouchpadAbsInfo::from_capabilities(live_test_capabilities())
            },
            "edgepad live palm source",
        )
    };
    let (mut source, device_path) = create_live_source_from_spec(&spec)?;
    // Leave slot 0 marked as a palm before edgepad starts. Linux will not
    // report the unchanged tool type again for the next contact in that slot.
    emit_frame(
        &mut source,
        [
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(10),
            RawEvent::abs_mt_tool_type(2),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
            RawEvent::btn_touch(true),
            RawEvent::btn_tool_finger(true),
        ],
    )?;
    emit_frame(
        &mut source,
        [
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::btn_touch(false),
            RawEvent::btn_tool_finger(false),
        ],
    )?;
    let (ready_sender, ready_receiver) = mpsc::channel();

    let proxy = thread::spawn(move || {
        let mut handler = NoopGestureHandler;
        run_proxy_with_gesture_handler_and_ready(
            &ProxyRunConfig {
                device_path,
                input_root: "/dev/input".into(),
                limit: ProxyRunLimit::Frames {
                    frame_boundaries: 3,
                    stop_after_limit: StopAfterFrameLimit::WhenIdle,
                },
                edge_widths: EdgeWidths::all(0.10),
                engine_options: EngineOptions::default(),
                slider_specs: Vec::new(),
                mode: ProxyMode::UinputGrab,
            },
            &mut handler,
            &mut |_| {
                ready_sender
                    .send(())
                    .map_err(|_| "live test readiness receiver disappeared".to_string())
            },
        )
    });

    ready_receiver
        .recv_timeout(Duration::from_secs(3))
        .map_err(|err| format!("live proxy did not become ready: {err}"))?;
    let output = find_proxy_output(&source_id)?;
    assert_eq!(output.input_id(), source_id);
    assert!(output
        .supported_absolute_axes()
        .is_some_and(|axes| axes.contains(AbsoluteAxisCode::ABS_MT_TOOL_TYPE)));

    emit_frame(
        &mut source,
        [
            RawEvent::abs_mt_tracking_id(11),
            RawEvent::abs_mt_position_x(20),
            RawEvent::btn_touch(true),
            RawEvent::btn_tool_finger(true),
        ],
    )?;
    thread::sleep(Duration::from_millis(30));
    emit_frame(&mut source, [RawEvent::abs_mt_position_x(220)])?;
    thread::sleep(Duration::from_millis(30));
    emit_frame(
        &mut source,
        [
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::btn_touch(false),
            RawEvent::btn_tool_finger(false),
        ],
    )?;

    let summary = proxy
        .join()
        .map_err(|_| "live proxy thread panicked".to_string())??;
    assert!(
        summary.stats.gestures.is_empty(),
        "palm became a gesture: {:?}",
        summary.stats
    );
    assert_eq!(summary.stats.passthrough_frames, 3);
    Ok(())
}

fn find_proxy_output(source_id: &InputId) -> Result<evdev::Device, String> {
    for _ in 0..20 {
        let output = evdev::enumerate().map(|(_, device)| device).find(|device| {
            device.name() == Some(DEFAULT_VIRTUAL_TOUCHPAD_NAME) && device.input_id() == *source_id
        });
        if let Some(output) = output {
            return Ok(output);
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("proxy output touchpad with the source identity did not appear".to_string())
}
