//! タイマーの計算だけを持つ。Tauri には依存しない。時刻はすべて引数で受け取る。
use serde::{Deserialize, Serialize};

pub const BLOCKS: u64 = 20;
pub const SEGMENTS_DEFAULT: u8 = 20;
pub const SEGMENTS_MIN: u8 = 2;
pub const SEGMENTS_MAX: u8 = 60;
pub const COLOR_AUTO: &str = "auto";
pub const LEVEL_COLORS: [&str; 5] = ["blue", "green", "yellow", "orange", "red"];

pub fn resolve_color(color: Option<&str>, remaining: u64, duration: u64) -> Option<&str> {
    if color == Some(COLOR_AUTO) { Some(LEVEL_COLORS[shade(remaining, duration) as usize]) } else { color }
}

pub const SHADE_THRESHOLDS: [u64; 4] = [75, 50, 25, 10];

pub fn shade(remaining: u64, duration: u64) -> u8 {
    if duration == 0 { return 4; }
    SHADE_THRESHOLDS.iter().filter(|&&th| remaining * 100 <= duration * th).count() as u8
}

fn default_segments() -> u8 { SEGMENTS_DEFAULT }

pub fn segments_ok(n: u8) -> bool {
    (SEGMENTS_MIN..=SEGMENTS_MAX).contains(&n)
}
pub const MIN_SECS: u64 = 60;
pub const YEAR_SECS: u64 = 31_557_600; // 365.25日
pub const MAX_SECS: u64 = 100 * YEAR_SECS;
const DAY: u64 = 86_400;

/// タイマーの色。キーを保存し、画面側にも同じキーの色がある。
pub const COLORS: [(&str, [u8; 3]); 8] = [
    ("red", [0xE5, 0x48, 0x4D]),
    ("orange", [0xF7, 0x6B, 0x15]),
    ("yellow", [0xE2, 0xA3, 0x36]),
    ("green", [0x30, 0xA4, 0x6C]),
    ("teal", [0x12, 0xA5, 0x94]),
    ("blue", [0x00, 0x90, 0xFF]),
    ("purple", [0x8E, 0x4E, 0xC6]),
    ("pink", [0xD6, 0x40, 0x9F]),
];
/// 色付きのメニューバーで、枠と色なしのメーターに使う灰色（ライトでもダークでも見える）
pub const NEUTRAL: [u8; 3] = [0x8E, 0x8E, 0x93];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(tag = "kind", content = "emoji", rename_all = "lowercase")]
pub enum DoneGimmick { #[default] Off, Boom, Party, Fire, Flower, Custom(String) }

pub const BURN_STEP_SECS: u64 = 5;

pub fn flower_for(color: Option<&str>) -> &'static str {
    match color {
        Some("red") => "🌹", Some("orange") => "🌺", Some("yellow") => "🌻",
        Some("green") => "🍀", Some("teal") => "🌼", Some("purple") => "🌷",
        Some("pink") | Some(COLOR_AUTO) => "🌸", _ => "💐",
    }
}

pub fn done_cells(g: &DoneGimmick, n: u64, color: Option<&str>, burned: u64) -> Vec<String> {
    if *g == DoneGimmick::Off { return Vec::new(); }
    (0..n).map(|i| match g {
        DoneGimmick::Off => "",
        DoneGimmick::Boom => "💥",
        DoneGimmick::Party => if i % 2 == 0 { "🎉" } else { "🎊" },
        DoneGimmick::Fire => if i < n.saturating_sub(burned) { "🔥" } else { "⬛" },
        DoneGimmick::Flower => flower_for(color),
        DoneGimmick::Custom(s) => s.as_str(),
    }.to_string()).collect()
}

