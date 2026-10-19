use std::time::Duration;

use edgepad::core::{
    AxisRange, Capabilities, EdgeWidths, Engine, EngineOptions, Event, GestureDirection,
    ResyncContact, SliderAxis, SliderDirection, SliderSpec, SlotError, Zone, ZoneSet,
};
use edgepad::raw::BTN_LEFT;

fn test_caps() -> Capabilities {
    Capabilities {
        slot_min: 0,
        slot_max: 1,
        x: AxisRange { min: 0, max: 1000 },
        y: AxisRange { min: 0, max: 700 },
    }
}

#[test]
fn rejects_slots_outside_device_capability_range_instead_of_clamping() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.08));

    let err = engine
        .process_frame(&[Event::slot(2), Event::tracking_id(10)])
        .expect_err("slot 2 is outside 0..=1 and must not be clamped");

    assert_eq!(
        err,
        SlotError::SlotOutOfRange {
            slot: 2,
            min: 0,
            max: 1
        }
    );
}

#[test]
fn rejects_duplicate_tracking_id_without_release_for_same_slot() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.08));

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(10),
            Event::x(500),
            Event::y(300),
        ])
        .expect("first contact starts cleanly");

    let err = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(11)])
        .expect_err("a slot cannot start twice without TRACKING_ID=-1");

    assert_eq!(
        err,
        SlotError::SlotAlreadyActive {
            slot: 0,
            active_tracking_id: 10,
            new_tracking_id: 11
        }
    );
}

#[test]
fn claimed_edge_touch_does_not_emit_initial_down_to_passthrough() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));

    let down = engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left edge contact is valid");

    assert_eq!(down.passthrough, Vec::<Event>::new());
    assert!(down.gestures.is_empty());

    let move_frame = engine
        .process_frame(&[Event::slot(0), Event::x(220), Event::y(310)])
        .expect("claimed movement remains internal");
    assert_eq!(move_frame.passthrough, Vec::<Event>::new());

    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("release emits gesture only");

    assert_eq!(up.passthrough, Vec::<Event>::new());
    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].zone, Zone::Left);
    assert_eq!(up.gestures[0].direction, GestureDirection::Right);
}

#[test]
fn edge_touch_that_leaves_tap_slop_and_returns_emits_no_gesture() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(21),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left edge contact is valid");

    engine
        .process_frame(&[Event::slot(0), Event::x(60)])
        .expect("movement beyond tap slop remains claimed");
    engine
        .process_frame(&[Event::slot(0), Event::x(20)])
        .expect("return to the starting point remains claimed");

    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("cancelled gesture releases cleanly");

    assert!(up.gestures.is_empty());
    assert!(up.slider_steps.is_empty());
    assert!(up.passthrough.is_empty());
}

#[test]
fn buttonpad_press_promotes_claimed_contact_and_cancels_edge_gesture() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));
    engine.set_buttonpad(true);

    let down = engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left edge contact is valid");
    assert!(down.passthrough.is_empty());

    assert_eq!(
        engine.update_physical_button(BTN_LEFT, true),
        vec![
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
        ]
    );

    let motion = engine
        .process_frame(&[Event::slot(0), Event::x(220), Event::y(310)])
        .expect("promoted contact motion passes through");
    assert_eq!(
        motion.passthrough,
        vec![Event::slot(0), Event::x(220), Event::y(310)]
    );
    assert!(motion.gestures.is_empty());

    assert!(engine.update_physical_button(BTN_LEFT, false).is_empty());
    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("promoted contact release passes through");
    assert_eq!(up.passthrough, vec![Event::slot(0), Event::tracking_id(-1)]);
    assert!(up.gestures.is_empty());
}

#[test]
fn edge_contacts_landing_together_pass_through_before_clickfinger_press() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));
    engine.set_buttonpad(true);

    let output = engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
            Event::slot(1),
            Event::tracking_id(21),
            Event::x(980),
            Event::y(320),
        ])
        .expect("two edge contacts are valid");

    assert_eq!(
        output.passthrough,
        vec![
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
            Event::slot(1),
            Event::tracking_id(21),
            Event::x(980),
            Event::y(320),
        ]
    );
    assert!(engine.update_physical_button(BTN_LEFT, true).is_empty());
}

#[test]
fn button_held_on_buttonpad_forces_new_edge_contact_to_passthrough() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));
    engine.set_buttonpad(true);
    assert!(engine.update_physical_button(BTN_LEFT, true).is_empty());

    let down = engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
        ])
        .expect("edge contact while button is held is valid");
    assert_eq!(
        down.passthrough,
        vec![
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
        ]
    );

    assert!(engine.update_physical_button(BTN_LEFT, false).is_empty());
    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("contact remains passthrough after button release");
    assert_eq!(up.passthrough, vec![Event::slot(0), Event::tracking_id(-1)]);
    assert!(up.gestures.is_empty());
}

