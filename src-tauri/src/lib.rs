#[cfg(target_os = "macos")]
mod notice;
mod store;
mod timer;
#[cfg(desktop)]
mod tray;

use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, RunEvent};
#[cfg(any(desktop, mobile))]
use tauri_plugin_notification::NotificationExt;
use timer::{Running, TimerDef};

pub struct AppState {
    pub data: Mutex<store::Data>,
    pub path: PathBuf,
    pub finished: Mutex<Vec<timer::FinishedRow>>,
}

#[derive(Serialize)]
struct RunningView {
    def_id: String,
    /// 呼び方：期間タイマーは名前、作業タイマーは付箋の文言（空なら長さ）
    label: String,
    /// タイマーの長さの表記（30分 など）
    length: String,
    remaining: String,
    remaining_secs: u64,
    /// 残りの割合（通知の区切りと揃うよう切り上げ）
    percent: u64,
    bar: String,
    lit: u64,
    segments: u8,
    shade: u8,
    show_in_bar: bool,
    daily: bool,
    period: bool,
    once: bool,
    color: Option<String>,
    /// 終わる時刻（UNIX秒）
    end_at: u64,
}

#[derive(Serialize)]
struct FinishedView {
    def_id: String, label: String, length: String,
    period: bool, once: bool, end_at: u64, finished_at: u64,
    segments: u8,
    cells: Option<String>,
}

fn finished_views(rows: &[timer::FinishedRow], gimmick: &timer::DoneGimmick, t: u64) -> Vec<FinishedView> {
    rows.iter().rev().map(|f| {
        let burned = (t.saturating_sub(f.finished_at) / timer::BURN_STEP_SECS).min(timer::notice_cells(f.segments));
        FinishedView {
            def_id: f.def_id.clone(), label: f.label.clone(), length: f.length.clone(),
            period: f.period, once: f.once, end_at: f.end_at, finished_at: f.finished_at,
            segments: f.segments,
            cells: timer::finished_text(gimmick, f.segments, f.color.as_deref(), burned),
        }
    }).collect()
}

#[derive(Serialize)]
struct StateView {
    timers: Vec<TimerDef>,
    running: Vec<RunningView>,
    focus: String,
    done_gimmick: timer::DoneGimmick,
    finished: Vec<FinishedView>,
}

pub(crate) fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// 現地の「今日の0時」（UNIX秒）。
fn today0(now: u64) -> u64 {
    use chrono::{Local, TimeZone};
    let Some(t) = Local.timestamp_opt(now as i64, 0).single() else { return now - now % 86_400 };
    let d = t.date_naive().and_hms_opt(0, 0, 0).unwrap();
    Local
        .from_local_datetime(&d)
        .earliest()
        .map(|x| x.timestamp() as u64)
        .unwrap_or(now - now % 86_400)
}

fn debug_scale(secs: u64) -> u64 {
    if std::env::var("BLOCK_TIMER_DEBUG").is_ok() {
        (secs / 60).max(1)
    } else {
        secs
    }
}

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

pub(crate) fn persist(st: &AppState, data: &store::Data) {
    if let Err(e) = store::save(&st.path, data) {
        eprintln!("timers.json の保存に失敗: {e}");
    }
}

/// ロックを外してから呼ぶこと。
fn after_change(app: &AppHandle) {
    #[cfg(desktop)]
    tray::rebuild(app);
    #[cfg(mobile)]
    reschedule_all(app);
}

pub(crate) fn start(app: &AppHandle, id: &str) -> Result<(), String> {
    start_with(app, id, None)
}

/// begin: 開始時刻（毎日タイマーの自動開始で使う）。None なら開始日 → 今の順。
fn start_with(app: &AppHandle, id: &str, begin: Option<u64>) -> Result<(), String> {
    let st = state(app);
    {
        let mut d = st.data.lock().unwrap();
        let def = d.timers.iter().find(|t| t.id == id).cloned().ok_or("タイマーが見つかりません")?;
        if d.running.iter().any(|r| r.def_id == id) {
            return Err("すでに実行中です".into());
        }
        let t = now();
        let started = begin.or(def.start_at).unwrap_or(t);
        // 開始時刻が過去のもの（毎日・開始日付き）は縮めると「もう終わっている」になるので縮めない
        let plain = begin.is_none() && def.start_at.is_none() && def.daily.is_none();
        let dur = if plain { debug_scale(def.duration_secs) } else { def.duration_secs };
        if started > t {
            return Err("開始時刻が未来です".into());
        }
        if started + dur <= t {
            return Err("この開始日ではもう終わっています".into());
        }
        let mut r = Running::from_def(&def, started, dur);
        timer::skip_passed(&mut r, t);
        d.running.push(r);
        persist(&st, &d);
    }
    st.finished.lock().unwrap().retain(|f| f.def_id != id);
    after_change(app);
    Ok(())
}

