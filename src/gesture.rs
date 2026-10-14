//! 中ボタンジェスチャーの判定ロジック (OS 非依存)。
//!
//! 低レベルマウスフックから受け取った入力を [`Gesture::handle`] に渡すと、
//! 元のイベントを握りつぶすかどうかと、代わりに送出すべき合成イベントを返す。
//!
//! 基本の流れ:
//! 1. 中ボタン down を握りつぶして「保留」状態に入る。
//! 2. 保留中の移動は握りつぶし (カーソル固定)、移動量を累積する。
//!    累積距離が閾値を超えたら、設定に応じてスクロールかドラッグに移行する。
//! 3. 閾値を超えずに up が来たら、保留していた中ボタン down/up を送出する (= 中クリック)。

/// 中ボタンを押したまま動かしたときの挙動。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveMode {
    /// 移動量をホイール (縦・横) に変換する。カーソルは固定。
    Scroll,
    /// 通常の中ボタンドラッグとして素通しする (CAD のパン操作など向け)。
    Drag,
}

/// ジェスチャー判定の設定値。
#[derive(Debug, Clone, PartialEq)]
pub struct GestureConfig {
    /// 押下＋移動時の挙動
    pub mode: MoveMode,
    /// 中クリックとみなさなくなる累積移動距離 (px)
    pub threshold: i32,
    /// 1px の移動あたりのホイール量 (120 で 1 ノッチ)
    pub sensitivity: f64,
    /// 横スクロールを有効にするか
    pub horizontal: bool,
    /// 縦スクロールの向きを反転するか
    pub invert_vertical: bool,
    /// 横スクロールの向きを反転するか
    pub invert_horizontal: bool,
    /// ホイール量を 120 (1 ノッチ) 単位に蓄積してから送出するか
    /// (高分解能ホイールを正しく扱えないアプリ向け)
    pub quantize: bool,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            mode: MoveMode::Scroll,
            threshold: 4,
            sensitivity: 10.0,
            horizontal: true,
            invert_vertical: false,
            invert_horizontal: false,
            quantize: false,
        }
    }
}

/// フックから渡す入力イベント。座標はスクリーン座標 (フックの `pt`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    MiddleDown { x: i32, y: i32 },
    MiddleUp,
    Move { x: i32, y: i32 },
}

/// 送出すべき合成イベント。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    MiddleDown,
    MiddleUp,
    /// 縦ホイール (正: 上方向 = 奥へ)
    Wheel(i32),
    /// 横ホイール (正: 右方向)
    HWheel(i32),
}

/// [`Gesture::handle`] の判定結果。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Decision {
    /// 元のイベントを握りつぶすか (true ならフックで 1 を返す)
    pub suppress: bool,
    /// 代わりに送出する合成イベント (先頭から順に送る)
    pub emit: Vec<Output>,
}

impl Decision {
    /// 元のイベントをそのまま通す。
    fn pass() -> Self {
        Self::default()
    }

    /// 元のイベントを握りつぶし、指定のイベントを送出する。
    fn suppress(emit: Vec<Output>) -> Self {
        Self { suppress: true, emit }
    }
}

/// 判定の内部状態。
#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    /// 中ボタンが押されていない
    Idle,
    /// 中ボタン down を保留中。`origin` は押下位置、`acc_*` は累積移動量
    Pending {
        origin: (i32, i32),
        acc_x: i32,
        acc_y: i32,
    },
    /// スクロール中。`rem_*` は未送出のホイール量の端数
    Scrolling {
        origin: (i32, i32),
        rem_x: f64,
        rem_y: f64,
    },
    /// 中ボタンドラッグを素通し中 (合成した down を送出済み)
    Dragging,
}

/// ホイール 1 ノッチ分の量 (WHEEL_DELTA)
const WHEEL_DELTA: f64 = 120.0;

/// 中ボタンジェスチャーの状態機械。
#[derive(Debug, Clone)]
pub struct Gesture {
    config: GestureConfig,
    state: State,
}

impl Gesture {
    /// 設定を指定して状態機械を作る。
    pub fn new(config: GestureConfig) -> Self {
        Self {
            config,
            state: State::Idle,
        }
    }