#[test]
fn physical_button_on_non_buttonpad_does_not_preempt_edge_ownership() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left edge contact is valid");
    assert!(engine.update_physical_button(BTN_LEFT, true).is_empty());

    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("separate button does not cancel edge gesture");
    assert!(up.passthrough.is_empty());
    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].direction, GestureDirection::Tap);
}

#[test]
fn buttonpad_tap_without_hardware_button_remains_edge_gesture() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));
    engine.set_buttonpad(true);

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left edge contact is valid");
    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("tap release is valid");

    assert!(up.passthrough.is_empty());
    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].direction, GestureDirection::Tap);
}

#[test]
fn slider_edge_touch_emits_steps_during_motion_without_release_swipe() {
    let mut engine = Engine::with_sliders(
        test_caps(),
        EdgeWidths::all(0.10),
        vec![SliderSpec {
            zone: Zone::Left,
            axis: SliderAxis::Vertical,
            step: 0.09,
        }],
    );

    let down = engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(22),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left slider contact is valid");

    assert!(down.slider_steps.is_empty());
    assert!(down.gestures.is_empty());

    let move_frame = engine
        .process_frame(&[Event::slot(0), Event::y(510)])
        .expect("slider motion emits steps");

    assert_eq!(move_frame.slider_steps.len(), 3);
    for step in &move_frame.slider_steps {
        assert_eq!(step.zone, Zone::Left);
        assert_eq!(step.direction, SliderDirection::Down);
        assert_eq!(step.slot, 0);
        assert_eq!(step.tracking_id, 22);
    }
    assert!(move_frame.gestures.is_empty());
    assert!(move_frame.passthrough.is_empty());

    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("slider release is valid");

    assert!(up.slider_steps.is_empty());
    assert!(up.gestures.is_empty());
    assert!(up.passthrough.is_empty());
}

#[test]
fn slider_that_emits_steps_and_returns_to_start_does_not_emit_tap() {
    let mut engine = Engine::with_sliders(
        test_caps(),
        EdgeWidths::all(0.10),
        vec![SliderSpec {
            zone: Zone::Left,
            axis: SliderAxis::Vertical,
            step: 0.09,
        }],
    );

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(23),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left slider contact is valid");

    let forward = engine
        .process_frame(&[Event::slot(0), Event::y(510)])
        .expect("forward slider motion emits steps");
    assert_eq!(forward.slider_steps.len(), 3);

    let returned = engine
        .process_frame(&[Event::slot(0), Event::y(300)])
        .expect("return slider motion emits reverse steps");
    assert!(!returned.slider_steps.is_empty());
    assert!(returned
        .slider_steps
        .iter()
        .all(|step| step.direction == SliderDirection::Up));

    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("returned slider contact releases cleanly");

    assert!(up.gestures.is_empty());
    assert!(up.slider_steps.is_empty());
    assert!(up.passthrough.is_empty());
}

#[test]
fn buttonpad_press_keeps_emitted_slider_steps_but_cancels_future_steps_and_gesture() {
    let mut engine = Engine::with_sliders(
        test_caps(),
        EdgeWidths::all(0.10),
        vec![SliderSpec {
            zone: Zone::Left,
            axis: SliderAxis::Vertical,
            step: 0.09,
        }],
    );
    engine.set_buttonpad(true);

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(22),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left slider contact is valid");
    let before_click = engine
        .process_frame(&[Event::slot(0), Event::y(510)])
        .expect("slider motion emits steps before click");
    assert_eq!(before_click.slider_steps.len(), 3);

    assert_eq!(
        engine.update_physical_button(BTN_LEFT, true),
        vec![
            Event::slot(0),
            Event::tracking_id(22),
            Event::x(20),
            Event::y(510),
        ]
    );
    let after_click = engine
        .process_frame(&[Event::slot(0), Event::y(650)])
        .expect("motion after click passes through");
    assert_eq!(after_click.passthrough, vec![Event::slot(0), Event::y(650)]);
    assert!(after_click.slider_steps.is_empty());

    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("promoted slider contact releases cleanly");
    assert!(up.slider_steps.is_empty());
    assert!(up.gestures.is_empty());
}

#[test]
fn slider_edge_touch_still_emits_tap_on_release() {
    let mut engine = Engine::with_sliders(
        test_caps(),
        EdgeWidths::all(0.10),
        vec![SliderSpec {
            zone: Zone::Left,
            axis: SliderAxis::Vertical,
            step: 0.09,
        }],
    );

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(23),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left slider contact is valid");

    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("slider tap release is valid");

    assert!(up.slider_steps.is_empty());
    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].zone, Zone::Left);
    assert_eq!(up.gestures[0].direction, GestureDirection::Tap);
}

