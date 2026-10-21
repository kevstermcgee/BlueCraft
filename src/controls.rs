//! BlueCraft's game-owned controller bindings, after the engine's focus/menu gate.
use bluecraft::PALETTE;
use vesper3d::viewer::gamepad::{Button, GamepadFrame};

const TRIGGER_THRESHOLD: f32 = 0.35;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GamepadActions {
    /// Held RT pressure/button state, suitable for repeated mining.
    pub mine: bool,
    /// Held LT pressure/button state, suitable for repeated placement.
    pub place: bool,
    /// Digital press edges: feed separate Lifecycle edge bits so a short tap
    /// survives rendered frames that run no simulation tick.
    pub mine_pressed: bool,
    pub place_pressed: bool,
    pub pick: bool,
    pub guide: bool,
    pub crouch: bool,
    pub slot: Option<u8>,
    pub save: bool,
    pub load: bool,
}

/// Call once per rendered frame using `input.gamepad()`. The engine already owns
/// A/jump, L3/sprint, both sticks, Start/pause and paused B/menu navigation.
/// BlueCraft adds LT/place, RT/mine, LB/RB and D-pad left/right/material,
/// X/R3/pick, Y/View/guide, held B/crouch and D-pad up/down/checkpoint save/load.
pub fn gamepad_actions(frame: &GamepadFrame, selected: u8, accepting_input: bool) -> GamepadActions {
    if !accepting_input {
        return GamepadActions::default();
    }
    let sample = PadSample::read(frame.triggers, |button| frame.down(button), |button| frame.pressed(button));
    sample.actions(selected)
}

/// Separating the public frame adapter from this value transform lets tests
/// exercise taps/holds without a controller, window or another device poll.
#[derive(Clone, Copy, Debug, Default)]
struct PadSample {
    triggers: [f32; 2],
    trigger_down: [bool; 2],
    trigger_pressed: [bool; 2],
    previous: bool,
    next: bool,
    pick: bool,
    guide: bool,
    crouch: bool,
    save: bool,
    load: bool,
}

impl PadSample {
    fn read(triggers: [f32; 2], down: impl Fn(Button) -> bool, pressed: impl Fn(Button) -> bool) -> Self {
        Self {
            triggers,
            trigger_down: [down(Button::LeftTrigger2), down(Button::RightTrigger2)],
            trigger_pressed: [pressed(Button::LeftTrigger2), pressed(Button::RightTrigger2)],
            previous: pressed(Button::LeftTrigger) || pressed(Button::DPadLeft),
            next: pressed(Button::RightTrigger) || pressed(Button::DPadRight),
            pick: pressed(Button::West) || pressed(Button::RightThumb),
            guide: pressed(Button::North) || pressed(Button::Select),
            crouch: down(Button::East),
            save: pressed(Button::DPadUp),
            load: pressed(Button::DPadDown),
        }
    }

