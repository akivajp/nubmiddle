//! Middle-button gesture recognition (OS-independent).
//!
//! Feed input from the low-level mouse hook to [`Gesture::handle`], and it returns
//! whether to swallow the original event and which synthetic events to send instead.
//!
//! Basic flow:
//! 1. Swallow the middle-button down and enter the "pending" state.
//! 2. While pending, swallow moves (keeping the cursor still) and accumulate them.
//!    Once the distance exceeds the threshold, switch to scrolling or dragging depending on the mode.
//! 3. If the button is released before that, send the held middle down/up (= a middle click).

/// What happens when the pointer is moved while the middle button is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveMode {
    /// Convert movement into wheel events (vertical / horizontal). The cursor stays still.
    Scroll,
    /// Pass it through as a normal middle-button drag (e.g. for panning in CAD).
    Drag,
}

/// Gesture recognition settings.
#[derive(Debug, Clone, PartialEq)]
pub struct GestureConfig {
    /// Behavior on press + move
    pub mode: MoveMode,
    /// Accumulated distance (px) beyond which it is no longer a middle click
    pub threshold: i32,
    /// Wheel amount per pixel of movement (120 = one notch)
    pub sensitivity: f64,
    /// Enable horizontal scrolling
    pub horizontal: bool,
    /// Scroll only along the dominant axis of the movement that started scrolling.
    /// Small sideways drift otherwise mixes horizontal wheel events into vertical scrolling,
    /// which some apps (e.g. Windows Settings) turn into a violently shaking scroll position
    pub axis_lock: bool,
    /// Invert vertical scroll direction
    pub invert_vertical: bool,
    /// Invert horizontal scroll direction
    pub invert_horizontal: bool,
    /// Accumulate the wheel amount and send it only in units of 120 (one notch)
    /// (for apps that mishandle high-resolution wheel input)
    pub quantize: bool,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            mode: MoveMode::Scroll,
            threshold: 4,
            sensitivity: 10.0,
            horizontal: true,
            axis_lock: true,
            invert_vertical: false,
            invert_horizontal: false,
            quantize: false,
        }
    }
}

/// Input event from the hook. Coordinates are screen coordinates (the hook's `pt`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    MiddleDown { x: i32, y: i32 },
    MiddleUp,
    Move { x: i32, y: i32 },
}

/// Synthetic event to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    MiddleDown,
    MiddleUp,
    /// Vertical wheel (positive: up = away from the user)
    Wheel(i32),
    /// Horizontal wheel (positive: right)
    HWheel(i32),
}

/// Result of [`Gesture::handle`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Decision {
    /// Whether to swallow the original event (if true, the hook returns 1)
    pub suppress: bool,
    /// Synthetic events to send instead, in order
    pub emit: Vec<Output>,
}

impl Decision {
    /// Let the original event through unchanged.
    fn pass() -> Self {
        Self::default()
    }

    /// Swallow the original event and send the given events.
    fn suppress(emit: Vec<Output>) -> Self {
        Self {
            suppress: true,
            emit,
        }
    }
}

/// Axes a scroll gesture is allowed to move along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axes {
    Both,
    Vertical,
    Horizontal,
}

/// Internal state.
#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    /// The middle button is not pressed
    Idle,
    /// Holding the middle down. `origin` is where it was pressed, `acc_*` is the accumulated movement
    Pending {
        origin: (i32, i32),
        acc_x: i32,
        acc_y: i32,
    },
    /// Scrolling along `axes`. `rem_*` is the remainder of wheel amount not yet sent
    Scrolling {
        origin: (i32, i32),
        axes: Axes,
        rem_x: f64,
        rem_y: f64,
    },
    /// Passing through a middle-button drag (the synthetic down has been sent)
    Dragging,
}

/// Wheel amount of one notch (WHEEL_DELTA)
const WHEEL_DELTA: f64 = 120.0;

/// State machine for the middle-button gesture.
#[derive(Debug, Clone)]
pub struct Gesture {
    config: GestureConfig,
    state: State,
}

impl Gesture {
    /// Creates a state machine with the given settings.
    pub fn new(config: GestureConfig) -> Self {
        Self {
            config,
            state: State::Idle,
        }
    }