#[test]
fn slider_step_inside_tap_slop_consumes_release_gesture() {
    let mut engine = Engine::with_sliders(
        test_caps(),
        EdgeWidths::all(0.10),
        vec![SliderSpec {
            zone: Zone::Left,
            axis: SliderAxis::Vertical,
            step: 0.01,
        }],
    );

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(24),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left slider contact is valid");

    let moved = engine
        .process_frame(&[Event::slot(0), Event::y(310)])
        .expect("movement inside tap slop emits a configured slider step");
    assert_eq!(moved.slider_steps.len(), 1);

    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("consumed slider contact releases cleanly");

    assert!(up.gestures.is_empty());
    assert!(up.slider_steps.is_empty());
    assert!(up.passthrough.is_empty());
}

#[test]
fn short_edge_touch_does_not_emit_tap_when_timing_is_known() {
    let mut engine = Engine::with_options(
        test_caps(),
        EdgeWidths::all(0.10),
        Vec::new(),
        EngineOptions {
            tap_min_duration: Duration::from_millis(80),
            ..EngineOptions::default()
        },
    );

    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(24),
                Event::x(20),
                Event::y(300),
            ],
            Duration::from_millis(1000),
        )
        .expect("left edge contact is valid");

    let up = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(1079),
        )
        .expect("short tap release is valid");

    assert!(up.gestures.is_empty());
    assert!(up.passthrough.is_empty());
}

#[test]
fn edge_touch_at_tap_min_duration_emits_tap() {
    let mut engine = Engine::with_options(
        test_caps(),
        EdgeWidths::all(0.10),
        Vec::new(),
        EngineOptions {
            tap_min_duration: Duration::from_millis(80),
            ..EngineOptions::default()
        },
    );

    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(25),
                Event::x(20),
                Event::y(300),
            ],
            Duration::from_millis(1000),
        )
        .expect("left edge contact is valid");

    let up = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(1080),
        )
        .expect("tap release is valid");

    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].zone, Zone::Left);
    assert_eq!(up.gestures[0].direction, GestureDirection::Tap);
}

#[test]
fn zero_tap_min_duration_allows_immediate_tap() {
    let mut engine = Engine::with_options(
        test_caps(),
        EdgeWidths::all(0.10),
        Vec::new(),
        EngineOptions {
            tap_min_duration: Duration::ZERO,
            ..EngineOptions::default()
        },
    );

    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(26),
                Event::x(20),
                Event::y(300),
            ],
            Duration::from_millis(1000),
        )
        .expect("left edge contact is valid");

    let up = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(1000),
        )
        .expect("immediate tap release is valid");

    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].zone, Zone::Left);
    assert_eq!(up.gestures[0].direction, GestureDirection::Tap);
}

#[test]
fn short_edge_swipe_still_emits_directional_gesture() {
    let mut engine = Engine::with_options(
        test_caps(),
        EdgeWidths::all(0.10),
        Vec::new(),
        EngineOptions {
            tap_min_duration: Duration::from_millis(80),
            ..EngineOptions::default()
        },
    );

    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(27),
                Event::x(20),
                Event::y(300),
            ],
            Duration::from_millis(1000),
        )
        .expect("left edge contact is valid");
    engine
        .process_frame_at(
            &[Event::slot(0), Event::x(220), Event::y(310)],
            Duration::from_millis(1030),
        )
        .expect("claimed movement remains internal");

    let up = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(1040),
        )
        .expect("short swipe release is valid");

    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].direction, GestureDirection::Right);
}

#[test]
fn inactive_edge_width_passes_edge_touch_through() {
    let mut engine = Engine::new(
        test_caps(),
        EdgeWidths {
            left: 0.0,
            right: 0.10,
            top: 0.0,
            bottom: 0.0,
        },
    );

    let down = engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(21),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left edge contact is valid");

    assert_eq!(
        down.passthrough,
        vec![
            Event::slot(0),
            Event::tracking_id(21),
            Event::x(20),
            Event::y(300)
        ]
    );
    assert!(down.gestures.is_empty());

    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("inactive edge release passes through");

    assert_eq!(up.passthrough, vec![Event::slot(0), Event::tracking_id(-1)]);
    assert!(up.gestures.is_empty());
}

#[test]
fn center_touch_is_passthrough_from_initial_down_to_release() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));

    let down = engine
        .process_frame(&[
            Event::slot(1),
            Event::tracking_id(30),
            Event::x(500),
            Event::y(300),
        ])
        .expect("center touch starts cleanly");

    assert_eq!(
        down.passthrough,
        vec![
            Event::slot(1),
            Event::tracking_id(30),
            Event::x(500),
            Event::y(300)
        ]
    );

    let up = engine
        .process_frame(&[Event::slot(1), Event::tracking_id(-1)])
        .expect("center touch release passes through");

    assert_eq!(up.passthrough, vec![Event::slot(1), Event::tracking_id(-1)]);
    assert!(up.gestures.is_empty());
}

#[test]
fn syn_dropped_clears_state_and_requires_explicit_resync() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(40),
            Event::x(500),
            Event::y(300),
        ])
        .expect("touch starts cleanly");

    let dropped = engine
        .process_frame(&[Event::syn_dropped()])
        .expect("SYN_DROPPED is represented as resync-required output, not ignored");

    assert!(dropped.resync_required);
    assert!(dropped.passthrough.is_empty());
    assert!(dropped.gestures.is_empty());

    let restarted = engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(41),
            Event::x(500),
            Event::y(300),
        ])
        .expect("state was cleared after SYN_DROPPED");

    assert!(!restarted.resync_required);
}

