//! Mac のメニューバー。lib.rs 側の #[cfg(desktop)] で desktop のときだけ読み込む。
use crate::timer::{self, Running};
use crate::AppState;
use std::sync::Mutex;
use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::image::Image;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, Wry};

const TRAY_ID: &str = "main";

#[derive(Default)]
struct TrayInner {
    running_items: Vec<(String, Submenu<Wry>)>,
    last_rows: Option<Vec<Row>>,
}

/// メーターの1行：(塗り幅px, 色)。色が None なら既定の色。
type Row = (u32, Option<[u8; 3]>);

struct TrayState(Mutex<TrayInner>);

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    app.manage(TrayState(Mutex::new(TrayInner::default())));
    let menu = make_menu(app)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(idle_icon())
        .icon_as_template(false)
        .tooltip("In This Moment")
        .menu(&menu)
        .on_menu_event(on_menu)
        .build(app)?;
    Ok(())
}

/// 通知と同じ「残り時間（%）」の形で出す。% は通知の区切りと揃うよう切り上げ。
fn label(r: &Running, focus: &str, now: u64) -> String {
    let rem = timer::remaining_secs(r, now);
    let pct = timer::percent(rem, r.duration_secs);
    format!(
        "{}　{}　残り{}（{}%）",
        timer::label(r, focus),
        timer::bar(timer::lit_blocks(rem, r.duration_secs, timer::BLOCKS)),
        timer::fmt_remaining(rem),
        pct
    )
}

