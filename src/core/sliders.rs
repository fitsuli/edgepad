//! Continuous slider steps and fractional movement retention.

use std::time::Duration;

use super::{
    Capabilities, Contact, ContactPhase, Engine, FrameOutput, Ownership, SliderAxis,
    SliderDirection, SliderSpec, SliderStep, SlotError, Zone,
};

impl Engine {
    /// Steps wait for `slider_delay` after the contact lands, so a second
    /// finger or a palm flag can still take the contact away. `None` releases
    /// them immediately.
    pub(super) fn emit_slider_steps_if_ready(
        &mut self,
        slot: i32,
        timestamp: Option<Duration>,
        output: &mut FrameOutput,
    ) -> Result<(), SlotError> {
        let (zone, spec, position, tracking_id) = {
            let Some(contact) = &self.slot(slot)?.contact else {
                return Ok(());
            };
            if contact.gesture_rejected {
                return Ok(());
            }
            let Ownership::Claimed(zone) = contact.ownership else {
                return Ok(());
            };
            let Some(spec) = self.slider_for_zone(zone) else {
                return Ok(());
            };
            let Some(position) = slider_position(self.caps, spec.axis, contact) else {
                return Ok(());
            };
            (zone, spec, position, contact.tracking_id)
        };

        let slider_delay = self.options.slider_delay;
        let contact = self
            .slot_mut(slot)?
            .contact
            .as_mut()
            .expect("slider contact remains active");
        let Some(mut anchor) = contact.slider_anchor else {
            contact.slider_anchor = Some(position);
            return Ok(());
        };
        if let (Some(started_at), Some(timestamp)) = (contact.started_at, timestamp) {
            if timestamp < started_at.saturating_add(slider_delay) {
                return Ok(());
            }
        }

        let moved_by_step = (position - anchor).abs() >= spec.step;

        // Advance by whole steps, retaining the remainder. Setting the
        // anchor to the latest position would lose small movements and make
        // sensitivity depend on the device's event rate.
        while position - anchor >= spec.step {
            push_slider_step(
                contact,
                output,
                SliderStep {
                    zone,
                    direction: positive_slider_direction(spec.axis),
                    slot,
                    tracking_id,
                },
            );
            anchor += spec.step;
        }

        while anchor - position >= spec.step {
            push_slider_step(
                contact,
                output,
                SliderStep {
                    zone,
                    direction: negative_slider_direction(spec.axis),
                    slot,
                    tracking_id,
                },
            );
            anchor -= spec.step;
        }

        contact.slider_anchor = Some(anchor);
        if moved_by_step {
            self.flush_pending_tap(output);
        }
        Ok(())
    }

    /// A diagonal drag from the edge is pointer motion. Handing it back before
    /// the first step keeps the cursor from freezing under it.
    pub(super) fn slid_off_axis_before_first_step(&self, slot: i32) -> Result<bool, SlotError> {
        let Some(contact) = &self.slot(slot)?.contact else {
            return Ok(false);
        };
        let Ownership::Claimed(zone) = contact.ownership else {
            return Ok(false);
        };
        let Some(spec) = self.slider_for_zone(zone) else {
            return Ok(false);
        };
        Ok(!contact.gesture_rejected
            && contact.contact_phase == ContactPhase::Motion
            && moved_across_slider(spec.axis, contact))
    }

    pub(super) fn slider_for_zone(&self, zone: Zone) -> Option<SliderSpec> {
        self.sliders
            .iter()
            .copied()
            .find(|slider| slider.zone == zone)
    }
}

fn push_slider_step(contact: &mut Contact, output: &mut FrameOutput, step: SliderStep) {
    // Even a step smaller than tap slop commits the contact to the slider.
    // Lifting or returning to the start must not also fire a tap.
    contact.consume_by_slider();
    output.slider_steps.push(step);
}

pub(super) fn validate_slider_specs(sliders: &[SliderSpec]) {
    for slider in sliders {
        assert!(
            slider.step.is_finite() && slider.step > 0.0 && slider.step <= 1.0,
            "invalid slider step for {:?}: {}",
            slider.zone,
            slider.step
        );
    }
}

// More than half as far across the slider as along it, about 27 degrees.
// shortcut: compares raw units, so X and Y resolution must match (the Zenbook
// S14 reports 62 and 64 units/mm), carry ABS resolution in Capabilities once a
// supported touchpad differs much.
fn moved_across_slider(axis: SliderAxis, contact: &Contact) -> bool {
    let (Some(start_x), Some(start_y), Some(x), Some(y)) = (
        contact.start_x,
        contact.start_y,
        contact.current_x,
        contact.current_y,
    ) else {
        return false;
    };
    let dx = (x - start_x).abs();
    let dy = (y - start_y).abs();
    let (along, across) = match axis {
        SliderAxis::Horizontal => (dx, dy),
        SliderAxis::Vertical => (dy, dx),
    };
    across * 2 > along
}

pub(super) fn slider_position(
    caps: Capabilities,
    axis: SliderAxis,
    state: &Contact,
) -> Option<f32> {
    match axis {
        SliderAxis::Horizontal => state.current_x.map(|x| caps.x.normalize(x)),
        SliderAxis::Vertical => state.current_y.map(|y| caps.y.normalize(y)),
    }
}

fn positive_slider_direction(axis: SliderAxis) -> SliderDirection {
    match axis {
        SliderAxis::Horizontal => SliderDirection::Right,
        SliderAxis::Vertical => SliderDirection::Down,
    }
}

fn negative_slider_direction(axis: SliderAxis) -> SliderDirection {
    match axis {
        SliderAxis::Horizontal => SliderDirection::Left,
        SliderAxis::Vertical => SliderDirection::Up,
    }
}