#[test]
fn restored_contact_is_passthrough_until_release_even_when_it_is_on_an_edge() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));
    engine
        .process_frame(&[Event::syn_dropped()])
        .expect("SYN_DROPPED should reset the engine");

    let restored = engine
        .restore_passthrough_contacts(&[ResyncContact {
            slot: 1,
            tracking_id: 42,
            x: 20,
            y: 300,
            tool_type: None,
        }])
        .expect("kernel snapshot should restore the active contact");

    assert_eq!(
        restored.passthrough,
        vec![
            Event::slot(1),
            Event::tracking_id(42),
            Event::x(20),
            Event::y(300),
        ]
    );
    assert!(restored.gestures.is_empty());

    let released = engine
        .process_frame(&[Event::slot(1), Event::tracking_id(-1)])
        .expect("restored contact release should pass through");
    assert_eq!(
        released.passthrough,
        vec![Event::slot(1), Event::tracking_id(-1)]
    );
    assert!(released.gestures.is_empty());
}

#[test]
fn gesture_distance_is_invariant_across_touchpad_coordinate_ranges() {
    let direction_for = |x_max: i32, start_x: i32, end_x: i32| {
        let mut engine = Engine::new(
            Capabilities {
                x: AxisRange { min: 0, max: x_max },
                ..test_caps()
            },
            EdgeWidths::all(0.10),
        );
        engine
            .process_frame(&[
                Event::slot(0),
                Event::tracking_id(50),
                Event::x(start_x),
                Event::y(300),
            ])
            .expect("contact should start");
        engine
            .process_frame(&[Event::slot(0), Event::x(end_x), Event::y(300)])
            .expect("contact should move");
        engine
            .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
            .expect("contact should release")
            .gestures[0]
            .direction
    };

    assert_eq!(direction_for(1000, 10, 25), GestureDirection::Tap);
    assert_eq!(direction_for(2000, 20, 50), GestureDirection::Tap);
}

#[test]
fn gesture_direction_compares_normalized_axis_travel() {
    let mut engine = Engine::new(
        Capabilities {
            x: AxisRange { min: 0, max: 2000 },
            y: AxisRange { min: 0, max: 1000 },
            ..test_caps()
        },
        EdgeWidths::all(0.10),
    );
    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(51),
            Event::x(20),
            Event::y(500),
        ])
        .expect("contact should start");
    engine
        .process_frame(&[Event::slot(0), Event::x(220), Event::y(650)])
        .expect("contact should move");
    let released = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("contact should release");

    assert_eq!(released.gestures[0].direction, GestureDirection::Down);
}

fn double_tap_engine(single_tap_binding: bool) -> Engine {
    Engine::with_options(
        test_caps(),
        EdgeWidths::all(0.10),
        Vec::new(),
        EngineOptions {
            double_tap_zones: ZoneSet::from_zones([Zone::Left]),
            single_tap_zones: if single_tap_binding {
                ZoneSet::from_zones([Zone::Left])
            } else {
                ZoneSet::default()
            },
            ..EngineOptions::default()
        },
    )
}

fn timed_left_tap(
    engine: &mut Engine,
    tracking_id: i32,
    x: i32,
    y: i32,
    started_at_ms: u64,
    released_at_ms: u64,
) -> (edgepad::core::FrameOutput, edgepad::core::FrameOutput) {
    let down = engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(tracking_id),
                Event::x(x),
                Event::y(y),
            ],
            Duration::from_millis(started_at_ms),
        )
        .expect("tap down should be valid");
    let up = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(released_at_ms),
        )
        .expect("tap release should be valid");
    (down, up)
}

#[test]
fn double_tap_emits_one_sequence_gesture_after_second_valid_tap() {
    let mut engine = double_tap_engine(true);

    let (_, first_up) = timed_left_tap(&mut engine, 100, 20, 300, 1000, 1060);
    assert!(first_up.gestures.is_empty());
    assert_eq!(engine.next_deadline(), Some(Duration::from_millis(1360)));

    let (_, second_up) = timed_left_tap(&mut engine, 101, 25, 305, 1200, 1260);
    assert_eq!(second_up.gestures.len(), 1);
    assert_eq!(second_up.gestures[0].direction, GestureDirection::DoubleTap);
    assert_eq!(second_up.gestures[0].tracking_id, 101);
    assert!(engine.next_deadline().is_none());
}

