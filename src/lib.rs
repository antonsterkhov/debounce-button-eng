#![no_std]
#![doc = include_str!("../README.md")]

use embedded_hal::digital::InputPin;

/// Monotonic time in milliseconds.
pub type Millis = u64;

/// Electrical level that represents a pressed button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveLevel {
    /// The button is pressed when the input pin reads high.
    High,
    /// The button is pressed when the input pin reads low.
    Low,
}

impl ActiveLevel {
    #[inline]
    const fn state_from_high(self, is_high: bool) -> ButtonState {
        match (self, is_high) {
            (Self::High, true) | (Self::Low, false) => ButtonState::Pressed,
            (Self::High, false) | (Self::Low, true) => ButtonState::Released,
        }
    }
}

/// Stable button state after debounce filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonState {
    /// The button is pressed.
    Pressed,
    /// The button is released.
    Released,
}

impl ButtonState {
    /// Returns `true` when the state is [`ButtonState::Pressed`].
    #[inline]
    pub const fn is_pressed(self) -> bool {
        matches!(self, Self::Pressed)
    }

    /// Returns `true` when the state is [`ButtonState::Released`].
    #[inline]
    pub const fn is_released(self) -> bool {
        matches!(self, Self::Released)
    }
}

/// Button timing and polarity configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonConfig {
    /// Time the raw pin state must remain unchanged before it is accepted as
    /// the stable debounced state.
    pub debounce_ms: Millis,
    /// Maximum interval between two short releases that still counts as a
    /// double click.
    pub double_click_ms: Millis,
    /// Time after which a continuously pressed button reports a hold event.
    pub hold_ms: Millis,
    /// Electrical level that represents the pressed state.
    pub active_level: ActiveLevel,
}

impl ButtonConfig {
    /// Default debounce time.
    pub const DEFAULT_DEBOUNCE_MS: Millis = 20;
    /// Default double-click interval.
    pub const DEFAULT_DOUBLE_CLICK_MS: Millis = 300;
    /// Default hold threshold.
    pub const DEFAULT_HOLD_MS: Millis = 800;

    /// Creates a configuration for the common pull-up wiring where the pressed
    /// state is low.
    #[inline]
    pub const fn active_low() -> Self {
        Self {
            debounce_ms: Self::DEFAULT_DEBOUNCE_MS,
            double_click_ms: Self::DEFAULT_DOUBLE_CLICK_MS,
            hold_ms: Self::DEFAULT_HOLD_MS,
            active_level: ActiveLevel::Low,
        }
    }

    /// Creates a configuration where the pressed state is high.
    #[inline]
    pub const fn active_high() -> Self {
        Self {
            active_level: ActiveLevel::High,
            ..Self::active_low()
        }
    }

    /// Returns the configuration with a different debounce time.
    #[inline]
    pub const fn with_debounce_ms(mut self, debounce_ms: Millis) -> Self {
        self.debounce_ms = debounce_ms;
        self
    }

    /// Returns the configuration with a different double-click interval.
    #[inline]
    pub const fn with_double_click_ms(mut self, double_click_ms: Millis) -> Self {
        self.double_click_ms = double_click_ms;
        self
    }

    /// Returns the configuration with a different hold threshold.
    #[inline]
    pub const fn with_hold_ms(mut self, hold_ms: Millis) -> Self {
        self.hold_ms = hold_ms;
        self
    }
}

impl Default for ButtonConfig {
    #[inline]
    fn default() -> Self {
        Self::active_low()
    }
}

/// Events produced by one [`Button::update`] call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ButtonEvents {
    /// The stable state changed to pressed.
    pub pressed: bool,
    /// The stable state changed to released.
    pub released: bool,
    /// A short single click was confirmed.
    pub click: bool,
    /// Two short clicks completed inside the configured interval.
    pub double_click: bool,
    /// The button reached the configured hold threshold.
    pub hold: bool,
}

impl ButtonEvents {
    /// Returns `true` if at least one event flag is set.
    #[inline]
    pub const fn any(self) -> bool {
        self.pressed || self.released || self.click || self.double_click || self.hold
    }
}

/// Result of one button update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonUpdate {
    /// Current stable debounced state after the update.
    pub state: ButtonState,
    /// Events produced by this update.
    pub events: ButtonEvents,
    /// Current hold time while the button is pressed.
    pub held_ms: Option<Millis>,
    /// Total hold time when this update released the button.
    pub released_after_ms: Option<Millis>,
}