pub(crate) fn stop(app: &AppHandle, id: &str) -> Result<(), String> {
    let st = state(app);
    {
        let mut d = st.data.lock().unwrap();
        let pos = d.running.iter().position(|r| r.def_id == id).ok_or("実行中ではありません")?;
        let r = d.running.remove(pos);
        if r.daily {
            d.daily_skip.push((r.def_id.clone(), r.started_at));
        }
        persist(&st, &d);
    }
    after_change(app);
    Ok(())
}

#[tauri::command]
fn get_state(app: AppHandle) -> StateView {
    let t = now();
    let st = state(&app);
    let d = st.data.lock().unwrap().clone();
    let finished = st.finished.lock().unwrap().clone();
    StateView {
        finished: finished_views(&finished, &d.done_gimmick, t),
        running: d
            .running
            .iter()
            .map(|r| {
                let rem = timer::remaining_secs(r, t);
                RunningView {
                    def_id: r.def_id.clone(),
                    label: timer::label(r, &d.focus),
                    length: timer::fmt_duration(r.duration_secs),
                    remaining: timer::fmt_remaining(rem),
                    remaining_secs: rem,
                    percent: timer::percent(rem, r.duration_secs),
                    bar: timer::bar(timer::lit_blocks(rem, r.duration_secs, timer::BLOCKS)),
                    lit: timer::lit_blocks(rem, r.duration_secs, r.segments as u64),
                    segments: r.segments,
                    shade: timer::shade(rem, r.duration_secs),
                    show_in_bar: r.show_in_bar,
                    daily: r.daily,
                    period: r.period,
                    once: r.once,
                    color: timer::resolve_color(r.color.as_deref(), rem, r.duration_secs).map(String::from),
                    end_at: r.started_at + r.duration_secs,
                }
            })
            .collect(),
        timers: d.timers,
        focus: d.focus,
        done_gimmick: d.done_gimmick,
    }
}

#[tauri::command]
fn start_timer(app: AppHandle, id: String) -> Result<(), String> {
    start(&app, &id)
}

#[tauri::command]
fn stop_timer(app: AppHandle, id: String) -> Result<(), String> {
    stop(&app, &id)
}

#[tauri::command]
fn save_timers(app: AppHandle, mut timers: Vec<TimerDef>) -> Result<(), String> {
    let prev = state(&app).data.lock().unwrap().timers.clone();
    store::validate_against(&mut timers, now(), &prev)?;
    {
        let st = state(&app);
        let mut d = st.data.lock().unwrap();
        // 動いているタイマーにも目盛り数・色の編集を反映する（フローティング・通知が古いまま残らないように）
        timer::sync_look(&mut d.running, &timers);
        d.timers = timers;
        persist(&st, &d);
    }
    after_change(&app);
    Ok(())
}

#[tauri::command]
fn dismiss_finished(app: AppHandle, id: String) {
    state(&app).finished.lock().unwrap().retain(|f| f.def_id != id);
}

#[tauri::command]
fn set_done_gimmick(app: AppHandle, gimmick: timer::DoneGimmick) -> Result<(), String> {
    store::validate_gimmick(&gimmick)?;
    {
        let st = state(&app);
        let mut d = st.data.lock().unwrap();
        d.done_gimmick = gimmick;
        persist(&st, &d);
    }
    after_change(&app);
    Ok(())
}

/// 付箋を書き換える。実行中のタイマーの通知やメニューの表示も付箋に合わせて変わる。
#[tauri::command]
fn set_focus(app: AppHandle, text: String) -> String {
    let text = store::normalize_focus(&text);
    {
        let st = state(&app);
        let mut d = st.data.lock().unwrap();
        d.focus = text.clone();
        persist(&st, &d);
    }
    after_change(&app);
    text
}