#[test]
fn double_tap_reuses_type_b_slot_coordinates_when_kernel_omits_unchanged_axes() {
    let mut engine = double_tap_engine(true);
    timed_left_tap(&mut engine, 102, 20, 300, 1000, 1060);

    let second_down = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(103)],
            Duration::from_millis(1200),
        )
        .expect("a Type-B slot may start without repeating unchanged axes");
    assert!(second_down.gestures.is_empty());
    assert!(second_down.passthrough.is_empty());

    let second_up = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(1260),
        )
        .expect("retained-position contact should release cleanly");
    assert_eq!(second_up.gestures.len(), 1);
    assert_eq!(second_up.gestures[0].direction, GestureDirection::DoubleTap);
    assert_eq!(second_up.gestures[0].tracking_id, 103);
}

#[test]
fn new_frame_coordinates_override_retained_slot_position_before_ownership() {
    let mut engine = double_tap_engine(true);
    timed_left_tap(&mut engine, 104, 20, 300, 1000, 1060);

    let center_down = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(105), Event::x(500)],
            Duration::from_millis(1200),
        )
        .expect("new X with retained Y should resolve after the full frame");

    assert_eq!(center_down.gestures.len(), 1);
    assert_eq!(center_down.gestures[0].tracking_id, 104);
    assert_eq!(center_down.gestures[0].direction, GestureDirection::Tap);
    assert!(center_down.passthrough.contains(&Event::tracking_id(105)));
    assert!(center_down.passthrough.contains(&Event::x(500)));
}

#[test]
fn overlapping_contacts_cannot_form_a_temporal_double_tap() {
    let mut engine = double_tap_engine(true);
    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(106),
                Event::x(20),
                Event::y(300),
            ],
            Duration::from_millis(1000),
        )
        .expect("first contact should start");
    engine
        .process_frame_at(
            &[
                Event::slot(1),
                Event::tracking_id(107),
                Event::x(22),
                Event::y(302),
            ],
            Duration::from_millis(1020),
        )
        .expect("overlapping contact should start");

    let first_up = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(1060),
        )
        .expect("first contact should release");
    let second_up = engine
        .process_frame_at(
            &[Event::slot(1), Event::tracking_id(-1)],
            Duration::from_millis(1080),
        )
        .expect("second contact should release");

    assert!(first_up.gestures.is_empty());
    assert!(second_up.gestures.is_empty());
    assert!(engine.next_deadline().is_none());
}

#[test]
fn single_tap_is_released_exactly_at_double_tap_deadline() {
    let mut engine = double_tap_engine(true);
    timed_left_tap(&mut engine, 110, 20, 300, 1000, 1060);

    assert!(engine
        .advance_time(Duration::from_millis(1359))
        .gestures
        .is_empty());
    let expired = engine.advance_time(Duration::from_millis(1360));
    assert_eq!(expired.gestures.len(), 1);
    assert_eq!(expired.gestures[0].direction, GestureDirection::Tap);
}

#[test]
fn double_tap_only_binding_drops_expired_single_tap_without_unmatched_action() {
    let mut engine = double_tap_engine(false);
    timed_left_tap(&mut engine, 120, 20, 300, 1000, 1060);

    let expired = engine.advance_time(Duration::from_millis(1360));
    assert!(expired.gestures.is_empty());
    assert!(engine.next_deadline().is_none());
}

#[test]
fn second_tap_just_inside_timeout_finishes_as_double_tap() {
    let mut engine = double_tap_engine(true);
    timed_left_tap(&mut engine, 130, 20, 300, 1000, 1060);

    let (_, second_up) = timed_left_tap(&mut engine, 131, 20, 300, 1359, 1419);
    assert_eq!(second_up.gestures.len(), 1);
    assert_eq!(second_up.gestures[0].direction, GestureDirection::DoubleTap);
}

#[test]
fn spatially_separate_taps_do_not_form_a_double_tap() {
    let mut engine = double_tap_engine(true);
    timed_left_tap(&mut engine, 140, 20, 200, 1000, 1060);

    let (second_down, second_up) = timed_left_tap(&mut engine, 141, 20, 260, 1200, 1260);
    assert_eq!(second_down.gestures.len(), 1);
    assert_eq!(second_down.gestures[0].direction, GestureDirection::Tap);
    assert!(second_up.gestures.is_empty());

    let second_expired = engine.advance_time(Duration::from_millis(1560));
    assert_eq!(second_expired.gestures.len(), 1);
    assert_eq!(second_expired.gestures[0].tracking_id, 141);
}

#[test]
fn movement_during_second_contact_releases_pending_single_immediately() {
    let mut engine = double_tap_engine(true);
    timed_left_tap(&mut engine, 150, 20, 300, 1000, 1060);
    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(151),
                Event::x(20),
                Event::y(300),
            ],
            Duration::from_millis(1200),
        )
        .expect("second contact should start");

    let moved = engine
        .process_frame_at(&[Event::slot(0), Event::x(80)], Duration::from_millis(1230))
        .expect("second contact movement should be valid");
    assert_eq!(moved.gestures.len(), 1);
    assert_eq!(moved.gestures[0].direction, GestureDirection::Tap);
    assert!(engine.next_deadline().is_none());

    let released = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(1260),
        )
        .expect("moved contact should release");
    assert_eq!(released.gestures[0].direction, GestureDirection::Right);
}