fn make_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    // ロックは clone の間だけ。メニュー作成はロックの外で行う。
    let data = app.state::<AppState>().data.lock().unwrap().clone();
    let t = crate::now();
    let menu = Menu::new(app)?;
    let mut items = Vec::new();
    for r in &data.running {
        let stop = MenuItem::with_id(app, format!("stop:{}", r.def_id), "停止", true, None::<&str>)?;
        let sub = Submenu::with_id_and_items(app, format!("run:{}", r.def_id), label(r, &data.focus, t), true, &[&stop])?;
        menu.append(&sub)?;
        items.push((r.def_id.clone(), sub));
    }
    if !data.running.is_empty() {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    for d in &data.timers {
        // 期間タイマー（毎日・開始日あり）はここに並べない。動いているものは上の行から停止でき、作成や編集は「タイマーを編集…」で行う
        if d.is_period() {
            continue;
        }
        // 作業タイマーは名前を持たないので長さだけ。実行中は重複して始められないので無効
        let running = data.running.iter().any(|r| r.def_id == d.id);
        menu.append(&MenuItem::with_id(app, format!("start:{}", d.id), timer::fmt_duration(d.duration_secs), !running, None::<&str>)?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&CheckMenuItem::with_id(app, "float", "フローティング表示", true, data.float, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "edit", "タイマーを編集…", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?)?;
    app.state::<TrayState>().0.lock().unwrap().running_items = items;
    Ok(menu)
}

pub fn rebuild(app: &AppHandle) {
    if let (Ok(menu), Some(tray)) = (make_menu(app), app.tray_by_id(TRAY_ID)) {
        let _ = tray.set_menu(Some(menu));
    }
}

/// メニューバーとフローティングに出す実行中（show_in_bar のもの、残りが少ない順に最大6本）。
fn shown(running: &[Running], now: u64) -> Vec<&Running> {
    let mut v: Vec<&Running> = running.iter().filter(|r| r.show_in_bar).collect();
    v.sort_by_key(|r| timer::remaining_secs(r, now));
    v.truncate(MAX_ROWS);
    v
}

pub fn update(app: &AppHandle, running: &[Running], focus: &str, now: u64) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let rows = shown(running, now);
    let fills: Vec<Row> = rows
        .iter()
        .map(|r| (fill_px(timer::remaining_secs(r, now), r.duration_secs), timer::color_rgb(timer::resolve_color(r.color.as_deref(), timer::remaining_secs(r, now), r.duration_secs))))
        .collect();
    let tooltip = if running.is_empty() {
        "In This Moment".to_string()
    } else {
        running.iter().map(|r| label(r, focus, now)).collect::<Vec<_>>().join("\n")
    };
    let (changed, items) = {
        let ts = app.state::<TrayState>();
        let mut inner = ts.0.lock().unwrap();
        let changed = inner.last_rows.as_ref() != Some(&fills);
        inner.last_rows = Some(fills.clone());
        (changed, inner.running_items.clone())
    };
    if changed {
        // メーターを出すものがなければアプリのアイコン（カラー）。
        // あればメーター：色が1つもなければテンプレート画像（OSが明暗に合わせる）、色があればカラー画像。
        if fills.is_empty() {
            let _ = tray.set_icon(Some(idle_icon()));
            let _ = tray.set_icon_as_template(false);
        } else {
            let (img, colored) = meter_icon(&fills);
            let _ = tray.set_icon(Some(img));
            let _ = tray.set_icon_as_template(!colored);
        }
    }
    let _ = tray.set_tooltip(Some(tooltip));
    for (id, sub) in items {
        if let Some(r) = running.iter().find(|r| r.def_id == id) {
            let _ = sub.set_text(label(r, focus, now));
        }
    }
}

// ---- メーターのアイコン（横棒＋10分割の縦線）。テンプレート画像なので黒＋透明で描く ----
const W: u32 = 88; // 2倍解像度。メニューバーでは 44x18pt 程度
const H: u32 = 36;
const X0: u32 = 2; // 外枠
const X1: u32 = 86;
const B: u32 = 2; // 枠の太さ・行の隙間
const IX0: u32 = X0 + B; // 内側
const IX1: u32 = X1 - B;
const INNER: u32 = IX1 - IX0; // 80px
const MAX_ROWS: usize = 6;

/// 10分割の目盛り単位で塗る。残りが1区切り分減るごとに1マス消える（残りが少しでもあれば最低1マス）。
fn fill_px(remaining: u64, duration: u64) -> u32 {
    if duration == 0 {
        return 0;
    }
    let segs = ((remaining as u128 * 10).div_ceil(duration as u128) as u32).min(10);
    INNER * segs / 10
}

/// rows: 各行の（塗り幅px, 色）。空なら空のメーター1本。
/// 1本なら縦 8〜28、2〜6本なら外枠 2〜34 の中に2pxの隙間をあけて並べる。
/// 戻り値の bool は「色付きか」。色が1つもなければ黒＋透明のテンプレート画像、
/// 1つでもあれば枠と既定色の行を灰色（NEUTRAL）にしたカラー画像にする。
fn meter_rgba(rows: &[Row]) -> (Vec<u8>, bool) {
    let colored = rows.iter().any(|(_, c)| c.is_some());
    let n = rows.len().clamp(1, MAX_ROWS) as u32;
    let (y0, y1) = if n == 1 { (8, 28) } else { (2, 34) };
    let inner_h = y1 - y0 - 2 * B;
    let row_h = if n == 1 { inner_h } else { (inner_h - B * (n - 1)) / n };
    let mut px = vec![0u8; (W * H * 4) as usize];
    let mut set = |x: u32, y: u32, rgb: [u8; 3], a: u8| {
        let i = ((y * W + x) * 4) as usize;
        if colored {
            px[i..i + 3].copy_from_slice(&rgb);
        } // テンプレート画像の RGB は 0（黒）のまま
        px[i + 3] = a;
    };
    for y in y0..y1 {
        for x in X0..X1 {
            let border = x < IX0 || x >= IX1 || y < y0 + B || y >= y1 - B;
            if border {
                set(x, y, timer::NEUTRAL, 255);
                continue;
            }
            let iy = y - y0 - B;
            let row = iy / (row_h + B);
            if row >= n || iy % (row_h + B) >= row_h {
                continue; // 行の間の隙間
            }
            let (fill, color) = rows.get(row as usize).copied().unwrap_or((0, None));
            let rgb = color.unwrap_or(timer::NEUTRAL);
            let ix = x - IX0;
            let divider = (1..10).any(|k| {
                let dx = INNER * k / 10;
                ix + 1 >= dx && ix <= dx // 2px の縦線
            });
            let filled = ix < fill;
            let a = match (filled, divider) {
                (true, true) => 0,    // 塗りの中の区切りは抜く
                (true, false) => 255, // 塗り
                (false, true) => 140, // 空き部分の区切りは薄く
                (false, false) => 0,
            };
            set(x, y, rgb, a);
        }
    }
    (px, colored)
}

/// 何も表示しないときのアイコン。アプリのアイコン（円相）を背景を抜いて 36x36 の RGBA にしたもの。
const IDLE_RGBA: &[u8] = include_bytes!("../icons/tray-idle.rgba");
const IDLE_SIZE: u32 = 36;

fn idle_icon() -> Image<'static> {
    Image::new(IDLE_RGBA, IDLE_SIZE, IDLE_SIZE)
}