impl ButtonUpdate {
    #[inline]
    const fn idle(state: ButtonState, held_ms: Option<Millis>) -> Self {
        Self {
            state,
            events: ButtonEvents {
                pressed: false,
                released: false,
                click: false,
                double_click: false,
                hold: false,
            },
            held_ms,
            released_after_ms: None,
        }
    }
}

/// Debounced wrapper around an `embedded-hal` input pin.
///
/// Call [`Button::update`] regularly and pass a monotonic millisecond
/// timestamp. The timestamp can come from a hardware timer, a system tick, or
/// any other counter that never goes backwards.
pub struct Button<PIN> {
    pin: PIN,
    config: ButtonConfig,
    raw_state: ButtonState,
    raw_changed_at: Millis,
    stable_state: ButtonState,
    pressed_at: Option<Millis>,
    pending_click_at: Option<Millis>,
    hold_reported: bool,
}

impl<PIN> Button<PIN> {
    /// Creates an active-low button with default timings.
    #[inline]
    pub const fn new(pin: PIN) -> Self {
        Self::with_config(pin, ButtonConfig::active_low())
    }

    /// Creates a button with custom timings and polarity.
    #[inline]
    pub const fn with_config(pin: PIN, config: ButtonConfig) -> Self {
        Self {
            pin,
            config,
            raw_state: ButtonState::Released,
            raw_changed_at: 0,
            stable_state: ButtonState::Released,
            pressed_at: None,
            pending_click_at: None,
            hold_reported: false,
        }
    }

    /// Returns the current configuration.
    #[inline]
    pub const fn config(&self) -> ButtonConfig {
        self.config
    }

    /// Replaces the whole configuration.
    #[inline]
    pub fn set_config(&mut self, config: ButtonConfig) {
        self.config = config;
    }

    /// Returns the current debounce time.
    #[inline]
    pub const fn debounce_ms(&self) -> Millis {
        self.config.debounce_ms
    }

    /// Sets the debounce time.
    #[inline]
    pub fn set_debounce_ms(&mut self, debounce_ms: Millis) {
        self.config.debounce_ms = debounce_ms;
    }

    /// Returns the current double-click interval.
    #[inline]
    pub const fn double_click_ms(&self) -> Millis {
        self.config.double_click_ms
    }

    /// Sets the double-click interval.
    #[inline]
    pub fn set_double_click_ms(&mut self, double_click_ms: Millis) {
        self.config.double_click_ms = double_click_ms;
    }

    /// Returns the current hold threshold.
    #[inline]
    pub const fn hold_ms(&self) -> Millis {
        self.config.hold_ms
    }

    /// Sets the hold threshold.
    #[inline]
    pub fn set_hold_ms(&mut self, hold_ms: Millis) {
        self.config.hold_ms = hold_ms;
    }

    /// Returns the stable debounced state.
    #[inline]
    pub const fn state(&self) -> ButtonState {
        self.stable_state
    }

    /// Returns `true` if the stable debounced state is
    /// [`ButtonState::Pressed`].
    #[inline]
    pub const fn is_pressed(&self) -> bool {
        self.stable_state.is_pressed()
    }

    /// Returns `true` if the stable debounced state is
    /// [`ButtonState::Released`].
    #[inline]
    pub const fn is_released(&self) -> bool {
        self.stable_state.is_released()
    }

    /// Returns the current debounced hold time.
    #[inline]
    pub fn held_ms(&self, now_ms: Millis) -> Option<Millis> {
        self.pressed_at
            .map(|pressed_at| elapsed(now_ms, pressed_at))
    }

    /// Returns a mutable reference to the inner pin.
    #[inline]
    pub fn pin_mut(&mut self) -> &mut PIN {
        &mut self.pin
    }

    /// Returns the inner pin.
    #[inline]
    pub fn into_inner(self) -> PIN {
        self.pin
    }
}

