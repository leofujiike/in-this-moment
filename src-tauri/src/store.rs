//! timers.json の読み書きと入力チェック。Tauri には依存しない。
use crate::timer::{self, Notify, Running, TimerDef, Unit, COLORS, MAX_SECS, MIN_SECS};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 保存形式の版。2 未満（v0.3 以前）は読み込み時に初期化する。
pub const VERSION: u32 = 2;
/// 付箋の最大文字数
pub const FOCUS_MAX: usize = 60;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Data {
    #[serde(default)]
    pub done_gimmick: timer::DoneGimmick,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub timers: Vec<TimerDef>,
    #[serde(default)]
    pub running: Vec<Running>,
    /// 手で止めた毎日タイマー：(def_id, その日の時間帯の開始)
    #[serde(default)]
    pub daily_skip: Vec<(String, u64)>,
    /// 付箋（いま何をやるか）。1枚だけ、実行中でも変えられる
    #[serde(default)]
    pub focus: String,
    /// Mac のフローティング表示
    #[serde(default)]
    pub float: bool,
    /// フローティングの大きさ（論理px）
    #[serde(default)]
    pub float_size: Option<(f64, f64)>,
    /// フローティングの位置（物理px、画面の左上から）
    #[serde(default)]
    pub float_pos: Option<(i32, i32)>,
}

/// 付箋の整形：改行は空白にし、前後の空白を除き、60文字までにする。
pub fn normalize_focus(s: &str) -> String {
    let one: String = s.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    one.trim().chars().take(FOCUS_MAX).collect::<String>().trim_end().to_string()
}

pub fn validate_gimmick(g: &timer::DoneGimmick) -> Result<(), String> {
    if let timer::DoneGimmick::Custom(s) = g {
        if s.is_empty() || s.chars().count() > 12 || s.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err("ギミックの文字は、見た目で1文字の絵文字などにしてください".into());
        }
    }
    Ok(())
}

pub fn defaults() -> Data {
    let def = |id: &str, secs| TimerDef {
        id: id.into(),
        name: String::new(),
        duration_secs: secs,
        unit: Unit::Min,
        notify: Notify::Standard,
        show_in_bar: true,
        start_at: None,
        daily: None,
        color: None,
        segments: timer::SEGMENTS_DEFAULT,
        pin_float: false,
    };
    Data {
        version: VERSION,
        timers: vec![def("t-default-30", 1800), def("t-default-5", 300)],
        ..Default::default()
    }
}

pub fn load(path: &Path) -> Data {
    if !path.exists() {
        return defaults();
    }
    match fs::read(path).ok().and_then(|b| serde_json::from_slice::<Data>(&b).ok()) {
        Some(d) if d.version >= VERSION => d,
        Some(old) => {
            // v0.3 以前：タイマーと実行状態は捨てて初期化し、画面の設定だけ引き継ぐ
            let d = Data { float: old.float, float_size: old.float_size, float_pos: old.float_pos, ..defaults() };
            let _ = save(path, &d);
            d
        }
        None => {
            let _ = fs::rename(path, path.with_extension("json.bak"));
            defaults()
        }
    }
}

pub fn save(path: &Path, data: &Data) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(data)?)?;
    fs::rename(tmp, path)
}

pub fn new_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    format!("t{}-{}", ms, SEQ.fetch_add(1, Ordering::Relaxed))
}

pub fn validate(defs: &mut [TimerDef], now: u64) -> Result<(), String> {
    validate_against(defs, now, &[])
}