#[test]
fn long_stationary_contact_is_not_a_tap() {
    let mut engine = double_tap_engine(true);

    let (_, released) = timed_left_tap(&mut engine, 160, 20, 300, 1000, 1181);
    assert!(released.gestures.is_empty());
    assert!(engine.next_deadline().is_none());
}

#[test]
fn second_contact_held_past_tap_max_duration_cannot_complete_double_tap() {
    let mut engine = double_tap_engine(true);
    timed_left_tap(&mut engine, 170, 20, 300, 1000, 1060);
    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(171),
                Event::x(20),
                Event::y(300),
            ],
            Duration::from_millis(1200),
        )
        .expect("second contact should start");

    let expired = engine.advance_time(Duration::from_millis(1380));
    assert_eq!(expired.gestures.len(), 1);
    assert_eq!(expired.gestures[0].tracking_id, 170);

    let released = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(1381),
        )
        .expect("long second contact should release");
    assert!(released.gestures.is_empty());
}

#[test]
fn syn_dropped_cancels_pending_tap_sequence() {
    let mut engine = double_tap_engine(true);
    timed_left_tap(&mut engine, 180, 20, 300, 1000, 1060);

    let dropped = engine
        .process_frame_at(&[Event::syn_dropped()], Duration::from_millis(1100))
        .expect("SYN_DROPPED should reset sequence state");
    assert!(dropped.resync_required);
    assert!(dropped.gestures.is_empty());
    assert!(engine.next_deadline().is_none());
}

fn left_slider_engine() -> Engine {
    left_slider_engine_with(EngineOptions::default())
}

#[test]
fn second_finger_inside_slider_delay_turns_edge_contact_into_scroll() {
    let mut engine = left_slider_engine();
    let at = Duration::from_millis;

    let down = engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(30),
                Event::x(20),
                Event::y(300),
            ],
            at(1000),
        )
        .expect("edge contact should land");
    let held = engine
        .process_frame_at(&[Event::slot(0), Event::y(510)], at(1100))
        .expect("edge contact should move");
    let second = engine
        .process_frame_at(
            &[
                Event::slot(1),
                Event::tracking_id(31),
                Event::x(400),
                Event::y(320),
            ],
            at(1250),
        )
        .expect("second finger should land");
    let scroll = engine
        .process_frame_at(
            &[Event::slot(0), Event::y(600), Event::slot(1), Event::y(620)],
            at(1400),
        )
        .expect("both fingers should scroll");
    let up = engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(-1),
                Event::slot(1),
                Event::tracking_id(-1),
            ],
            at(1500),
        )
        .expect("both fingers should lift");

    assert!(down.passthrough.is_empty());
    assert!(held.passthrough.is_empty());
    assert_eq!(
        second.passthrough,
        vec![
            Event::slot(0),
            Event::tracking_id(30),
            Event::x(20),
            Event::y(510),
            Event::slot(1),
            Event::tracking_id(31),
            Event::x(400),
            Event::y(320),
        ]
    );
    assert_eq!(
        scroll.passthrough,
        vec![Event::slot(0), Event::y(600), Event::slot(1), Event::y(620)]
    );
    for output in [&down, &held, &second, &scroll, &up] {
        assert!(output.slider_steps.is_empty());
        assert!(output.gestures.is_empty());
    }
}

#[test]
fn edge_contact_landing_while_another_finger_is_down_is_never_claimed() {
    let mut engine = left_slider_engine();
    let at = Duration::from_millis;

    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(32),
                Event::x(400),
                Event::y(300),
            ],
            at(1000),
        )
        .expect("center finger should land");
    let edge = engine
        .process_frame_at(
            &[
                Event::slot(1),
                Event::tracking_id(33),
                Event::x(20),
                Event::y(300),
            ],
            at(1360),
        )
        .expect("edge finger should land late");
    engine
        .process_frame_at(&[Event::slot(0), Event::tracking_id(-1)], at(1400))
        .expect("center finger should lift");
    let alone = engine
        .process_frame_at(&[Event::slot(1), Event::y(600)], at(2000))
        .expect("edge finger should keep moving alone");

    assert_eq!(
        edge.passthrough,
        vec![
            Event::slot(1),
            Event::tracking_id(33),
            Event::x(20),
            Event::y(300),
        ]
    );
    assert_eq!(alone.passthrough, vec![Event::slot(1), Event::y(600)]);
    assert!(alone.slider_steps.is_empty());
}