    /// 現在の状態の名前を返す (ログ表示用)。
    pub fn state_name(&self) -> &'static str {
        match self.state {
            State::Idle => "idle",
            State::Pending { .. } => "pending",
            State::Scrolling { .. } => "scrolling",
            State::Dragging => "dragging",
        }
    }

    /// 中ボタンが押されている間 (保留・スクロール・ドラッグ中) なら true。
    pub fn is_active(&self) -> bool {
        self.state != State::Idle
    }

    /// 入力イベントを 1 つ処理し、判定結果を返す。
    pub fn handle(&mut self, input: Input) -> Decision {
        match (self.state, input) {
            // --- 中ボタン down: 保留を開始する ---
            (State::Idle, Input::MiddleDown { x, y }) => {
                self.state = State::Pending {
                    origin: (x, y),
                    acc_x: 0,
                    acc_y: 0,
                };
                Decision::suppress(vec![])
            }
            // 押下中にさらに down が来るのは想定外 (別デバイス等)。素通しする
            (_, Input::MiddleDown { .. }) => Decision::pass(),

            // --- 保留中の移動: カーソルを固定して移動量を累積する ---
            (State::Pending { origin, acc_x, acc_y }, Input::Move { x, y }) => {
                // カーソルは origin に固定されているので、pt - origin がこの移動の差分
                let (dx, dy) = (x - origin.0, y - origin.1);
                let (acc_x, acc_y) = (acc_x + dx, acc_y + dy);
                let t = i64::from(self.config.threshold);
                let dist2 = i64::from(acc_x).pow(2) + i64::from(acc_y).pow(2);
                if dist2 < t * t {
                    // まだ閾値未満: 保留を続ける
                    self.state = State::Pending { origin, acc_x, acc_y };
                    return Decision::suppress(vec![]);
                }
                match self.config.mode {
                    MoveMode::Scroll => {
                        // 閾値までの移動分もスクロールに反映して、反応の遅れを感じさせない
                        self.state = State::Scrolling {
                            origin,
                            rem_x: 0.0,
                            rem_y: 0.0,
                        };
                        let emit = self.scroll(acc_x, acc_y);
                        Decision::suppress(emit)
                    }
                    MoveMode::Drag => {
                        // 保留していた down をここで送出し、以降の移動は素通しする。
                        // この移動自体は握りつぶす (down より先にカーソルが動かないように)
                        self.state = State::Dragging;
                        Decision::suppress(vec![Output::MiddleDown])
                    }
                }
            }

            // --- スクロール中の移動: ホイールに変換する ---
            (State::Scrolling { origin, .. }, Input::Move { x, y }) => {
                let emit = self.scroll(x - origin.0, y - origin.1);
                Decision::suppress(emit)
            }

            // --- 中ボタン up ---
            (State::Pending { .. }, Input::MiddleUp) => {
                // 閾値を超えずに離された: 中クリックとして送出する
                self.state = State::Idle;
                Decision::suppress(vec![Output::MiddleDown, Output::MiddleUp])
            }
            (State::Scrolling { .. }, Input::MiddleUp) => {
                // スクロールの終了。中ボタンのイベントは一切送らない
                self.state = State::Idle;
                Decision::suppress(vec![])
            }
            (State::Dragging, Input::MiddleUp) => {
                // 合成した down と順序が入れ替わらないよう、up も合成イベントとして送る
                self.state = State::Idle;
                Decision::suppress(vec![Output::MiddleUp])
            }
            // 押下を見ていない up (起動時に押されていた等) は素通しする
            (State::Idle, Input::MiddleUp) => Decision::pass(),

            // --- それ以外の移動 (通常時・ドラッグ中) は素通し ---
            (State::Idle | State::Dragging, Input::Move { .. }) => Decision::pass(),
        }
    }

    /// 状態を初期化する (一時停止時など)。
    #[allow(dead_code)] // トレイメニューの一時停止で使う予定
    pub fn reset(&mut self) {
        self.state = State::Idle;
    }

    /// 移動量 (dx, dy) をホイール量に変換し、送出すべきイベントを返す。
    /// 1 回で送り切れない端数は状態に持ち越す。
    fn scroll(&mut self, dx: i32, dy: i32) -> Vec<Output> {
        let State::Scrolling { origin, rem_x, rem_y } = self.state else {
            return vec![];
        };
        let cfg = &self.config;
        // トラックポイントを下に倒す (dy > 0) と下方向スクロール (ホイールは負) にする
        let sign_v = if cfg.invert_vertical { 1.0 } else { -1.0 };
        let sign_h = if cfg.invert_horizontal { -1.0 } else { 1.0 };
        let rem_y = rem_y + sign_v * f64::from(dy) * cfg.sensitivity;
        let rem_x = if cfg.horizontal {
            rem_x + sign_h * f64::from(dx) * cfg.sensitivity
        } else {
            0.0
        };

        let (out_y, rem_y) = split_wheel(rem_y, cfg.quantize);
        let (out_x, rem_x) = split_wheel(rem_x, cfg.quantize);
        self.state = State::Scrolling { origin, rem_x, rem_y };

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

/// 蓄積したホイール量を「今送る整数量」と「持ち越す端数」に分ける。
/// `quantize` が true なら 120 の倍数だけを送る。
fn split_wheel(value: f64, quantize: bool) -> (i32, f64) {
    let unit = if quantize { WHEEL_DELTA } else { 1.0 };
    // 0 方向への切り捨て (逆方向に動かしたときに端数が暴れないように)
    let out = (value / unit).trunc() * unit;
    (out as i32, value - out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 既定設定で、指定モードの状態機械を作る
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
        // 1px ずつ 2 回 (累積 2px < 閾値 4px)。カーソルは固定されているので pt は常に origin+1
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
        // 5px 下へ: 閾値超え → 5 * 10 = 50 の下方向スクロール
        let d = g.handle(Input::Move { x: 0, y: 5 });
        assert!(d.suppress);
        assert_eq!(d.emit, vec![Output::Wheel(-50)]);
        // 続けて 2px 下へ
        assert_eq!(g.handle(Input::Move { x: 0, y: 2 }).emit, vec![Output::Wheel(-20)]);
        // スクロール後の up では何も送らない
        assert_eq!(g.handle(Input::MiddleUp), Decision::suppress(vec![]));
    }

    #[test]
    fn horizontal_scroll_and_inversion() {
        let mut g = Gesture::new(GestureConfig {
            invert_vertical: true,
            invert_horizontal: true,
            ..GestureConfig::default()
        });
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        let d = g.handle(Input::Move { x: 4, y: 4 });
        assert_eq!(d.emit, vec![Output::Wheel(40), Output::HWheel(-40)]);
    }

    #[test]
    fn horizontal_disabled_ignores_dx() {
        let mut g = Gesture::new(GestureConfig {
            horizontal: false,
            ..GestureConfig::default()
        });
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        assert_eq!(g.handle(Input::Move { x: 10, y: 0 }).emit, vec![]);
        assert_eq!(g.handle(Input::Move { x: 0, y: -1 }).emit, vec![Output::Wheel(10)]);
    }

    #[test]
    fn quantize_accumulates_to_full_notches() {
        let mut g = Gesture::new(GestureConfig {
            quantize: true,
            ..GestureConfig::default()
        });
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        // 5px = 50: まだ 1 ノッチに満たない
        assert_eq!(g.handle(Input::Move { x: 0, y: -5 }).emit, vec![]);
        // さらに 8px = 80 → 計 130: 120 を送って 10 を持ち越す
        assert_eq!(g.handle(Input::Move { x: 0, y: -8 }).emit, vec![Output::Wheel(120)]);
        // さらに 11px = 110 → 計 120
        assert_eq!(g.handle(Input::Move { x: 0, y: -11 }).emit, vec![Output::Wheel(120)]);
    }

    #[test]
    fn fractional_sensitivity_carries_remainder() {
        let mut g = Gesture::new(GestureConfig {
            sensitivity: 0.5,
            threshold: 1,
            ..GestureConfig::default()
        });
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        // 1px = 0.5 → まだ 0
        assert_eq!(g.handle(Input::Move { x: 0, y: -1 }).emit, vec![]);
        // 計 1.0 → 1 を送る
        assert_eq!(g.handle(Input::Move { x: 0, y: -1 }).emit, vec![Output::Wheel(1)]);
    }

    #[test]
    fn drag_mode_emits_down_then_passes_moves() {
        let mut g = gesture(MoveMode::Drag);
        g.handle(Input::MiddleDown { x: 0, y: 0 });
        // 閾値超えの移動は握りつぶし、保留していた down を送る
        assert_eq!(
            g.handle(Input::Move { x: 5, y: 0 }),
            Decision::suppress(vec![Output::MiddleDown])
        );
        // 以降の移動は素通し
        assert_eq!(g.handle(Input::Move { x: 6, y: 0 }), Decision::pass());
        // up は合成イベントとして送る
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
        // 押下を見ていない up は素通し
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