pub fn finished_text(g: &DoneGimmick, segments: u8, color: Option<&str>, burned: u64) -> Option<String> {
    if *g == DoneGimmick::Off { None } else { Some(done_cells(g, notice_cells(segments), color, burned).concat()) }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Notify {
    #[default]
    #[serde(rename = "standard")]
    Standard,
    #[serde(rename = "25")]
    Every25,
    #[serde(rename = "10")]
    Every10,
}

impl Notify {
    pub fn thresholds(self) -> &'static [u64] {
        match self {
            Notify::Standard => &[75, 50, 25, 10],
            Notify::Every25 => &[75, 50, 25],
            Notify::Every10 => &[90, 80, 70, 60, 50, 40, 30, 20, 10],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    Min,
    Hour,
    Day,
    Year,
}

/// 毎日の時間帯。0時からの分（0〜1439）。終了が開始より前なら翌日まで。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Daily {
    pub start_min: u16,
    pub end_min: u16,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TimerDef {
    pub id: String,
    pub name: String,
    pub duration_secs: u64,
    pub unit: Unit,
    #[serde(default)]
    pub notify: Notify,
    /// メニューバーに表示するか。標準はしない
    #[serde(default)]
    pub show_in_bar: bool,
    /// 開始日（現地の0時、UNIX秒）
    #[serde(default)]
    pub start_at: Option<u64>,
    #[serde(default)]
    pub daily: Option<Daily>,
    /// COLORS のキー。None は既定の色
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default = "default_segments")]
    pub segments: u8,
    #[serde(default)]
    pub pin_float: bool,
}

impl TimerDef {
    /// 期間タイマー（開始日か毎日を持つもの）。名前を持つのはこれだけ。
    pub fn is_period(&self) -> bool {
        self.start_at.is_some() || self.daily.is_some()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Running {
    pub def_id: String,
    /// v0.2 との互換のために残す（新しく作る Running では 0）
    #[serde(default)]
    pub slot: u32,
    pub name: String,
    pub duration_secs: u64,
    pub notify: Notify,
    pub started_at: u64,
    pub next: usize,
    /// メニューバーに表示するか。標準はしない
    #[serde(default)]
    pub show_in_bar: bool,
    #[serde(default)]
    pub daily: bool,
    /// 開始時の色のコピー
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default = "default_segments")]
    pub segments: u8,
    /// 期間タイマーから始めた
    #[serde(default)]
    pub period: bool,
    /// 終了日時を指定して始めた1回きり
    #[serde(default)]
    pub once: bool,
}

impl Running {
    pub fn from_def(d: &TimerDef, started_at: u64, duration_secs: u64) -> Running {
        let period = d.is_period();
        Running {
            def_id: d.id.clone(),
            slot: 0,
            // 名前を持つのは期間タイマーだけ。作業タイマーの呼び方は付箋（label）で決まる
            name: if period { d.name.clone() } else { String::new() },
            duration_secs,
            notify: d.notify,
            started_at,
            next: 0,
            show_in_bar: d.show_in_bar,
            daily: d.daily.is_some(),
            color: d.color.clone(),
            segments: d.segments,
            period,
            once: false,
        }
    }
}

/// 終了済み行はメモリ上だけで保持する（保存形式には含めない）。
#[derive(Clone, Debug, PartialEq)]
pub struct FinishedRow {
    pub def_id: String,
    pub label: String,
    pub length: String,
    pub color: Option<String>,
    pub segments: u8,
    pub period: bool,
    pub once: bool,
    pub end_at: u64,
    pub finished_at: u64,
}

pub fn push_finished(list: &mut Vec<FinishedRow>, row: FinishedRow) {
    list.retain(|f| f.def_id != row.def_id);
    list.push(row);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Threshold(u64),
    Finished,
}

pub fn remaining_secs(r: &Running, now: u64) -> u64 {
    r.duration_secs.saturating_sub(now.saturating_sub(r.started_at))
}

pub fn lit_blocks(remaining: u64, duration: u64, segments: u64) -> u64 {
    if duration == 0 {
        0
    } else {
        (remaining * segments).div_ceil(duration)
    }
}

/// 残りの割合（%）。通知の区切りと揃うよう切り上げ。
pub fn percent(remaining: u64, duration: u64) -> u64 {
    if duration == 0 { 0 } else { (remaining as u128 * 100).div_ceil(duration as u128) as u64 }
}