impl<PIN> Button<PIN>
where
    PIN: InputPin,
{
    /// Reads the pin once, updates the debouncer, and returns the current state
    /// with events.
    ///
    /// This method does not block and does not wait for debounce to finish. It
    /// should be called regularly with a fresh monotonic timestamp in
    /// `now_ms`.
    ///
    /// One call:
    ///
    /// 1. Reads the physical pin through `InputPin::is_high`.
    /// 2. Converts the electrical level to [`ButtonState`] using
    ///    [`ActiveLevel`].
    /// 3. Records a raw state change and its timestamp.
    /// 4. Accepts a raw state as stable when it has been unchanged for at least
    ///    `debounce_ms`.
    /// 5. Emits `events.pressed` when the stable state becomes
    ///    [`ButtonState::Pressed`].
    /// 6. Emits `events.released` and `released_after_ms` when the stable state
    ///    becomes [`ButtonState::Released`].
    /// 7. Stores a pending single click after a short release.
    /// 8. Emits `events.double_click` if a second short click arrives inside
    ///    `double_click_ms`, without emitting an earlier `events.click`.
    /// 9. Emits `events.click` once if the pending click is not followed by a
    ///    second click before `double_click_ms` expires.
    /// 10. Updates `held_ms` while the button is pressed and emits
    ///     `events.hold` once the hold threshold is reached.
    ///
    /// Event flags are valid only for the returned update. Read the stable
    /// state from [`ButtonUpdate::state`].
    pub fn update(&mut self, now_ms: Millis) -> Result<ButtonUpdate, PIN::Error> {
        let raw_state = self.read_raw_state()?;

        if raw_state != self.raw_state {
            self.raw_state = raw_state;
            self.raw_changed_at = now_ms;
        }

        let mut update = ButtonUpdate::idle(self.stable_state, self.held_ms(now_ms));

        if self.stable_state != self.raw_state
            && elapsed(now_ms, self.raw_changed_at) >= self.config.debounce_ms
        {
            self.accept_stable_state(now_ms, &mut update);
        }

        if self.stable_state == ButtonState::Pressed {
            let held_ms = self.held_ms(now_ms).unwrap_or(0);
            update.held_ms = Some(held_ms);

            if !self.hold_reported && held_ms >= self.config.hold_ms {
                update.events.hold = true;
                self.hold_reported = true;
            }
        }

        self.report_pending_click(now_ms, &mut update);
        update.state = self.stable_state;
        Ok(update)
    }

    #[inline]
    fn read_raw_state(&mut self) -> Result<ButtonState, PIN::Error> {
        self.pin
            .is_high()
            .map(|is_high| self.config.active_level.state_from_high(is_high))
    }

    fn accept_stable_state(&mut self, now_ms: Millis, update: &mut ButtonUpdate) {
        self.stable_state = self.raw_state;

        match self.stable_state {
            ButtonState::Pressed => {
                self.pressed_at = Some(now_ms);
                self.hold_reported = false;
                update.events.pressed = true;
                update.held_ms = Some(0);
            }
            ButtonState::Released => {
                let held_ms = self
                    .pressed_at
                    .map(|pressed_at| elapsed(now_ms, pressed_at));

                self.pressed_at = None;
                self.hold_reported = false;
                update.events.released = true;
                update.held_ms = None;
                update.released_after_ms = held_ms;

                if held_ms.is_some_and(|held_ms| held_ms < self.config.hold_ms) {
                    self.register_short_release(now_ms, update);
                }
            }
        }
    }

    fn register_short_release(&mut self, now_ms: Millis, update: &mut ButtonUpdate) {
        if self.pending_click_at.is_some_and(|pending_click_at| {
            elapsed(now_ms, pending_click_at) <= self.config.double_click_ms
        }) {
            update.events.double_click = true;
            self.pending_click_at = None;
        } else {
            self.report_pending_click(now_ms, update);
            self.pending_click_at = Some(now_ms);
        }
    }

    fn report_pending_click(&mut self, now_ms: Millis, update: &mut ButtonUpdate) {
        if self.pending_click_at.is_some_and(|pending_click_at| {
            elapsed(now_ms, pending_click_at) > self.config.double_click_ms
        }) {
            update.events.click = true;
            self.pending_click_at = None;
        }
    }
}

