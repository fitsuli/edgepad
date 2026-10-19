//! Contact ownership, passthrough slot routing, and buttonpad promotion.

use super::sliders::slider_position;
use super::{Contact, Engine, Event, FrameOutput, Ownership, SlotError, Zone};

impl Engine {
    pub fn set_buttonpad(&mut self, buttonpad: bool) {
        self.buttonpad = buttonpad;
        if !buttonpad {
            self.pressed_physical_buttons.clear();
        }
    }

    pub fn update_physical_button(&mut self, code: u16, pressed: bool) -> Vec<Event> {
        if !self.buttonpad {
            return Vec::new();
        }

        if !pressed {
            self.pressed_physical_buttons.remove(&code);
            return Vec::new();
        }

        self.pressed_physical_buttons.insert(code);
        self.promote_claimed_contacts()
    }

    pub fn restore_pressed_physical_buttons(&mut self, codes: &[u16]) {
        self.pressed_physical_buttons.clear();
        if self.buttonpad {
            self.pressed_physical_buttons.extend(codes.iter().copied());
        }
    }

    #[cfg(test)]
    pub(crate) fn pressed_physical_buttons(&self) -> Vec<u16> {
        self.pressed_physical_buttons.iter().copied().collect()
    }

    pub(super) fn route_event_for_current_slot(
        &mut self,
        event: Event,
        output: &mut FrameOutput,
    ) -> Result<(), SlotError> {
        let slot_state = self.slot_mut(self.current_slot)?;
        match slot_state.contact.as_ref().map(|contact| contact.ownership) {
            Some(Ownership::Passthrough) => {
                self.push_passthrough_event_for_current_slot(event, output)
            }
            None | Some(Ownership::Unknown) => slot_state.held_events.push(event),
            Some(Ownership::Claimed(_)) => {}
        }
        Ok(())
    }

    pub(super) fn push_passthrough_event_for_current_slot(
        &self,
        event: Event,
        output: &mut FrameOutput,
    ) {
        self.push_passthrough_event_for_slot(self.current_slot, event, output);
    }

    fn push_passthrough_event_for_slot(&self, slot: i32, event: Event, output: &mut FrameOutput) {
        match event {
            Event::Slot(_) | Event::SynDropped => output.passthrough.push(event),
            Event::TrackingId(_) | Event::X(_) | Event::Y(_) | Event::ToolType(_) => {
                // Filtering edge contacts also filters slot switches. The
                // output slot can therefore differ from the physical slot;
                // emitting a bare release here can leave the wrong finger down.
                if last_passthrough_slot(&output.passthrough) != Some(slot) {
                    output.passthrough.push(Event::slot(slot));
                }
                output.passthrough.push(event);
            }
        }
    }

    pub(super) fn decide_ownership_if_ready(
        &mut self,
        output: &mut FrameOutput,
    ) -> Result<(), SlotError> {
        let slot = self.current_slot;
        let Some(contact) = &self.slot(slot)?.contact else {
            return Ok(());
        };
        if contact.ownership != Ownership::Unknown {
            return Ok(());
        }
        let (Some(start_x), Some(start_y)) = (contact.start_x, contact.start_y) else {
            return Ok(());
        };
        let zone = if (self.buttonpad && !self.pressed_physical_buttons.is_empty())
            || contact.overlapped_other_contact
            || self.landed_while_typing(contact)
        {
            None
        } else {
            self.zone_for_position(start_x, start_y)
        };
        let rejected = contact.gesture_rejected;
        let Some(zone) = zone else {
            let state = self.slot_mut(slot)?;
            state
                .contact
                .as_mut()
                .expect("contact is being classified")
                .ownership = Ownership::Passthrough;
            let held_events = std::mem::take(&mut state.held_events);
            let tool_type = state.memory.tool_type;
            if !rejected {
                self.flush_pending_tap(output);
            }
            for event in held_events {
                self.push_passthrough_event_for_slot(slot, event, output);
            }
            if let Some(tool_type) = tool_type {
                self.push_passthrough_event_for_slot(slot, Event::tool_type(tool_type), output);
            }
            return Ok(());
        };

        let slider_anchor = self
            .slider_for_zone(zone)
            .and_then(|spec| slider_position(self.caps, spec.axis, contact));
        let sequence_match = !rejected && self.contact_matches_pending_tap(contact, zone);
        // Palms neither complete nor interrupt an unrelated finger's pending tap.
        if self.pending_tap.is_some() && !sequence_match && !rejected {
            self.flush_pending_tap(output);
        }

        let state = self.slot_mut(slot)?;
        let contact = state.contact.as_mut().expect("contact is being classified");
        contact.ownership = Ownership::Claimed(zone);
        contact.slider_anchor = slider_anchor;
        contact.tap_sequence_match = sequence_match;
        state.held_events.clear();
        Ok(())
    }

    pub(super) fn promote_claimed_contacts(&mut self) -> Vec<Event> {
        (self.caps.slot_min..=self.caps.slot_max)
            .flat_map(|slot| self.promote_claimed_contact(slot).unwrap_or_default())
            .collect()
    }

    pub(super) fn promote_claimed_contact(&mut self, slot: i32) -> Result<Vec<Event>, SlotError> {
        // The contact's original down was hidden from the virtual device.
        // Synthesize a complete down at its current position so the compositor
        // can use it for click-drag, clickfinger, scrolling, or pointer motion.
        let slot_state = self.slot_mut(slot)?;
        let tool_type = slot_state.memory.tool_type;
        let Some(contact) = &mut slot_state.contact else {
            return Ok(Vec::new());
        };
        if !matches!(contact.ownership, Ownership::Claimed(_)) {
            return Ok(Vec::new());
        }
        let (Some(x), Some(y)) = (contact.current_x, contact.current_y) else {
            return Ok(Vec::new());
        };
        contact.ownership = Ownership::Passthrough;
        contact.slider_anchor = None;

        let mut events = vec![
            Event::slot(slot),
            Event::tracking_id(contact.tracking_id),
            Event::x(x),
            Event::y(y),
        ];
        events.extend(tool_type.map(Event::tool_type));
        Ok(events)
    }

    fn zone_for_position(&self, start_x: i32, start_y: i32) -> Option<Zone> {
        let x = self.caps.x.normalize(start_x);
        let y = self.caps.y.normalize(start_y);

        // The dead zone only checks where the touch lands, so a slide that
        // starts lower can still move through it.
        let below_side_dead_zone = y >= self.options.side_edge_top_dead_zone;

        // This order resolves corner overlaps and is part of gesture behavior.
        if x < self.edges.left && below_side_dead_zone {
            Some(Zone::Left)
        } else if x > 1.0 - self.edges.right && below_side_dead_zone {
            Some(Zone::Right)
        } else if y < self.edges.top {
            Some(Zone::Top)
        } else if y > 1.0 - self.edges.bottom {
            Some(Zone::Bottom)
        } else {
            None
        }
    }

    fn landed_while_typing(&self, contact: &Contact) -> bool {
        match (contact.started_at, self.last_key_press) {
            (Some(started_at), Some(pressed_at)) => {
                started_at.saturating_sub(pressed_at) < self.options.typing_lockout
            }
            _ => false,
        }
    }
}

fn last_passthrough_slot(events: &[Event]) -> Option<i32> {
    events.iter().rev().find_map(|event| match event {
        Event::Slot(slot) => Some(*slot),
        _ => None,
    })
}