pub fn bar(lit: u64) -> String {
    let lit = lit.min(BLOCKS) as usize;
    "▮".repeat(lit) + &"▯".repeat(BLOCKS as usize - lit)
}

/// 通知用の点灯マス。アプリのメーターと同じ色のキーから、いちばん近い色の四角を選ぶ。
/// 四角の絵文字は8色に足りないので、ティールは緑、ピンクは紫で代用する。色なし（既定）はアプリの既定色（青）。
pub fn color_square(key: Option<&str>) -> &'static str {
    match key {
        Some("red") => "🟥",
        Some("orange") => "🟧",
        Some("yellow") => "🟨",
        Some("green") | Some("teal") => "🟩",
        Some("purple") | Some("pink") => "🟪",
        _ => "🟦",
    }
}

/// 通知に並べるマスの上限。macOS の通知は横に14マスほどで折り返し、⬜ の並びは1語として
/// まとめて次の行へ送られて崩れるので、余裕をみて1行に収まる数にする。
pub const NOTICE_CELLS_MAX: u64 = 12;

/// 通知のマス数。目盛り数が上限以下ならそのまま、多ければ上限まで減らす。
pub fn notice_cells(segments: u8) -> u64 {
    (segments as u64).min(NOTICE_CELLS_MAX)
}

/// 通知用のメーター。アプリの画面と同じく、目盛り数（segments）個のうち lit 個を色つきで点灯する。
pub fn meter_text(lit: u64, segments: u64, color: Option<&str>) -> String {
    let n = segments.clamp(SEGMENTS_MIN as u64, SEGMENTS_MAX as u64);
    let lit = lit.min(n);
    color_square(color).repeat(lit as usize) + &"⬜".repeat((n - lit) as usize)
}

fn crossed(r: &Running, rem: u64) -> bool {
    let ths = r.notify.thresholds();
    r.next < ths.len() && rem as u128 * 100 <= r.duration_secs as u128 * ths[r.next] as u128
}

/// 区切りを複数またいでも返すのは最後の1件だけ。
pub fn tick(r: &mut Running, now: u64) -> Option<Event> {
    let ths = r.notify.thresholds();
    let rem = remaining_secs(r, now);
    if rem == 0 {
        r.next = ths.len();
        return Some(Event::Finished);
    }
    let mut hit = None;
    while crossed(r, rem) {
        hit = Some(ths[r.next]);
        r.next += 1;
    }
    hit.map(Event::Threshold)
}

/// 開始時点ですでに過ぎた区切りを、通知せずに進める。
pub fn skip_passed(r: &mut Running, now: u64) {
    let rem = remaining_secs(r, now);
    while crossed(r, rem) {
        r.next += 1;
    }
}

/// 保存したタイマーの見た目（目盛り数・色）の編集を、動いているタイマーにも反映する。
/// 動いているタイマーは開始時の値のコピーを持つので、編集しただけでは古いまま残る。
pub fn sync_look(running: &mut [Running], defs: &[TimerDef]) {
    for r in running.iter_mut() {
        if let Some(d) = defs.iter().find(|d| d.id == r.def_id) {
            r.segments = d.segments;
            r.color = d.color.clone();
        }
    }
}

/// 通知やメニューでの呼び方。期間タイマーは名前、作業タイマーは付箋の文言（空なら長さ）。
pub fn label(r: &Running, focus: &str) -> String {
    if r.period {
        return r.name.clone();
    }
    let f = focus.trim();
    if f.is_empty() { fmt_duration(r.duration_secs) } else { f.to_string() }
}

/// 色のキー → RGB。None や知らないキーは None（既定の色）。
pub fn color_rgb(key: Option<&str>) -> Option<[u8; 3]> {
    let k = key?;
    COLORS.iter().find(|(n, _)| *n == k).map(|(_, c)| *c)
}