#[inline]
const fn elapsed(now_ms: Millis, earlier_ms: Millis) -> Millis {
    now_ms.saturating_sub(earlier_ms)
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use embedded_hal_mock::eh1::{
        digital::{Mock as PinMock, State as PinState, Transaction as PinTransaction},
        MockError,
    };

    use super::*;

    fn config() -> ButtonConfig {
        ButtonConfig::active_low()
            .with_debounce_ms(10)
            .with_double_click_ms(200)
            .with_hold_ms(1000)
    }

    #[test]
    fn filters_bounce_and_reports_press_release() {
        let expectations = [
            PinTransaction::get(PinState::High),
            PinTransaction::get(PinState::Low),
            PinTransaction::get(PinState::High),
            PinTransaction::get(PinState::Low),
            PinTransaction::get(PinState::Low),
            PinTransaction::get(PinState::Low),
            PinTransaction::get(PinState::High),
            PinTransaction::get(PinState::High),
            PinTransaction::get(PinState::High),
            PinTransaction::get(PinState::High),
        ];
        let pin = PinMock::new(&expectations);
        let mut button = Button::with_config(pin, config());

        assert_eq!(button.update(0).unwrap().state, ButtonState::Released);
        assert!(!button.update(1).unwrap().events.any());
        assert!(!button.update(5).unwrap().events.any());
        assert!(!button.update(7).unwrap().events.any());
        assert!(!button.update(16).unwrap().events.any());

        let update = button.update(17).unwrap();
        assert_eq!(update.state, ButtonState::Pressed);
        assert!(update.events.pressed);
        assert_eq!(update.held_ms, Some(0));

        assert!(!button.update(30).unwrap().events.any());
        assert!(!button.update(35).unwrap().events.any());

        let update = button.update(40).unwrap();
        assert_eq!(update.state, ButtonState::Released);
        assert!(update.events.released);
        assert!(!update.events.click);
        assert!(!update.events.double_click);
        assert_eq!(update.released_after_ms, Some(23));

        let update = button.update(241).unwrap();
        assert_eq!(update.state, ButtonState::Released);
        assert!(update.events.click);
        assert!(!update.events.double_click);
        assert_eq!(update.released_after_ms, None);

        button.into_inner().done();
    }

    #[test]
    fn detects_double_click() {
        let expectations = [
            PinTransaction::get(PinState::Low),
            PinTransaction::get(PinState::Low),
            PinTransaction::get(PinState::High),
            PinTransaction::get(PinState::High),
            PinTransaction::get(PinState::Low),
            PinTransaction::get(PinState::Low),
            PinTransaction::get(PinState::High),
            PinTransaction::get(PinState::High),
        ];
        let pin = PinMock::new(&expectations);
        let mut button = Button::with_config(pin, config().with_debounce_ms(5).with_hold_ms(1000));

        assert!(!button.update(0).unwrap().events.any());
        assert!(button.update(5).unwrap().events.pressed);

        assert!(!button.update(20).unwrap().events.any());
        let update = button.update(25).unwrap();
        assert!(update.events.released);
        assert!(!update.events.click);
        assert!(!update.events.double_click);

        assert!(!button.update(80).unwrap().events.any());
        assert!(button.update(85).unwrap().events.pressed);

        assert!(!button.update(100).unwrap().events.any());
        let update = button.update(105).unwrap();
        assert!(update.events.released);
        assert!(!update.events.click);
        assert!(update.events.double_click);

        button.into_inner().done();
    }

    #[test]
    fn reports_hold_once_and_tracks_hold_time() {
        let expectations = [
            PinTransaction::get(PinState::Low),
            PinTransaction::get(PinState::Low),
            PinTransaction::get(PinState::Low),
            PinTransaction::get(PinState::Low),
            PinTransaction::get(PinState::Low),
            PinTransaction::get(PinState::High),
            PinTransaction::get(PinState::High),
        ];
        let pin = PinMock::new(&expectations);
        let mut button = Button::with_config(pin, config().with_debounce_ms(5).with_hold_ms(50));

        assert!(!button.update(0).unwrap().events.any());
        assert!(button.update(5).unwrap().events.pressed);

        let update = button.update(54).unwrap();
        assert_eq!(update.held_ms, Some(49));
        assert!(!update.events.hold);

        let update = button.update(55).unwrap();
        assert_eq!(update.held_ms, Some(50));
        assert!(update.events.hold);

        let update = button.update(80).unwrap();
        assert_eq!(update.held_ms, Some(75));
        assert!(!update.events.hold);

        assert!(!button.update(100).unwrap().events.any());
        let update = button.update(105).unwrap();
        assert!(update.events.released);
        assert!(!update.events.click);
        assert_eq!(update.released_after_ms, Some(100));

        button.into_inner().done();
    }

    #[test]
    fn supports_active_high_and_runtime_debounce_setting() {
        let expectations = [
            PinTransaction::get(PinState::Low),
            PinTransaction::get(PinState::High),
            PinTransaction::get(PinState::High),
        ];
        let pin = PinMock::new(&expectations);
        let mut button = Button::with_config(pin, ButtonConfig::active_high());

        button.set_debounce_ms(3);
        assert_eq!(button.debounce_ms(), 3);

        assert_eq!(button.update(0).unwrap().state, ButtonState::Released);
        assert!(!button.update(1).unwrap().events.any());

        let update = button.update(4).unwrap();
        assert_eq!(update.state, ButtonState::Pressed);
        assert!(update.events.pressed);

        button.into_inner().done();
    }

    #[test]
    fn propagates_pin_errors() {
        let expectations =
            [PinTransaction::get(PinState::High).with_error(MockError::Io(ErrorKind::Other))];
        let pin = PinMock::new(&expectations);
        let mut button = Button::new(pin);

        assert!(button.update(0).is_err());

        button.into_inner().done();
    }
}
