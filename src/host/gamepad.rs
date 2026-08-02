use fanticon::system::ControllerState;
use gilrs::{Axis, Button, GamepadId, Gilrs};

const STICK_THRESHOLD: f32 = 0.5;

pub struct GamepadInput {
    gilrs: Option<Gilrs>,
    slots: [Option<GamepadId>; 2],
    suppressed: [u8; 2],
}

impl GamepadInput {
    pub fn new() -> Self {
        Self {
            gilrs: Gilrs::new()
                .map_err(|error| eprintln!("Fanticon gamepads disabled: {error}"))
                .ok(),
            slots: [None; 2],
            suppressed: [0; 2],
        }
    }

    pub fn poll(&mut self) -> [u8; 2] {
        let Some(gilrs) = &mut self.gilrs else { return [0; 2] };
        while gilrs.next_event().is_some() {}

        let connected = gilrs.gamepads().map(|(id, _)| id).collect::<Vec<_>>();
        for slot in &mut self.slots {
            if slot.is_some_and(|id| !connected.contains(&id)) {
                *slot = None;
            }
        }
        for id in connected {
            if self.slots.contains(&Some(id)) {
                continue;
            }
            if let Some(slot) = self.slots.iter_mut().find(|slot| slot.is_none()) {
                *slot = Some(id);
            }
        }

        let mut states = [0; 2];
        for (slot, id) in self.slots.iter().copied().enumerate() {
            let Some(gamepad) = id.and_then(|id| gilrs.connected_gamepad(id)) else { continue };
            let physical =
                mapped_state(|button| gamepad.is_pressed(button), |axis| gamepad.value(axis));
            self.suppressed[slot] &= physical;
            states[slot] = physical & !self.suppressed[slot];
        }
        states
    }

    pub fn suppress_held_inputs(&mut self) {
        self.suppressed = [u8::MAX; 2];
    }
}

fn mapped_state(is_pressed: impl Fn(Button) -> bool, axis_value: impl Fn(Axis) -> f32) -> u8 {
    let mut state = 0;
    let x = axis_value(Axis::LeftStickX);
    let y = axis_value(Axis::LeftStickY);
    if is_pressed(Button::DPadUp) || y > STICK_THRESHOLD {
        state |= ControllerState::UP;
    }
    if is_pressed(Button::DPadDown) || y < -STICK_THRESHOLD {
        state |= ControllerState::DOWN;
    }
    if is_pressed(Button::DPadLeft) || x < -STICK_THRESHOLD {
        state |= ControllerState::LEFT;
    }
    if is_pressed(Button::DPadRight) || x > STICK_THRESHOLD {
        state |= ControllerState::RIGHT;
    }
    if is_pressed(Button::South) {
        state |= ControllerState::A;
    }
    if is_pressed(Button::East) {
        state |= ControllerState::B;
    }
    if is_pressed(Button::Select) {
        state |= ControllerState::SELECT;
    }
    if is_pressed(Button::Start) {
        state |= ControllerState::START;
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_buttons_and_left_stick_map_to_fanticon_bits() {
        let state = mapped_state(
            |button| matches!(button, Button::South | Button::Start),
            |axis| match axis {
                Axis::LeftStickX => -0.75,
                Axis::LeftStickY => 0.8,
                _ => 0.0,
            },
        );
        assert_eq!(
            state,
            ControllerState::UP
                | ControllerState::LEFT
                | ControllerState::A
                | ControllerState::START
        );
    }

    #[test]
    fn stick_dead_zone_does_not_create_directions() {
        assert_eq!(mapped_state(|_| false, |_| STICK_THRESHOLD), 0);
    }
}