/// 終わる日時を決めて、いますぐ始める1回きりの作業タイマー（プリセットには残らない）。
#[tauri::command]
fn start_until(app: AppHandle, end_at: u64) -> Result<(), String> {
    let t = now();
    if end_at <= t + timer::MIN_SECS {
        return Err("終了日時は今から1分より先にしてください".into());
    }
    if end_at > t + timer::MAX_SECS {
        return Err("終了日時は100年以内にしてください".into());
    }
    {
        let st = state(&app);
        let mut d = st.data.lock().unwrap();
        d.running.push(Running {
            def_id: format!("once-{}", store::new_id()),
            slot: 0,
            name: String::new(),
            duration_secs: debug_scale(end_at - t),
            notify: timer::Notify::Standard,
            started_at: t,
            next: 0,
            show_in_bar: true,
            daily: false,
            color: None,
            period: false,
            once: true,
            segments: timer::SEGMENTS_DEFAULT,
        });
        persist(&st, &d);
    }
    after_change(&app);
    Ok(())
}

/// 窓を開く（Mac）。to: "app"＝メイン画面を開く、"float"＝フローティングを出す。
/// どちらも、もう一方の窓は閉じない。フローティングを閉じるのはメニューの「フローティング表示」だけ。
/// Android では窓が1つなので何もしない。
#[tauri::command]
fn switch_view(app: AppHandle, to: String) {
    #[cfg(desktop)]
    match to.as_str() {
        "app" => tray::open_editor(&app),
        "float" => tray::set_float(&app, true),
        _ => {}
    }
    #[cfg(mobile)]
    let _ = (app, to);
}

const FLOAT_MIN_W: f64 = 160.0;

#[cfg(desktop)]
fn float_max_height(w: &tauri::WebviewWindow) -> f64 {
    w.current_monitor().ok().flatten()
        .map(|m| (m.size().height as f64 / m.scale_factor() - 80.0).max(40.0))
        .unwrap_or(800.0)
}

// 位置は物理px、高さは論理px。下端がはみ出すときだけ上へ戻す。
#[cfg(any(desktop, test))]
fn float_y_on_screen(y: i32, height: f64, scale: f64, top: i32, screen_height: u32) -> i32 {
    let bottom = top as f64 + screen_height as f64;
    if y as f64 + height * scale > bottom {
        (bottom - height * scale).floor().max(top as f64) as i32
    } else { y }
}

#[cfg(desktop)]
fn keep_float_on_screen(w: &tauri::WebviewWindow, height: f64) -> Result<(), String> {
    let e = |e: tauri::Error| e.to_string();
    let Some(m) = w.current_monitor().map_err(e)? else { return Ok(()) };
    let pos = w.outer_position().map_err(e)?;
    let y = float_y_on_screen(pos.y, height, w.scale_factor().map_err(e)?, m.position().y, m.size().height);
    if y != pos.y { w.set_position(tauri::PhysicalPosition::new(pos.x, y)).map_err(e)?; }
    Ok(())
}

#[tauri::command]
fn resize_float_by(app: AppHandle, delta: f64) -> Result<(), String> {
    if !delta.is_finite() { return Err("高さの差分が正しくありません".into()); }
    #[cfg(desktop)]
    {
        use tauri::LogicalSize;
        let Some(w) = app.get_webview_window("float") else { return Ok(()) };
        let e = |e: tauri::Error| e.to_string();
        let sf = w.scale_factor().map_err(e)?;
        let size = w.inner_size().map_err(e)?.to_logical::<f64>(sf);
        let h = (size.height + delta).clamp(40.0, float_max_height(&w));
        let pos = w.outer_position().map_err(e)?;
        w.set_size(LogicalSize::new(size.width, h)).map_err(e)?;
        w.set_position(pos).map_err(e)?;
        keep_float_on_screen(&w, h)?;
    }
    #[cfg(mobile)]
    let _ = (app, delta);
    Ok(())
}

#[tauri::command]
fn fit_float(app: AppHandle, fixed: bool, height: f64) -> Result<(), String> {
    if !height.is_finite() { return Err("高さが正しくありません".into()); }
    #[cfg(desktop)]
    {
        use tauri::LogicalSize;
        let Some(w) = app.get_webview_window("float") else { return Ok(()) };
        let e = |e: tauri::Error| e.to_string();
        if fixed {
            let h = height.clamp(24.0, float_max_height(&w));
            let sf = w.scale_factor().map_err(e)?;
            let width = w.inner_size().map_err(e)?.to_logical::<f64>(sf).width.max(FLOAT_MIN_W);
            let pos = w.outer_position().map_err(e)?;
            w.set_max_size(None::<LogicalSize<f64>>).map_err(e)?;
            w.set_min_size(Some(LogicalSize::new(FLOAT_MIN_W, h))).map_err(e)?;
            w.set_max_size(Some(LogicalSize::new(10_000.0, h))).map_err(e)?;
            w.set_size(LogicalSize::new(width, h)).map_err(e)?;
            w.set_position(pos).map_err(e)?;
            keep_float_on_screen(&w, h)?;
        } else {
            w.set_min_size(Some(LogicalSize::new(FLOAT_MIN_W, 40.0))).map_err(e)?;
            w.set_max_size(None::<LogicalSize<f64>>).map_err(e)?;
        }
    }
    #[cfg(mobile)]
    let _ = (app, fixed, height);
    Ok(())
}