/// prev は保存済みの一覧。開始日と長さが変わっていない行は「もう終わっています」で止めない
/// （終わった期間タイマーが1本あるだけで、ほかの行まで保存できなくなるのを防ぐ）。
pub fn validate_against(defs: &mut [TimerDef], now: u64, prev: &[TimerDef]) -> Result<(), String> {
    for (i, d) in defs.iter_mut().enumerate() {
        let row = i + 1;
        if !timer::segments_ok(d.segments) {
            return Err(format!("{row}行目：メモリ数は2〜60で入れてください"));
        }
        d.name = d.name.trim().to_string();
        if d.is_period() {
            d.pin_float = false;
            // 名前を持つのは期間タイマーだけ
            let n = d.name.chars().count();
            if n == 0 {
                return Err(format!("{row}行目：名前を入れてください"));
            }
            if n > 30 {
                return Err(format!("{row}行目：名前は30文字以内にしてください"));
            }
        } else {
            d.name.clear();
        }
        d.color = d.color.take().filter(|c| c == timer::COLOR_AUTO || COLORS.iter().any(|(k, _)| k == c));
        if let Some(dl) = d.daily {
            if dl.start_min > 1439 || dl.end_min > 1439 || dl.start_min == dl.end_min {
                return Err(format!("{row}行目：開始と終了の時刻を正しく入れてください"));
            }
            if d.start_at.is_some() {
                return Err(format!("{row}行目：毎日と開始日は同時に使えません"));
            }
            d.duration_secs = timer::daily_len(dl);
            d.unit = Unit::Min;
        }
        if d.duration_secs < MIN_SECS || d.duration_secs > MAX_SECS {
            return Err(format!("{row}行目：時間は1分〜100年にしてください"));
        }
        if let Some(s) = d.start_at {
            if s > now {
                return Err(format!("{row}行目：開始日に未来の日付は使えません"));
            }
            let unchanged = prev.iter().any(|p| p.id == d.id && p.start_at == d.start_at && p.duration_secs == d.duration_secs);
            if s + d.duration_secs <= now && !unchanged {
                return Err(format!("{row}行目：この開始日ではもう終わっています"));
            }
        }
        if d.id.is_empty() {
            d.id = new_id();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timer::Daily;
    const NOW: u64 = 2_000_000_000;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("block-timer-test-{tag}-{}", new_id()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn load_missing_gives_defaults() {
        assert_eq!(load(&tmpdir("missing").join("timers.json")), defaults());
    }

    #[test]
    fn roundtrip() {
        let p = tmpdir("rt").join("sub/timers.json");
        let mut d = defaults();
        d.timers[0].notify = Notify::Every10;
        save(&p, &d).unwrap();
        assert_eq!(load(&p), d);
    }

    #[test]
    fn broken_json_backed_up() {
        let dir = tmpdir("broken");
        let p = dir.join("timers.json");
        fs::write(&p, b"{not json").unwrap();
        assert_eq!(load(&p), defaults());
        assert!(dir.join("timers.json.bak").exists());
    }

    #[test]
    fn validation() {
        let base = TimerDef { name: "x".into(), ..defaults().timers[0].clone() };
        let check = |f: &dyn Fn(&mut TimerDef)| {
            let mut v = vec![base.clone()];
            f(&mut v[0]);
            validate(&mut v, NOW)
        };
        // 期間タイマーだけ名前が必要
        let period = |d: &mut TimerDef| d.start_at = Some(NOW - 100);
        assert!(check(&|d| {
            period(d);
            d.name = "  ".into();
        })
        .is_err());
        assert!(check(&|d| {
            period(d);
            d.name = "あ".repeat(31);
        })
        .is_err());
        assert!(check(&|d| {
            period(d);
            d.name = "あ".repeat(30);
        })
        .is_ok());
        assert!(check(&|d| d.name = "  ".into()).is_ok());
        assert!(check(&|d| d.duration_secs = 59).is_err());
        assert!(check(&|d| d.duration_secs = MAX_SECS + 1).is_err());
        assert!(check(&|d| d.duration_secs = MAX_SECS).is_ok());
        assert!(check(&|d| d.daily = Some(Daily { start_min: 600, end_min: 600 })).is_err());
        assert!(check(&|d| d.daily = Some(Daily { start_min: 0, end_min: 1440 })).is_err());
        assert!(check(&|d| {
            d.daily = Some(Daily { start_min: 360, end_min: 1380 });
            d.start_at = Some(NOW - 100);
        })
        .is_err());
        assert!(check(&|d| d.start_at = Some(NOW + 1)).is_err());
        assert!(check(&|d| d.start_at = Some(NOW - 1800)).is_err());
        assert!(check(&|d| d.start_at = Some(NOW - 1799)).is_ok());
        // 期間タイマーの名前は前後の空白を除く。作業タイマーの名前は消える
        let mut v = vec![TimerDef { id: String::new(), name: " A ".into(), start_at: Some(NOW - 100), ..base.clone() }];
        validate(&mut v, NOW).unwrap();
        assert_eq!(v[0].name, "A");
        assert!(!v[0].id.is_empty());
        let mut v = vec![TimerDef { name: "残り".into(), ..base.clone() }];
        validate(&mut v, NOW).unwrap();
        assert_eq!(v[0].name, "");
        // 色は知っているキーだけ残る
        let mut v = vec![TimerDef { color: Some("blue".into()), ..base.clone() }, TimerDef { color: Some("zzz".into()), ..base.clone() }];
        validate(&mut v, NOW).unwrap();
        assert_eq!(v[0].color.as_deref(), Some("blue"));
        assert_eq!(v[1].color, None);
        v[0].color = Some(timer::COLOR_AUTO.into());
        validate(&mut v, NOW).unwrap();
        assert_eq!(v[0].color.as_deref(), Some(timer::COLOR_AUTO));
        let mut v = vec![TimerDef { name: "夜".into(), daily: Some(Daily { start_min: 1320, end_min: 360 }), ..base.clone() }];
        validate(&mut v, NOW).unwrap();
        assert_eq!(v[0].duration_secs, 8 * 3600);
    }

    #[test]
    fn expired_saved_row_does_not_block_save() {
        let base = TimerDef { id: "p".into(), name: "x".into(), start_at: Some(NOW - 5000), duration_secs: 1800, ..defaults().timers[0].clone() };
        // 終わった期間タイマーを新しく入れるのはエラー
        let mut v = vec![base.clone()];
        assert!(validate(&mut v, NOW).is_err());
        // すでに保存済みで変わっていなければ通る（ほかの行の編集を止めない）
        let mut v = vec![base.clone(), TimerDef { id: "q".into(), ..defaults().timers[1].clone() }];
        assert!(validate_against(&mut v, NOW, &[base.clone()]).is_ok());
        // 開始日か長さを変えたらチェックされる
        let mut v = vec![TimerDef { duration_secs: 60, ..base.clone() }];
        assert!(validate_against(&mut v, NOW, &[base.clone()]).is_err());
        let mut v = vec![TimerDef { start_at: Some(NOW - 6000), ..base.clone() }];
        assert!(validate_against(&mut v, NOW, &[base.clone()]).is_err());
    }

    #[test]
    fn old_file_is_reset() {
        // v0.3 のファイル（version なし）：タイマー・実行状態は初期化、画面の設定は残す
        let p = tmpdir("old").join("timers.json");
        fs::write(
            &p,
            r#"{"timers":[{"id":"a","name":"n","duration_secs":60,"unit":"min","project":"P"}],
                "running":[{"def_id":"a","slot":1,"name":"n","duration_secs":60,"notify":"standard","started_at":5,"next":0}],
                "history":[{"def_id":"a","name":"n","project":null,"from":1,"secs":2}],
                "daily_skip":[["a",1]],"float":true,"float_size":[200.0,300.0],"float_pos":[10,20]}"#
                .as_bytes(),
        )
        .unwrap();
        let d = load(&p);
        assert_eq!(d.version, VERSION);
        assert_eq!(d.timers, defaults().timers);
        assert!(d.running.is_empty() && d.daily_skip.is_empty());
        assert_eq!(d.focus, "");
        assert!(d.float);
        assert_eq!(d.float_size, Some((200.0, 300.0)));
        assert_eq!(d.float_pos, Some((10, 20)));
        // すぐ保存されていて、次回はそのまま読める
        assert_eq!(load(&p), d);
        assert!(!fs::read_to_string(&p).unwrap().contains("history"));
    }

    #[test]
    fn segments_validation_and_compatibility() {
        for n in [1, 61] {
            let mut defs = defaults().timers;
            defs[0].segments = n;
            assert_eq!(validate(&mut defs, NOW).unwrap_err(), "1行目：メモリ数は2〜60で入れてください");
        }
        for n in [2, 60] {
            let mut defs = defaults().timers;
            defs[0].segments = n;
            assert!(validate(&mut defs, NOW).is_ok());
        }
        let p = tmpdir("segments").join("timers.json");
        let mut json = serde_json::to_value(defaults()).unwrap();
        json["timers"][0].as_object_mut().unwrap().remove("segments");
        json["timers"][1].as_object_mut().unwrap().remove("segments");
        json["focus"] = serde_json::json!("保存した付箋");
        fs::write(&p, serde_json::to_vec(&json).unwrap()).unwrap();
        let bytes = fs::read(&p).unwrap();
        let d = load(&p);
        assert_eq!(d.version, VERSION);
        assert_eq!(d.focus, "保存した付箋");
        assert!(d.timers.iter().all(|t| t.segments == 20));
        assert_eq!(fs::read(&p).unwrap(), bytes); // 初期化・再保存しない
    }

    #[test]
    fn pinned_presets_only() {
        let mut defs = defaults().timers;
        defs[0].pin_float = true;
        defs[1].pin_float = true;
        defs[1].name = "毎日".into();
        defs[1].daily = Some(Daily { start_min: 360, end_min: 1380 });
        validate(&mut defs, NOW).unwrap();
        assert!(defs[0].pin_float);
        assert!(!defs[1].pin_float);
        defs[0].start_at = Some(NOW - 1);
        defs[0].name = "期間".into();
        validate(&mut defs, NOW).unwrap();
        assert!(!defs[0].pin_float);
    }

    #[test]
    fn gimmick_validation_and_compatibility() {
        use timer::DoneGimmick::*;
        for s in ["🌟", "👨‍👩‍👧‍👦"] { assert!(validate_gimmick(&Custom(s.into())).is_ok()); }
        for s in ["", " ", "a\nb", "1234567890123"] { assert!(validate_gimmick(&Custom(s.into())).is_err()); }
        for g in [Off, Boom, Party, Fire, Flower] { assert!(validate_gimmick(&g).is_ok()); }
        let p = tmpdir("gimmick").join("timers.json");
        let mut d = defaults();
        d.done_gimmick = Custom("👨‍👩‍👧‍👦".into());
        save(&p, &d).unwrap();
        assert_eq!(load(&p), d);
        let mut v = serde_json::to_value(&d).unwrap();
        v.as_object_mut().unwrap().remove("done_gimmick");
        let bytes = serde_json::to_vec(&v).unwrap();
        fs::write(&p, &bytes).unwrap();
        let loaded = load(&p);
        assert_eq!(loaded.done_gimmick, Off);
        assert_eq!(loaded.timers, d.timers);
        assert_eq!(loaded.version, 2);
        assert_eq!(fs::read(&p).unwrap(), bytes);
    }

    #[test]
    fn focus_normalized() {
        assert_eq!(normalize_focus("  書類作成 \n 続き "), "書類作成   続き");
        assert_eq!(normalize_focus(""), "");
        assert_eq!(normalize_focus(&"あ".repeat(80)).chars().count(), FOCUS_MAX);
    }
}