fn meter_icon(rows: &[Row]) -> (Image<'static>, bool) {
    let (px, colored) = meter_rgba(rows);
    (Image::new_owned(px, W, H), colored)
}

fn on_menu(app: &AppHandle, e: MenuEvent) {
    let id = e.id().as_ref();
    match id {
        "quit" => app.exit(0),
        "edit" => open_editor(app),
        "float" => toggle_float(app),
        _ => {
            if let Some(def) = id.strip_prefix("start:") {
                let _ = crate::start(app, def);
            } else if let Some(def) = id.strip_prefix("stop:") {
                let _ = crate::stop(app, def);
            }
        }
    }
}

pub fn open_editor(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("editor") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    if let Ok(w) = WebviewWindowBuilder::new(app, "editor", WebviewUrl::App("index.html".into()))
        .title("In This Moment")
        .inner_size(360.0, 520.0)
        .build()
    {
        let _ = w.set_focus();
    }
}

fn toggle_float(app: &AppHandle) {
    let on = app.state::<AppState>().data.lock().unwrap().float;
    set_float(app, !on);
}

/// フローティング表示のオン・オフ。状態は保存し、メニューのチェックも揃える。
pub fn set_float(app: &AppHandle, on: bool) {
    {
        let st = app.state::<AppState>();
        let mut d = st.data.lock().unwrap();
        d.float = on;
        crate::persist(&st, &d);
    }
    if on {
        open_float(app);
    } else if let Some(w) = app.get_webview_window("float") {
        let _ = w.close();
    }
    rebuild(app);
}