#[test]
fn one_finger_slide_catches_up_on_first_frame_after_slider_delay() {
    let mut engine = left_slider_engine();
    let at = Duration::from_millis;

    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(34),
                Event::x(20),
                Event::y(300),
            ],
            at(1000),
        )
        .expect("edge contact should land");
    let held = engine
        .process_frame_at(&[Event::slot(0), Event::y(510)], at(1200))
        .expect("edge contact should move");
    let still_held = engine
        .process_frame_at(&[], at(1290))
        .expect("motionless frame should process");
    let caught_up = engine
        .process_frame_at(&[], at(1300))
        .expect("motionless frame should release held steps");
    let live = engine
        .process_frame_at(&[Event::slot(0), Event::y(580)], at(1350))
        .expect("edge contact should keep moving");

    assert!(held.slider_steps.is_empty());
    assert!(still_held.slider_steps.is_empty());
    assert_eq!(caught_up.slider_steps.len(), 3);
    assert!(caught_up
        .slider_steps
        .iter()
        .all(|step| step.direction == SliderDirection::Down && step.tracking_id == 34));
    assert_eq!(live.slider_steps.len(), 1);
}

#[test]
fn one_finger_flick_shorter_than_slider_delay_steps_on_release_without_tap() {
    let mut engine = left_slider_engine();
    let at = Duration::from_millis;

    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(35),
                Event::x(20),
                Event::y(510),
            ],
            at(1000),
        )
        .expect("edge contact should land");
    let held = engine
        .process_frame_at(&[Event::slot(0), Event::y(300)], at(1080))
        .expect("edge contact should flick up");
    let up = engine
        .process_frame_at(&[Event::slot(0), Event::tracking_id(-1)], at(1150))
        .expect("edge contact should lift");

    assert!(held.slider_steps.is_empty());
    assert_eq!(up.slider_steps.len(), 3);
    assert!(up
        .slider_steps
        .iter()
        .all(|step| step.direction == SliderDirection::Up));
    assert!(up.gestures.is_empty());
}

#[test]
fn edge_contact_landing_as_the_other_finger_lifts_in_the_same_frame_is_not_claimed() {
    let mut engine = left_slider_engine();
    let at = Duration::from_millis;

    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(40),
                Event::x(400),
                Event::y(300),
            ],
            at(1000),
        )
        .expect("center finger should land");
    let handover = engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(-1),
                Event::slot(1),
                Event::tracking_id(41),
                Event::x(20),
                Event::y(300),
            ],
            at(1500),
        )
        .expect("center finger lifts as edge finger lands");
    let moved = engine
        .process_frame_at(&[Event::slot(1), Event::y(600)], at(2000))
        .expect("edge finger should keep moving");

    assert_eq!(
        handover.passthrough,
        vec![
            Event::slot(0),
            Event::tracking_id(-1),
            Event::slot(1),
            Event::tracking_id(41),
            Event::x(20),
            Event::y(300),
        ]
    );
    assert_eq!(moved.passthrough, vec![Event::slot(1), Event::y(600)]);
    assert!(moved.slider_steps.is_empty());
}

#[test]
fn steps_released_by_a_frame_wait_for_that_frames_palm_flag_or_second_finger() {
    let at = Duration::from_millis;
    let promoting_tails = [
        vec![Event::tool_type(2)],
        vec![
            Event::slot(1),
            Event::tracking_id(43),
            Event::x(400),
            Event::y(320),
        ],
    ];

    for tail in promoting_tails {
        let mut engine = left_slider_engine();
        engine
            .process_frame_at(
                &[
                    Event::slot(0),
                    Event::tracking_id(42),
                    Event::x(20),
                    Event::y(300),
                ],
                at(1000),
            )
            .expect("edge contact should land");
        let mut frame = vec![Event::slot(0), Event::y(510)];
        frame.extend(tail.iter().copied());
        let boundary = engine
            .process_frame_at(&frame, at(1300))
            .expect("delay boundary frame should process");

        assert!(boundary.slider_steps.is_empty(), "{tail:?}");
        assert_eq!(
            boundary.passthrough[..2],
            [Event::slot(0), Event::tracking_id(42)]
        );
    }

    let mut engine = left_slider_engine();
    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(44),
                Event::x(20),
                Event::y(300),
            ],
            at(1000),
        )
        .expect("edge contact should land");
    let moved_and_lifted = engine
        .process_frame_at(
            &[Event::slot(0), Event::y(510), Event::tracking_id(-1)],
            at(1400),
        )
        .expect("edge contact should move and lift in one frame");
    assert_eq!(moved_and_lifted.slider_steps.len(), 3);
}

fn left_slider_engine_with(options: EngineOptions) -> Engine {
    Engine::with_options(
        test_caps(),
        EdgeWidths::all(0.10),
        vec![SliderSpec {
            zone: Zone::Left,
            axis: SliderAxis::Vertical,
            step: 0.09,
        }],
        EngineOptions {
            slider_delay: Duration::from_millis(300),
            ..options
        },
    )
}