    fn actions(self, selected: u8) -> GamepadActions {
        let trigger = |index: usize| {
            // Some mapped devices report pressure, others report a digital
            // trigger. Keep a short press/release that occurs within one poll.
            (self.triggers[index].is_finite() && self.triggers[index] >= TRIGGER_THRESHOLD)
                || self.trigger_down[index]
                || self.trigger_pressed[index]
        };
        let change = i32::from(self.next) - i32::from(self.previous);
        let slot = (change != 0).then(|| (i32::from(selected) + change).rem_euclid(PALETTE.len() as i32) as u8);
        GamepadActions {
            mine: trigger(1),
            place: trigger(0),
            mine_pressed: self.trigger_pressed[1],
            place_pressed: self.trigger_pressed[0],
            pick: self.pick,
            guide: self.guide,
            crouch: self.crouch,
            slot,
            // Opposed D-pad presses must not save and immediately reload.
            save: self.save && !self.load,
            load: self.load && !self.save,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analog_triggers_edit_the_expected_side_only_past_the_threshold() {
        let mut sample = PadSample { triggers: [0.34, 0.35], ..Default::default() };
        let actions = sample.actions(0);
        assert!(!actions.place);
        assert!(actions.mine);
        sample.triggers = [1., 0.];
        assert!(sample.actions(0).place);
        assert!(!sample.actions(0).mine);
        sample.triggers = [f32::NAN, f32::INFINITY];
        assert_eq!(sample.actions(0), GamepadActions::default());
    }

    #[test]
    fn digital_trigger_holds_and_short_taps_are_kept() {
        let held = PadSample { trigger_down: [true, true], ..Default::default() };
        assert!(held.actions(0).mine && held.actions(0).place);
        assert!(!held.actions(0).mine_pressed && !held.actions(0).place_pressed);
        let tap = PadSample { trigger_pressed: [false, true], ..Default::default() };
        assert!(tap.actions(0).mine);
        assert!(tap.actions(0).mine_pressed);
        assert!(!tap.actions(0).place_pressed);
        assert!(!PadSample::default().actions(0).mine);
    }

    #[test]
    fn short_trigger_edges_survive_frames_without_a_fixed_tick() {
        use vesper3d::viewer::devkit::InputAccumulator;
        const MINE: u32 = 4;
        const PLACE: u32 = 8;
        let mut accumulator = InputAccumulator::<bool>::new();
        let tap = PadSample::read([0.; 2], |_| false, |button| button == Button::RightTrigger2).actions(0);
        accumulator.feed(tap.mine, u32::from(tap.mine_pressed) * MINE, [0.; 2]);
        // At 144 Hz, release may arrive before the next 60 Hz simulation tick.
        accumulator.feed(false, 0, [0.; 2]);
        let tick = accumulator.take_tick();
        assert!(!tick.held);
        assert!(tick.pressed(MINE));
        assert!(!accumulator.take_tick().pressed(MINE), "deliver a tap on exactly one tick");
        let tap = PadSample::read([0.; 2], |_| false, |button| button == Button::LeftTrigger2).actions(0);
        accumulator.feed(tap.place, u32::from(tap.place_pressed) * PLACE, [0.; 2]);
        accumulator.feed(false, 0, [0.; 2]);
        assert!(accumulator.take_tick().pressed(PLACE));
    }

    #[test]
    fn bumpers_wrap_once_per_press_and_opposites_cancel() {
        let previous = PadSample { previous: true, ..Default::default() };
        let next = PadSample { next: true, ..Default::default() };
        assert_eq!(previous.actions(0).slot, Some(PALETTE.len() as u8 - 1));
        assert_eq!(next.actions(PALETTE.len() as u8 - 1).slot, Some(0));
        assert_eq!(next.actions(4).slot, Some(5));
        assert_eq!(PadSample::default().actions(5).slot, None, "a held bumper has no second press edge");
        assert_eq!(PadSample { previous: true, next: true, ..Default::default() }.actions(3).slot, None);
    }

    #[test]
    fn face_buttons_and_checkpoint_requests_are_edges_but_crouch_is_held() {
        let pressed = PadSample { pick: true, guide: true, crouch: true, save: true, ..Default::default() };
        let actions = pressed.actions(0);
        assert!(actions.pick && actions.guide && actions.crouch && actions.save);
        assert!(!actions.load);
        let held = PadSample { crouch: true, ..Default::default() }.actions(0);
        assert!(held.crouch);
        assert!(!held.pick && !held.guide && !held.save && !held.load);
        let conflicting = PadSample { save: true, load: true, ..Default::default() }.actions(0);
        assert!(!conflicting.save && !conflicting.load);
    }

    #[test]
    fn public_adapter_reads_pressure_and_suppresses_gameplay_while_paused() {
        let mut frame = GamepadFrame::default();
        frame.triggers = [0.8, 0.9];
        let active = gamepad_actions(&frame, 0, true);
        assert!(active.mine && active.place);
        assert_eq!(gamepad_actions(&frame, 0, false), GamepadActions::default());
        // The engine resumes gameplay only after its neutral-device gate.
        assert_eq!(gamepad_actions(&GamepadFrame::default(), 0, true), GamepadActions::default());
    }

    #[test]
    fn xbox_button_snapshots_follow_bindings_and_held_bumpers_do_not_repeat() {
        let snapshot = |down: &[Button], pressed: &[Button]| {
            PadSample::read([0.; 2], |button| down.contains(&button), |button| pressed.contains(&button))
        };
        let press = snapshot(&[Button::East], &[Button::LeftTrigger, Button::West, Button::North, Button::DPadUp]);
        let actions = press.actions(0);
        assert_eq!(actions.slot, Some(PALETTE.len() as u8 - 1));
        assert!(actions.crouch && actions.pick && actions.guide && actions.save);
        let held = snapshot(&[Button::LeftTrigger, Button::West, Button::North, Button::East], &[]);
        assert_eq!(held.actions(8), GamepadActions { crouch: true, ..Default::default() });
        let alternate = snapshot(&[], &[Button::DPadRight, Button::RightThumb, Button::Select, Button::DPadDown]);
        let actions = alternate.actions(8);
        assert_eq!(actions.slot, Some(0));
        assert!(actions.pick && actions.guide && actions.load);
    }
}