/// 常に最前面の小さな窓。index.html?view=float を表示する。
pub fn open_float(app: &AppHandle) {
    if app.get_webview_window("float").is_some() {
        return;
    }
    // 大きさと位置は前回のものを使う。いったん隠して作り、位置を戻してから見せる（真ん中に一瞬出ないように）
    let (size, pos) = {
        let st = app.state::<AppState>();
        let d = st.data.lock().unwrap();
        (d.float_size.unwrap_or((220.0, 68.0)), d.float_pos)
    };
    let Ok(w) = WebviewWindowBuilder::new(app, "float", WebviewUrl::App("index.html?view=float".into()))
        .title("In This Moment")
        .inner_size(size.0.max(160.0), size.1)
        .min_inner_size(160.0, 40.0)
        .decorations(false)
        .always_on_top(true)
        .resizable(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .visible(false)
        .build()
    else {
        return;
    };
    if let Some((x, y)) = pos.filter(|p| on_screen(app, *p)) {
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
    let _ = w.show();
}

/// 前回の位置がどこかの画面の中にあるか（画面の構成が変わって見えなくなるのを防ぐ）。
fn on_screen(app: &AppHandle, (x, y): (i32, i32)) -> bool {
    let (px, py) = (x + 20, y + 20); // 左上の少し内側
    app.available_monitors().unwrap_or_default().iter().any(|m| {
        let (p, s) = (m.position(), m.size());
        px >= p.x && py >= p.y && px < p.x + s.width as i32 && py < p.y + s.height as i32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(px: &[u8], x: u32, y: u32) -> u8 {
        px[((y * W + x) * 4 + 3) as usize]
    }

    #[test]
    fn fill_width() {
        assert_eq!(fill_px(1800, 1800), INNER);
        assert_eq!(fill_px(1621, 1800), INNER); // 9割を切るまでは10マス
        assert_eq!(fill_px(1620, 1800), INNER * 9 / 10);
        assert_eq!(fill_px(900, 1800), INNER / 2);
        assert_eq!(fill_px(1, 1800), INNER / 10);
        assert_eq!(fill_px(0, 1800), 0);
    }

    #[test]
    fn meter_pixels() {
        let mid = 18;
        let (half, colored) = meter_rgba(&[(INNER / 2, None)]);
        assert!(!colored);
        assert_eq!(half.len(), (W * H * 4) as usize);
        assert_eq!(alpha(&half, X0, mid), 255); // 枠
        assert_eq!(alpha(&half, IX0 + 3, mid), 255); // 塗り
        assert_eq!(alpha(&half, IX0 + INNER / 10, mid), 0); // 塗りの中の区切り
        assert_eq!(alpha(&half, IX0 + INNER * 7 / 10, mid), 140); // 空きの区切り
        assert_eq!(alpha(&half, IX0 + 58, mid), 0); // 空き（区切り線の間）
        assert_eq!(alpha(&half, 0, 0), 0); // 外側
        assert_eq!(alpha(&half, X0, 5), 0); // 1本のときは枠が 8〜28
        assert_eq!(meter_rgba(&[]), meter_rgba(&[(0, None)]));
    }

    #[test]
    fn meter_colors() {
        let rgb = |px: &[u8], x: u32, y: u32| {
            let i = ((y * W + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2]]
        };
        let blue = timer::color_rgb(Some("blue"));
        // 色なしだけ：黒＋透明（テンプレート）
        let (px, colored) = meter_rgba(&[(INNER, None)]);
        assert!(!colored);
        assert_eq!(rgb(&px, IX0 + 3, 18), [0, 0, 0]);
        // 色ありが混じる：色の行は色、既定色の行と枠は灰色
        let (px, colored) = meter_rgba(&[(INNER, blue), (INNER, None)]);
        assert!(colored);
        assert_eq!(rgb(&px, IX0 + 3, 7), blue.unwrap());
        assert_eq!(rgb(&px, IX0 + 3, 27), timer::NEUTRAL);
        assert_eq!(rgb(&px, X0, 10), timer::NEUTRAL);
        assert_eq!(alpha(&px, IX0 + 3, 7), 255);
    }

    #[test]
    fn idle_icon_size() {
        assert_eq!(IDLE_RGBA.len(), (IDLE_SIZE * IDLE_SIZE * 4) as usize);
    }

    #[test]
    fn meter_rows() {
        let (px, _) = meter_rgba(&[(INNER, None), (0, None), (INNER / 2, None)]);
        assert_eq!(alpha(&px, X0, 2), 255); // 外枠は 2〜34
        assert_eq!(alpha(&px, IX0 + 3, 7), 255); // 1本目の塗り
        assert_eq!(alpha(&px, IX0 + 3, 12), 0); // 隙間
        assert_eq!(alpha(&px, IX0 + 3, 16), 0); // 2本目は空
        assert_eq!(alpha(&px, IX0 + 3, 24), 255); // 3本目の塗り
        assert_eq!(alpha(&px, IX0 + 3, 31), 255);
        assert_eq!(alpha(&px, IX0 + 58, 28), 0); // 3本目の塗り幅の外
        let (six, _) = meter_rgba(&[(INNER, None); 6]);
        assert_eq!(alpha(&six, IX0 + 3, 4), 255);
        assert_eq!(alpha(&six, IX0 + 3, 31), 255); // 6本目の最後の行
    }
}