#[test]
fn side_edge_dead_zone_blocks_slides_that_start_near_the_keyboard_only() {
    let mut engine = left_slider_engine_with(EngineOptions {
        side_edge_top_dead_zone: 0.25,
        ..EngineOptions::default()
    });
    let at = Duration::from_millis;

    let high = engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(50),
                Event::x(20),
                Event::y(150),
            ],
            at(1000),
        )
        .expect("contact should land in the dead zone");
    let high_slide = engine
        .process_frame_at(&[Event::slot(0), Event::y(450)], at(1400))
        .expect("contact should slide down");
    engine
        .process_frame_at(&[Event::slot(0), Event::tracking_id(-1)], at(1500))
        .expect("contact should lift");

    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(51),
                Event::x(20),
                Event::y(400),
            ],
            at(2000),
        )
        .expect("contact should land below the dead zone");
    let low_slide = engine
        .process_frame_at(&[Event::slot(0), Event::y(50)], at(2400))
        .expect("contact should slide up into the top quarter");

    assert_eq!(
        high.passthrough,
        vec![
            Event::slot(0),
            Event::tracking_id(50),
            Event::x(20),
            Event::y(150),
        ]
    );
    assert_eq!(high_slide.passthrough, vec![Event::slot(0), Event::y(450)]);
    assert!(high_slide.slider_steps.is_empty());
    assert!(low_slide.passthrough.is_empty());
    assert_eq!(low_slide.slider_steps.len(), 5);
    assert!(low_slide
        .slider_steps
        .iter()
        .all(|step| step.direction == SliderDirection::Up));
}

#[test]
fn diagonal_drag_from_slider_edge_becomes_pointer_motion_before_a_full_step() {
    // Zenbook S14 axes. In raw units this move is diagonal (2 * 60 > 100);
    // normalized per axis it would look vertical (2 * 0.015 < 0.042).
    let mut engine = Engine::with_options(
        Capabilities {
            slot_min: 0,
            slot_max: 1,
            x: AxisRange { min: 0, max: 3911 },
            y: AxisRange { min: 0, max: 2382 },
        },
        EdgeWidths::all(0.07),
        vec![SliderSpec {
            zone: Zone::Left,
            axis: SliderAxis::Vertical,
            step: 0.09,
        }],
        EngineOptions {
            slider_delay: Duration::from_millis(300),
            ..EngineOptions::default()
        },
    );
    let at = Duration::from_millis;

    let down = engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(60),
                Event::x(150),
                Event::y(1000),
            ],
            at(1000),
        )
        .expect("edge contact should land");
    let diagonal = engine
        .process_frame_at(&[Event::slot(0), Event::x(90), Event::y(1100)], at(1050))
        .expect("edge contact should move diagonally");
    let later = engine
        .process_frame_at(&[Event::slot(0), Event::y(1700)], at(1400))
        .expect("pointer contact should keep moving");

    assert!(down.passthrough.is_empty());
    assert_eq!(
        diagonal.passthrough,
        vec![
            Event::slot(0),
            Event::tracking_id(60),
            Event::x(90),
            Event::y(1100),
        ]
    );
    assert_eq!(later.passthrough, vec![Event::slot(0), Event::y(1700)]);
    assert!(later.slider_steps.is_empty());
}

#[test]
fn sideways_drift_after_the_first_step_keeps_the_slide() {
    let mut engine = left_slider_engine_with(EngineOptions::default());
    let at = Duration::from_millis;

    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(61),
                Event::x(20),
                Event::y(300),
            ],
            at(1000),
        )
        .expect("edge contact should land");
    let first = engine
        .process_frame_at(&[Event::slot(0), Event::y(380)], at(1350))
        .expect("vertical slide should step");
    let drift = engine
        .process_frame_at(&[Event::slot(0), Event::x(200), Event::y(450)], at(1400))
        .expect("slide should drift sideways");

    assert_eq!(first.slider_steps.len(), 1);
    assert!(drift.passthrough.is_empty());
    assert_eq!(drift.slider_steps.len(), 1);
}

#[test]
fn edge_contact_landing_right_after_a_key_press_stays_pointer_input() {
    let mut engine = left_slider_engine_with(EngineOptions::default());
    let at = Duration::from_millis;
    engine.record_key_press(at(800));

    let typing = engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(70),
                Event::x(20),
                Event::y(300),
            ],
            at(1050),
        )
        .expect("edge contact should land while typing");
    let typing_slide = engine
        .process_frame_at(&[Event::slot(0), Event::y(600)], at(1500))
        .expect("contact should slide");
    engine
        .process_frame_at(&[Event::slot(0), Event::tracking_id(-1)], at(1600))
        .expect("contact should lift");

    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(71),
                Event::x(20),
                Event::y(300),
            ],
            at(1700),
        )
        .expect("edge contact should land after the lockout");
    let slide = engine
        .process_frame_at(&[Event::slot(0), Event::y(600)], at(2100))
        .expect("contact should slide");

    assert_eq!(
        typing.passthrough,
        vec![
            Event::slot(0),
            Event::tracking_id(70),
            Event::x(20),
            Event::y(300),
        ]
    );
    assert!(typing_slide.slider_steps.is_empty());
    assert!(slide.passthrough.is_empty());
    assert_eq!(slide.slider_steps.len(), 4);
}