/// 時間帯に入った毎日タイマーを自動で始める。ロックを外してから呼ぶこと。
fn auto_daily(app: &AppHandle, t: u64) {
    let ids: Vec<(String, u64)> = {
        let st = state(app);
        let mut d = st.data.lock().unwrap();
        let before = d.daily_skip.len();
        d.daily_skip.retain(|(_, w)| w + 2 * 86_400 > t);
        if d.daily_skip.len() != before {
            persist(&st, &d);
        }
        if !d.timers.iter().any(|x| x.daily.is_some()) {
            return;
        }
        let t0 = today0(t);
        d.timers
            .iter()
            .filter_map(|x| Some((x.id.clone(), timer::active_window(x.daily?, t0, t)?)))
            .filter(|(id, w)| {
                !d.running.iter().any(|r| &r.def_id == id) && !d.daily_skip.iter().any(|(sid, sw)| sid == id && sw == w)
            })
            .collect()
    };
    for (id, w) in ids {
        let _ = start_with(app, &id, Some(w));
    }
}

fn tick_all(app: &AppHandle) {
    let t = now();
    let st = state(app);
    let (events, running, focus, gimmick) = {
        let mut d = st.data.lock().unwrap();
        let mut events = Vec::new();
        for r in d.running.iter_mut() {
            if let Some(e) = timer::tick(r, t) {
                events.push((r.clone(), e));
            }
        }
        if !events.is_empty() {
            d.running.retain(|r| timer::remaining_secs(r, t) > 0);
            persist(&st, &d);
        }
        (events, d.running.clone(), d.focus.clone(), d.done_gimmick.clone())
    };
    #[cfg(desktop)]
    {
        let finished = events.iter().any(|(_, e)| *e == timer::Event::Finished);
        if finished && app.get_webview_window("float").is_some() {
            let mut list = st.finished.lock().unwrap();
            for (r, e) in &events {
                if *e == timer::Event::Finished && r.show_in_bar {
                    timer::push_finished(&mut list, timer::FinishedRow {
                        def_id: r.def_id.clone(), label: timer::label(r, &focus),
                        length: timer::fmt_duration(r.duration_secs), color: r.color.clone(),
                        segments: r.segments, period: r.period, once: r.once,
                        end_at: r.started_at + r.duration_secs, finished_at: t,
                    });
                }
            }
        }
        // 通知は全体で常に1つ。同じ tick で複数鳴っても出すのは1件だけ（終了を優先、なければ最後）。
        // 古い通知を消す処理は macOS のみ（notice.rs）。
        let pick = events.iter().rev().find(|(_, e)| *e == timer::Event::Finished).or(events.last());
        if let Some((r, e)) = pick {
            let (title, body) = timer::notification_text(r, &timer::label(r, &focus), *e, &gimmick);
            if app.notification().builder().title(title).body(body).show().is_ok() {
                #[cfg(target_os = "macos")]
                notice::shown();
            }
            // 通知センターの設定に左右されないよう、音はアプリから直接鳴らす
            #[cfg(target_os = "macos")]
            {
                // 区切りは Glass、終了は Hero
                let sound = if *e == timer::Event::Finished { "Hero" } else { "Glass" };
                let _ = std::process::Command::new("afplay")
                    .arg(format!("/System/Library/Sounds/{sound}.aiff"))
                    .spawn();
            }
        }
        #[cfg(target_os = "macos")]
        notice::tidy();
        if finished {
            tray::rebuild(app);
        }
        tray::update(app, &running, &focus, t);
    }
    // Android は OS へ予約済みなので、ここでは通知しない（二重通知を防ぐ）
    #[cfg(mobile)]
    let _ = (events, running, focus, gimmick);
    auto_daily(app, t);
}