    /// Returns the name of the current state (for logging).
    pub fn state_name(&self) -> &'static str {
        match self.state {
            State::Idle => "idle",
            State::Pending { .. } => "pending",
            State::Scrolling { .. } => "scrolling",
            State::Dragging => "dragging",
        }
    }

    /// True while the middle button is held (pending, scrolling or dragging).
    pub fn is_active(&self) -> bool {
        self.state != State::Idle
    }

    /// Axes being scrolled, or None when not scrolling (e.g. to show a matching cursor).
    pub fn scroll_axes(&self) -> Option<Axes> {
        match self.state {
            State::Scrolling { axes, .. } => Some(axes),
            _ => None,
        }
    }

    /// Processes one input event and returns the decision.
    pub fn handle(&mut self, input: Input) -> Decision {
        match (self.state, input) {
            // --- Middle down: start holding ---
            (State::Idle, Input::MiddleDown { x, y }) => {
                self.state = State::Pending {
                    origin: (x, y),
                    acc_x: 0,
                    acc_y: 0,
                };
                Decision::suppress(vec![])
            }
            // Another down while held is unexpected (e.g. another device). Let it through
            (_, Input::MiddleDown { .. }) => Decision::pass(),

            // --- Move while pending: keep the cursor still and accumulate ---
            (
                State::Pending {
                    origin,
                    acc_x,
                    acc_y,
                },
                Input::Move { x, y },
            ) => {
                // The cursor is pinned at origin, so pt - origin is this move's delta
                let (dx, dy) = (x - origin.0, y - origin.1);
                let (acc_x, acc_y) = (acc_x + dx, acc_y + dy);
                let t = i64::from(self.config.threshold);
                let dist2 = i64::from(acc_x).pow(2) + i64::from(acc_y).pow(2);
                if dist2 < t * t {
                    // Still below the threshold: keep holding
                    self.state = State::Pending {
                        origin,
                        acc_x,
                        acc_y,
                    };
                    return Decision::suppress(vec![]);
                }
                match self.config.mode {
                    MoveMode::Scroll => {
                        // Include the movement up to the threshold so scrolling does not feel delayed
                        self.state = State::Scrolling {
                            origin,
                            axes: self.choose_axes(acc_x, acc_y),
                            rem_x: 0.0,
                            rem_y: 0.0,
                        };
                        let emit = self.scroll(acc_x, acc_y);
                        Decision::suppress(emit)
                    }
                    MoveMode::Drag => {
                        // Send the held down now and let subsequent moves through.
                        // Swallow this move itself (so the cursor does not move before the down)
                        self.state = State::Dragging;
                        Decision::suppress(vec![Output::MiddleDown])
                    }
                }
            }

            // --- Move while scrolling: convert to wheel ---
            (State::Scrolling { origin, .. }, Input::Move { x, y }) => {
                let emit = self.scroll(x - origin.0, y - origin.1);
                Decision::suppress(emit)
            }

            // --- Middle up ---
            (State::Pending { .. }, Input::MiddleUp) => {
                // Released below the threshold: send a middle click
                self.state = State::Idle;
                Decision::suppress(vec![Output::MiddleDown, Output::MiddleUp])
            }
            (State::Scrolling { .. }, Input::MiddleUp) => {
                // End of scrolling. Send no middle-button events at all
                self.state = State::Idle;
                Decision::suppress(vec![])
            }
            (State::Dragging, Input::MiddleUp) => {
                // Send the up as a synthetic event too, so it cannot overtake the synthetic down
                self.state = State::Idle;
                Decision::suppress(vec![Output::MiddleUp])
            }
            // Let through an up whose down we never saw (e.g. held at startup)
            (State::Idle, Input::MiddleUp) => Decision::pass(),

            // --- Other moves (idle / dragging) pass through ---
            (State::Idle | State::Dragging, Input::Move { .. }) => Decision::pass(),
        }
    }

    /// Resets the state (e.g. when paused).
    pub fn reset(&mut self) {
        self.state = State::Idle;
    }

    /// Decides the scroll axes from the movement accumulated until the threshold was crossed.
    fn choose_axes(&self, acc_x: i32, acc_y: i32) -> Axes {
        let cfg = &self.config;
        if !cfg.horizontal {
            Axes::Vertical
        } else if !cfg.axis_lock {
            Axes::Both
        } else if acc_x.abs() > acc_y.abs() {
            Axes::Horizontal
        } else {
            // Ties go to vertical, which is by far the more common direction
            Axes::Vertical
        }
    }

    /// Converts movement (dx, dy) into wheel amounts and returns the events to send.
    /// Any remainder that cannot be sent yet is carried over in the state.
    fn scroll(&mut self, dx: i32, dy: i32) -> Vec<Output> {
        let State::Scrolling {
            origin,
            axes,
            rem_x,
            rem_y,
        } = self.state
        else {
            return vec![];
        };
        let cfg = &self.config;
        // Pushing the TrackPoint down (dy > 0) scrolls down (negative wheel)
        let sign_v = if cfg.invert_vertical { 1.0 } else { -1.0 };
        let sign_h = if cfg.invert_horizontal { -1.0 } else { 1.0 };
        // Movement along a disabled axis is dropped
        let rem_y = if axes == Axes::Horizontal {
            0.0
        } else {
            rem_y + sign_v * f64::from(dy) * cfg.sensitivity
        };
        let rem_x = if axes == Axes::Vertical {
            0.0
        } else {
            rem_x + sign_h * f64::from(dx) * cfg.sensitivity
        };

        let (out_y, rem_y) = split_wheel(rem_y, cfg.quantize);
        let (out_x, rem_x) = split_wheel(rem_x, cfg.quantize);
        self.state = State::Scrolling {
            origin,
            axes,
            rem_x,
            rem_y,
        };

        let mut emit = Vec::with_capacity(2);
        if out_y != 0 {
            emit.push(Output::Wheel(out_y));
        }
        if out_x != 0 {
            emit.push(Output::HWheel(out_x));
        }
        emit
    }
}