pub fn daily_len(d: Daily) -> u64 {
    let m = (d.end_min as u64 + 1440 - d.start_min as u64) % 1440;
    (if m == 0 { 1440 } else { m }) * 60
}

/// 今が時間帯の中なら、その時間帯の開始時刻を返す。
pub fn active_window(d: Daily, today0: u64, now: u64) -> Option<u64> {
    let mut c = today0 + d.start_min as u64 * 60;
    if now < c {
        c -= DAY;
    }
    (now < c + daily_len(d)).then_some(c)
}

/// 前日から days 日先までの時間帯の開始のうち、まだ終わっていないもの（Android の予約用）。
#[cfg_attr(not(mobile), allow(dead_code))]
pub fn upcoming_windows(d: Daily, today0: u64, now: u64, days: u64) -> Vec<u64> {
    (0..=days + 1)
        .map(|k| today0 + k * DAY + d.start_min as u64 * 60 - DAY)
        .filter(|c| c + daily_len(d) > now)
        .collect()
}

pub fn fmt_remaining(secs: u64) -> String {
    if secs >= YEAR_SECS {
        let (y, d) = (secs / YEAR_SECS, secs % YEAR_SECS / DAY);
        if d == 0 { format!("{y}年") } else { format!("{y}年{d}日") }
    } else if secs >= DAY {
        let (d, h) = (secs / DAY, secs % DAY / 3600);
        if h == 0 { format!("{d}日") } else { format!("{d}日{h}時間") }
    } else if secs >= 3600 {
        let (h, m) = (secs / 3600, secs % 3600 / 60);
        if m == 0 { format!("{h}時間") } else { format!("{h}時間{m}分") }
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

pub fn fmt_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}秒")
    } else if secs % YEAR_SECS == 0 {
        format!("{}年", secs / YEAR_SECS)
    } else if secs % DAY == 0 {
        format!("{}日", secs / DAY)
    } else if secs % 3600 == 0 {
        format!("{}時間", secs / 3600)
    } else {
        format!("{}分", secs / 60)
    }
}

/// label は `label(r, focus)` の結果。
/// タイトルに呼び方、本文の1行目に残り時間、2行目にメーター。
pub fn notification_text(r: &Running, label: &str, e: Event, g: &DoneGimmick) -> (String, String) {
    let seg = notice_cells(r.segments);
    let (status, meter) = match e {
        Event::Threshold(p) => {
            let rem = r.duration_secs * p / 100;
            let color = resolve_color(r.color.as_deref(), rem, r.duration_secs);
            (format!("残り{}（{}%）", fmt_remaining(rem), p), meter_text(lit_blocks(rem, r.duration_secs, seg), seg, color))
        }
        Event::Finished => ("時間です".to_string(), finished_text(g, r.segments, r.color.as_deref(), 0)
            .unwrap_or_else(|| meter_text(0, seg, r.color.as_deref()))),
    };
    (label.to_string(), format!("{}\n{}", status, meter))
}