/// Android：予約を全部取り消し、実行中と毎日タイマー（7日先まで）を入れ直す。
#[cfg(mobile)]
fn reschedule_all(app: &AppHandle) {
    use tauri_plugin_notification::Schedule;
    let t = now();
    let d = state(app).data.lock().unwrap().clone();
    let mut runs: Vec<Running> = d.running.clone();
    let t0 = today0(t);
    for def in &d.timers {
        let Some(dl) = def.daily else { continue };
        for w in timer::upcoming_windows(dl, t0, t, 7) {
            let taken = d.running.iter().any(|r| r.def_id == def.id && r.started_at == w)
                || d.daily_skip.iter().any(|(id, sw)| id == &def.id && *sw == w);
            if !taken {
                runs.push(Running::from_def(def, w, def.duration_secs));
            }
        }
    }
    let _ = app.notification().cancel_all();
    let mut id = 1;
    for r in &runs {
        for (at, title, body) in timer::schedule(r, &d.focus, &d.done_gimmick) {
            if at <= t {
                continue;
            }
            let Ok(date) = time::OffsetDateTime::from_unix_timestamp(at as i64) else { continue };
            let _ = app
                .notification()
                .builder()
                .id(id)
                .title(title)
                .body(body)
                .schedule(Schedule::At { date, repeating: false, allow_while_idle: true })
                .show();
            id += 1;
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            get_state,
            start_timer,
            stop_timer,
            save_timers,
            set_focus,
            set_done_gimmick,
            dismiss_finished,
            start_until,
            switch_view,
            fit_float,
            resize_float_by
        ])
        .on_window_event(|w, e| {
            // フローティングの大きさと位置を覚えておく（次に開くときに使う）
            if w.label() != "float" {
                return;
            }
            let st = state(w.app_handle());
            match e {
                tauri::WindowEvent::Resized(size) if size.width > 0 => {
                    let s = size.to_logical::<f64>(w.scale_factor().unwrap_or(1.0));
                    let mut d = st.data.lock().unwrap();
                    d.float_size = Some((s.width.round(), s.height.round()));
                    persist(&st, &d);
                }
                tauri::WindowEvent::Moved(p) => {
                    let mut d = st.data.lock().unwrap();
                    d.float_pos = Some((p.x, p.y));
                    persist(&st, &d);
                }
                _ => {}
            }
        })
        .setup(|app| {
            let path = app.path().app_data_dir()?.join("timers.json");
            let data = store::load(&path);
            let float = data.float;
            app.manage(AppState { data: Mutex::new(data), path, finished: Mutex::new(Vec::new()) });

            #[cfg(desktop)]
            {
                #[cfg(target_os = "macos")]
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                tray::build(app.handle())?;
                if float {
                    tray::open_float(app.handle());
                }
            }
            #[cfg(mobile)]
            {
                let _ = float;
                tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default()).build()?;
                let _ = app.notification().request_permission();
            }

            // 停止中に過ぎた区切りを処理してから常駐に入る
            tick_all(app.handle());
            #[cfg(mobile)]
            reschedule_all(app.handle());

            let h = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(500));
                tick_all(&h);
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("起動に失敗しました")
        .run(|_, e| {
            // 編集画面を閉じても常駐し続ける
            if let RunEvent::ExitRequested { api, code: None, .. } = e {
                api.prevent_exit();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_window_stays_inside_monitor() {
        assert_eq!(float_y_on_screen(100, 100.0, 2.0, 0, 1800), 100);
        assert_eq!(float_y_on_screen(1700, 100.0, 2.0, 0, 1800), 1600);
        assert_eq!(float_y_on_screen(-200, 200.0, 2.0, -1000, 1000), -400);
        assert_eq!(float_y_on_screen(0, 2000.0, 2.0, -1000, 1000), -1000);
    }

    #[test]
    fn finished_view_order_and_burning() {
        let row = |id: &str, at| timer::FinishedRow {
            def_id: id.into(), label: "付箋".into(), length: "30分".into(), color: None,
            segments: 20, period: false, once: true, end_at: at, finished_at: at,
        };
        let rows = vec![row("a", 100), row("b", 110)];
        for (t, fire, black) in [(110, 12, 0), (114, 12, 0), (115, 11, 1), (170, 0, 12), (999, 0, 12)] {
            let views = finished_views(&rows, &timer::DoneGimmick::Fire, t);
            assert_eq!(views[0].def_id, "b");
            assert_eq!(views[1].def_id, "a");
            assert_eq!(views[0].cells, Some("🔥".repeat(fire) + &"⬛".repeat(black)));
        }
        assert!(finished_views(&rows, &timer::DoneGimmick::Off, 110).iter().all(|v| v.cells.is_none()));
    }
}