/// Splits an accumulated wheel amount into "integer amount to send now" and "remainder to carry".
/// If `quantize` is true, only multiples of 120 are sent.
fn split_wheel(value: f64, quantize: bool) -> (i32, f64) {
    let unit = if quantize { WHEEL_DELTA } else { 1.0 };
    // Truncate toward zero (so the remainder behaves when the direction reverses)
    let out = (value / unit).trunc() * unit;
    (out as i32, value - out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a state machine with default settings and the given mode
    fn gesture(mode: MoveMode) -> Gesture {
        Gesture::new(GestureConfig {
            mode,
            ..GestureConfig::default()
        })
    }

    #[test]
    fn click_without_move_emits_middle_click() {
        let mut g = gesture(MoveMode::Scroll);
        assert_eq!(
            g.handle(Input::MiddleDown { x: 100, y: 100 }),
            Decision::suppress(vec![])
        );
        assert_eq!(
            g.handle(Input::MiddleUp),
            Decision::suppress(vec![Output::MiddleDown, Output::MiddleUp])
        );
        assert!(!g.is_active());
    }

    #[test]
    fn small_jitter_below_threshold_is_still_click() {
        let mut g = gesture(MoveMode::Scroll);
        g.handle(Input::MiddleDown { x: 100, y: 100 });
        // Two 1px moves (2px total < 4px threshold). The cursor is pinned, so pt is always origin+1
        assert_eq!(
            g.handle(Input::Move { x: 101, y: 100 }),
            Decision::suppress(vec![])
        );
        assert_eq!(
            g.handle(Input::Move { x: 101, y: 100 }),
            Decision::suppress(vec![])
        );
        assert_eq!(
            g.handle(Input::MiddleUp).emit,
            vec![Output::MiddleDown, Output::MiddleUp]
        );
    }

    #[test]
    fn moving_down_scrolls_down() {
        let mut g = gesture(MoveMode::Scroll);
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        // 5px down: over the threshold -> scroll down by 5 * 10 = 50
        let d = g.handle(Input::Move { x: 0, y: 5 });
        assert!(d.suppress);
        assert_eq!(d.emit, vec![Output::Wheel(-50)]);
        // Then 2px further down
        assert_eq!(
            g.handle(Input::Move { x: 0, y: 2 }).emit,
            vec![Output::Wheel(-20)]
        );
        // The up after scrolling sends nothing
        assert_eq!(g.handle(Input::MiddleUp), Decision::suppress(vec![]));
    }

    #[test]
    fn horizontal_scroll_and_inversion() {
        let mut g = Gesture::new(GestureConfig {
            axis_lock: false,
            invert_vertical: true,
            invert_horizontal: true,
            ..GestureConfig::default()
        });
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        let d = g.handle(Input::Move { x: 4, y: 4 });
        assert_eq!(d.emit, vec![Output::Wheel(40), Output::HWheel(-40)]);
    }

    #[test]
    fn axis_lock_vertical_drops_sideways_drift() {
        let mut g = gesture(MoveMode::Scroll);
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        // Mostly upward: locked to vertical
        assert_eq!(
            g.handle(Input::Move { x: -1, y: -5 }).emit,
            vec![Output::Wheel(50)]
        );
        // Sideways drift (even larger than the vertical part) sends no horizontal wheel
        assert_eq!(
            g.handle(Input::Move { x: -3, y: -2 }).emit,
            vec![Output::Wheel(20)]
        );
        assert_eq!(g.handle(Input::Move { x: 2, y: 0 }).emit, vec![]);
    }

    #[test]
    fn axis_lock_horizontal_drops_vertical_drift() {
        let mut g = gesture(MoveMode::Scroll);
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        // Mostly rightward: locked to horizontal
        assert_eq!(
            g.handle(Input::Move { x: 5, y: 1 }).emit,
            vec![Output::HWheel(50)]
        );
        assert_eq!(
            g.handle(Input::Move { x: 1, y: 4 }).emit,
            vec![Output::HWheel(10)]
        );
    }

    #[test]
    fn axis_lock_is_per_gesture() {
        let mut g = gesture(MoveMode::Scroll);
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        g.handle(Input::Move { x: 0, y: 5 });
        g.handle(Input::MiddleUp);
        // The next gesture decides its axis again
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        assert_eq!(
            g.handle(Input::Move { x: 5, y: 0 }).emit,
            vec![Output::HWheel(50)]
        );
    }

    #[test]
    fn scroll_axes_reports_locked_axis() {
        let mut g = gesture(MoveMode::Scroll);
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        // Not scrolling yet while pending
        assert_eq!(g.scroll_axes(), None);
        g.handle(Input::Move { x: 0, y: 5 });
        assert_eq!(g.scroll_axes(), Some(Axes::Vertical));
        g.handle(Input::MiddleUp);
        assert_eq!(g.scroll_axes(), None);

        let mut g = Gesture::new(GestureConfig {
            axis_lock: false,
            ..GestureConfig::default()
        });
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        g.handle(Input::Move { x: 0, y: 5 });
        assert_eq!(g.scroll_axes(), Some(Axes::Both));
    }

    #[test]
    fn horizontal_disabled_ignores_dx() {
        let mut g = Gesture::new(GestureConfig {
            horizontal: false,
            ..GestureConfig::default()
        });
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        assert_eq!(g.handle(Input::Move { x: 10, y: 0 }).emit, vec![]);
        assert_eq!(
            g.handle(Input::Move { x: 0, y: -1 }).emit,
            vec![Output::Wheel(10)]
        );
    }

    #[test]
    fn quantize_accumulates_to_full_notches() {
        let mut g = Gesture::new(GestureConfig {
            quantize: true,
            ..GestureConfig::default()
        });
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        // 5px = 50: not yet one notch
        assert_eq!(g.handle(Input::Move { x: 0, y: -5 }).emit, vec![]);
        // 8px more = 80 -> 130 total: send 120 and carry 10
        assert_eq!(
            g.handle(Input::Move { x: 0, y: -8 }).emit,
            vec![Output::Wheel(120)]
        );
        // 11px more = 110 -> 120 total
        assert_eq!(
            g.handle(Input::Move { x: 0, y: -11 }).emit,
            vec![Output::Wheel(120)]
        );
    }

    #[test]
    fn fractional_sensitivity_carries_remainder() {
        let mut g = Gesture::new(GestureConfig {
            sensitivity: 0.5,
            threshold: 1,
            ..GestureConfig::default()
        });
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        // 1px = 0.5 -> still 0
        assert_eq!(g.handle(Input::Move { x: 0, y: -1 }).emit, vec![]);
        // 1.0 total -> send 1
        assert_eq!(
            g.handle(Input::Move { x: 0, y: -1 }).emit,
            vec![Output::Wheel(1)]
        );
    }

    #[test]
    fn drag_mode_emits_down_then_passes_moves() {
        let mut g = gesture(MoveMode::Drag);
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        // The move crossing the threshold is swallowed and the held down is sent
        assert_eq!(
            g.handle(Input::Move { x: 5, y: 0 }),
            Decision::suppress(vec![Output::MiddleDown])
        );
        // Subsequent moves pass through
        assert_eq!(g.handle(Input::Move { x: 6, y: 0 }), Decision::pass());
        // The up is sent as a synthetic event
        assert_eq!(
            g.handle(Input::MiddleUp),
            Decision::suppress(vec![Output::MiddleUp])
        );
        assert!(!g.is_active());
    }

    #[test]
    fn idle_events_pass_through() {
        let mut g = gesture(MoveMode::Scroll);
        assert_eq!(g.handle(Input::Move { x: 1, y: 1 }), Decision::pass());
        // An up whose down we never saw passes through
        assert_eq!(g.handle(Input::MiddleUp), Decision::pass());
    }

    #[test]
    fn reset_returns_to_idle() {
        let mut g = gesture(MoveMode::Scroll);
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        assert!(g.is_active());
        g.reset();
        assert_eq!(g.state_name(), "idle");
    }

    #[test]
    fn split_wheel_truncates_toward_zero() {
        assert_eq!(split_wheel(-2.5, false), (-2, -0.5));
        assert_eq!(split_wheel(250.0, true), (240, 10.0));
        assert_eq!(split_wheel(-250.0, true), (-240, -10.0));
    }
}