/// Android の予約用: (UNIX秒, タイトル, 本文)。next 以降の区切りと終了。通知IDは呼ぶ側が振る。
#[cfg_attr(not(mobile), allow(dead_code))]
pub fn schedule(r: &Running, focus: &str, g: &DoneGimmick) -> Vec<(u64, String, String)> {
    let ths = r.notify.thresholds();
    let name = label(r, focus);
    let mut out = Vec::new();
    for &p in ths.iter().skip(r.next) {
        let at = r.started_at + r.duration_secs - r.duration_secs * p / 100;
        let (t, b) = notification_text(r, &name, Event::Threshold(p), g);
        out.push((at, t, b));
    }
    let (t, b) = notification_text(r, &name, Event::Finished, g);
    out.push((r.started_at + r.duration_secs, t, b));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(name: &str, dur: u64, notify: Notify) -> Running {
        Running {
            def_id: "x".into(),
            slot: 0,
            name: name.into(),
            duration_secs: dur,
            notify,
            started_at: 0,
            next: 0,
            show_in_bar: true,
            daily: false,
            color: None,
            segments: SEGMENTS_DEFAULT,
            period: false,
            once: false,
        }
    }

    const D66: Daily = Daily { start_min: 360, end_min: 1380 }; // 6:00–23:00
    const D226: Daily = Daily { start_min: 1320, end_min: 360 }; // 22:00–6:00
    const T0: u64 = 1_000 * DAY; // ある日の0時
    fn at(h: u64, m: u64) -> u64 {
        T0 + h * 3600 + m * 60
    }

    #[test]
    fn blocks() {
        assert_eq!(lit_blocks(1800, 1800, 20), 20);
        assert_eq!(lit_blocks(1350, 1800, 20), 15);
        assert_eq!(lit_blocks(1, 1800, 20), 1);
        assert_eq!(lit_blocks(0, 1800, 20), 0);
        assert_eq!(lit_blocks(1800, 1800, 60), 60);
        assert_eq!(lit_blocks(1, 1800, 60), 1);
        assert_eq!(lit_blocks(0, 1800, 60), 0);
        assert_eq!(lit_blocks(900, 1800, 7), 4);
        assert_eq!(lit_blocks(1, 0, 7), 0);
        assert_eq!(bar(15).chars().filter(|c| *c == '▮').count(), 15);
        assert_eq!(bar(15).chars().count(), 20);
        assert_eq!(percent(1350, 1800), 75);
        assert_eq!(percent(1349, 1800), 75);
        assert_eq!(percent(0, 1800), 0);
    }

    #[test]
    fn shade_boundaries() {
        for (rem, expected) in [(1800, 0), (1351, 0), (1350, 1), (900, 2), (450, 3), (180, 4), (0, 4)] {
            assert_eq!(shade(rem, 1800), expected);
        }
        assert_eq!(shade(0, 0), 4);
    }

    #[test]
    fn finished_rows_replace_same_timer() {
        let row = |id: &str, at| FinishedRow {
            def_id: id.into(), label: "作業".into(), length: "30分".into(), color: None,
            segments: 20, period: false, once: false, end_at: at, finished_at: at,
        };
        let mut list = Vec::new();
        push_finished(&mut list, row("a", 1));
        push_finished(&mut list, row("b", 2));
        push_finished(&mut list, row("a", 3));
        assert_eq!(list, vec![row("b", 2), row("a", 3)]);
    }

    #[test]
    fn completion_cells_and_serde() {
        use DoneGimmick::*;
        assert!(done_cells(&Off, 4, None, 0).is_empty());
        assert_eq!(done_cells(&Boom, 3, None, 0).concat(), "💥💥💥");
        assert_eq!(done_cells(&Party, 4, None, 0).concat(), "🎉🎊🎉🎊");
        assert_eq!(done_cells(&Fire, 5, None, 2).concat(), "🔥🔥🔥⬛⬛");
        assert_eq!(done_cells(&Fire, 5, None, 9).concat(), "⬛".repeat(5));
        for (color, flower) in [(Some("red"), "🌹"), (None, "💐"), (Some("auto"), "🌸"), (Some("zzz"), "💐")] {
            assert_eq!(done_cells(&Flower, 1, color, 0), vec![flower]);
        }
        assert_eq!(done_cells(&Custom("🌟".into()), 2, None, 0).concat(), "🌟🌟");
        for g in [Off, Boom, Party, Fire, Flower, Custom("🌟".into())] {
            assert!(done_cells(&g, 0, None, 0).is_empty());
            assert_eq!(serde_json::from_str::<DoneGimmick>(&serde_json::to_string(&g).unwrap()).unwrap(), g);
        }
        assert_eq!(serde_json::to_value(Custom("🌟".into())).unwrap(), serde_json::json!({"kind":"custom","emoji":"🌟"}));
        assert_eq!(serde_json::to_value(Off).unwrap(), serde_json::json!({"kind":"off"}));
        assert_eq!(finished_text(&Off, 20, None, 0), None);
        assert_eq!(finished_text(&Boom, 20, None, 0), Some("💥".repeat(12)));
        assert_eq!(finished_text(&Boom, 7, None, 0), Some("💥".repeat(7)));
        let r = run("", 1800, Notify::Standard);
        assert_eq!(notification_text(&r, "x", Event::Finished, &Boom).1, "時間です\n".to_string() + &"💥".repeat(12));
        assert_eq!(schedule(&r, "x", &Boom).last().unwrap().2, notification_text(&r, "x", Event::Finished, &Boom).1);
    }

    #[test]
    fn auto_color_boundaries() {
        for (rem, color) in [(1800, "blue"), (1351, "blue"), (1350, "green"), (900, "yellow"), (450, "orange"), (180, "red"), (0, "red")] {
            assert_eq!(resolve_color(Some(COLOR_AUTO), rem, 1800), Some(color));
            assert_eq!(resolve_color(Some("red"), rem, 1800), Some("red"));
            assert_eq!(resolve_color(None, rem, 1800), None);
        }
        assert_eq!(resolve_color(Some(COLOR_AUTO), 0, 0), Some("red"));
        let mut r = run("", 1800, Notify::Standard);
        r.color = Some(COLOR_AUTO.into());
        assert_eq!(meter_line(notification_text(&r, "x", Event::Threshold(50), &DoneGimmick::Off)), "🟨".repeat(6) + &"⬜".repeat(6));
    }

    #[test]
    fn running_copies_segments() {
        let mut d: TimerDef = serde_json::from_str(
            r#"{"id":"a","name":"","duration_secs":1800,"unit":"min","segments":7}"#
        ).unwrap();
        let r = Running::from_def(&d, 0, 1800);
        d.segments = 60;
        assert_eq!(r.segments, 7);
        assert_eq!(Running::from_def(&d, 0, 1800).segments, 60);
    }

    #[test]
    fn tick_standard() {
        let mut r = run("t", 1800, Notify::Standard);
        assert_eq!(tick(&mut r, 449), None);
        assert_eq!(tick(&mut r, 450), Some(Event::Threshold(75)));
        assert_eq!(tick(&mut r, 449), None);
    }

    #[test]
    fn tick_multi_cross_and_finish() {
        let mut r = run("t", 1800, Notify::Standard);
        assert_eq!(tick(&mut r, 1700), Some(Event::Threshold(10)));
        assert_eq!(r.next, 4);
        assert_eq!(tick(&mut r, 1750), None);
        assert_eq!(tick(&mut r, 1800), Some(Event::Finished));
    }

    #[test]
    fn skip_passed_silent() {
        let mut r = run("t", 1800, Notify::Standard);
        skip_passed(&mut r, 1000);
        assert_eq!(r.next, 2);
        assert_eq!(tick(&mut r, 1000), None);
    }

    #[test]
    fn labels() {
        // 期間タイマーは付箋に関係なく名前
        let mut p = run("人生", 1800, Notify::Standard);
        p.period = true;
        assert_eq!(label(&p, "書類作成"), "人生");
        // 作業タイマーは付箋の文言（前後の空白は除く）。空なら長さ
        let w = run("", 1800, Notify::Standard);
        assert_eq!(label(&w, " 書類作成 "), "書類作成");
        assert_eq!(label(&w, ""), "30分");
        assert_eq!(label(&w, "  "), "30分");
    }

    #[test]
    fn colors() {
        assert_eq!(color_rgb(Some("blue")), Some([0x00, 0x90, 0xFF]));
        assert_eq!(color_rgb(None), None);
        assert_eq!(color_rgb(Some("black")), None);
        assert_eq!(COLORS.len(), 8);
    }

    #[test]
    fn daily_windows() {
        assert_eq!(daily_len(D66), 17 * 3600);
        assert_eq!(daily_len(D226), 8 * 3600);
        assert_eq!(active_window(D66, T0, at(5, 0)), None);
        assert_eq!(active_window(D66, T0, at(7, 0)), Some(at(6, 0)));
        assert_eq!(active_window(D66, T0, at(23, 30)), None);
        assert_eq!(active_window(D226, T0, at(1, 0)), Some(at(22, 0) - DAY));
        assert_eq!(active_window(D226, T0, at(23, 0)), Some(at(22, 0)));
        assert_eq!(active_window(D226, T0, at(7, 0)), None);
        let w = upcoming_windows(D66, T0, at(7, 0), 7);
        assert_eq!(w.len(), 8);
        assert_eq!(w[0], at(6, 0));
        assert_eq!(upcoming_windows(D66, T0, at(23, 30), 7).len(), 7);
    }

    #[test]
    fn formats() {
        assert_eq!(fmt_remaining(1_944_000), "22日12時間");
        assert_eq!(fmt_remaining(1_296_000), "15日");
        assert_eq!(fmt_remaining(23_040), "6時間24分");
        assert_eq!(fmt_remaining(450), "7:30");
        assert_eq!(fmt_remaining(YEAR_SECS * 49 + DAY * 318), "49年318日");
        assert_eq!(fmt_duration(80 * YEAR_SECS), "80年");
        assert_eq!(fmt_duration(2_592_000), "30日");
        assert_eq!(fmt_duration(28_800), "8時間");
        assert_eq!(fmt_duration(1800), "30分");
        assert_eq!(fmt_duration(30), "30秒");
    }

    #[test]
    fn notification() {
        let r = run("", 2_592_000, Notify::Standard);
        let (t, b) = notification_text(&r, "A開発", Event::Threshold(75), &DoneGimmick::Off);
        assert_eq!(t, "A開発");
        // 既定の20目盛りは12マスに減らす（75% → 9マス）
        assert_eq!(b, "残り22日12時間（75%）\n".to_string() + &"🟦".repeat(9) + &"⬜".repeat(3));
        let (t, b) = notification_text(&r, "A開発", Event::Finished, &DoneGimmick::Off);
        assert_eq!(t, "A開発");
        assert_eq!(b, "時間です\n".to_string() + &"⬜".repeat(12));
    }

    /// 通知の本文のうち、メーターの行（2行目）だけを取り出す。
    fn meter_line((_, body): (String, String)) -> String {
        body.lines().last().unwrap().to_string()
    }

    #[test]
    fn notification_meter_follows_app() {
        // 目盛り数と色はタイマーの設定に従う。点灯数はアプリ画面と同じ切り上げ（10目盛りの75% → 8）
        let mut r = run("", 1800, Notify::Standard);
        r.segments = 10;
        r.color = Some("red".into());
        let b = meter_line(notification_text(&r, "x", Event::Threshold(75), &DoneGimmick::Off));
        assert_eq!(b, "🟥".repeat(8) + &"⬜".repeat(2));
        let b = meter_line(notification_text(&r, "x", Event::Threshold(10), &DoneGimmick::Off));
        assert_eq!(b, "🟥".repeat(1) + &"⬜".repeat(9));
        let b = meter_line(notification_text(&r, "x", Event::Finished, &DoneGimmick::Off));
        assert_eq!(b, "⬜".repeat(10));
        // 上限以下の目盛り数はそのまま、多ければ12マスに収める
        r.segments = 12;
        assert_eq!(meter_line(notification_text(&r, "x", Event::Finished, &DoneGimmick::Off)).chars().count(), 12);
        r.segments = 60;
        let b = meter_line(notification_text(&r, "x", Event::Threshold(50), &DoneGimmick::Off));
        assert_eq!(b.chars().count(), 12);
        assert_eq!(b.chars().filter(|c| *c == '🟥').count(), 6);
    }

    #[test]
    fn sync_look_updates_running() {
        let def = |id: &str, segments: u8, color: Option<&str>| TimerDef {
            id: id.into(),
            name: String::new(),
            duration_secs: 1800,
            unit: Unit::Min,
            notify: Notify::Standard,
            show_in_bar: true,
            start_at: None,
            daily: None,
            color: color.map(String::from),
            segments,
            pin_float: false,
        };
        let mut a = run("", 1800, Notify::Standard);
        a.def_id = "a".into();
        let mut once = run("", 1800, Notify::Standard);
        once.def_id = "once-1".into(); // 保存したタイマーに無いもの（1回きり）はそのまま
        once.segments = 7;
        let mut rs = vec![a, once];
        sync_look(&mut rs, &[def("a", 40, Some("pink")), def("b", 5, None)]);
        assert_eq!((rs[0].segments, rs[0].color.as_deref()), (40, Some("pink")));
        assert_eq!((rs[1].segments, rs[1].color.as_deref()), (7, None));
    }

    #[test]
    fn meter_text_bounds_and_colors() {
        assert_eq!(meter_text(99, 5, None), "🟦".repeat(5));
        assert_eq!(meter_text(0, 1, None).chars().count(), 2); // 目盛り数は2〜60に収める
        assert_eq!(meter_text(0, 500, None).chars().count(), 60);
        // 色のキーは全部、四角に割り当てられている（既定の青と同じになるのは blue だけ）
        for (key, _) in COLORS {
            assert!(color_square(Some(key)) != "🟦" || key == "blue", "{key}");
        }
        assert_eq!(color_square(None), "🟦");
        assert_eq!(color_square(Some("unknown")), "🟦");
    }

    #[test]
    fn schedule_times() {
        let s = schedule(&run("", 1800, Notify::Standard), "書類作成", &DoneGimmick::Off);
        assert_eq!(s.len(), 5);
        assert_eq!(s[0].0, 450);
        assert_eq!(s[0].1, "書類作成");
        assert!(s[0].2.starts_with("残り"));
        assert_eq!(s.last().unwrap().0, 1800);
        assert_eq!(s.last().unwrap().1, "書類作成");
        assert!(s.last().unwrap().2.starts_with("時間です\n"));
        // 付箋が空なら長さ
        assert_eq!(schedule(&run("", 1800, Notify::Standard), "", &DoneGimmick::Off).last().unwrap().1, "30分");
        assert_eq!(schedule(&run("t", 1800, Notify::Every10), "", &DoneGimmick::Off).len(), 10);
        let mut r2 = run("t", 1800, Notify::Standard);
        r2.next = 2;
        assert_eq!(schedule(&r2, "", &DoneGimmick::Off).len(), 3);
    }

    #[test]
    fn serde_compat() {
        let d: TimerDef = serde_json::from_str(
            r#"{"id":"a","name":"n","duration_secs":60,"unit":"min","notify":"25"}"#,
        )
        .unwrap();
        assert_eq!(d.notify, Notify::Every25);
        assert_eq!(d.segments, SEGMENTS_DEFAULT);
        assert!(!d.show_in_bar);
        assert!(!d.pin_float);
        assert_eq!((d.start_at, d.daily, d.color.clone()), (None, None, None));
        assert!(!d.is_period());
        let d: TimerDef =
            serde_json::from_str(r#"{"id":"a","name":"n","duration_secs":60,"unit":"year"}"#).unwrap();
        assert_eq!(d.notify, Notify::Standard);
        assert_eq!(d.unit, Unit::Year);
        let r: Running = serde_json::from_str(
            r#"{"def_id":"a","slot":1,"name":"n","duration_secs":60,"notify":"standard","started_at":5,"next":0}"#,
        )
        .unwrap();
        assert!(!r.show_in_bar && !r.daily);
        assert_eq!(r.segments, SEGMENTS_DEFAULT);
        assert_eq!((r.color.clone(), r.period, r.once), (None, false, false));
        // v0.3 までの形（project・counted_from を含む）も読める
        let r: Running = serde_json::from_str(
            r#"{"def_id":"a","slot":0,"name":"n","duration_secs":60,"notify":"standard","started_at":5,"next":0,"counted_from":9,"project":"P","show_in_bar":true,"daily":false}"#,
        )
        .unwrap();
        assert!(r.show_in_bar);
        assert_eq!((r.color, r.period, r.once), (None, false, false));
    }
}
