//! macOS：通知センターに残る通知を常に1つにする。
//! tauri-plugin-notification は macOS で届いた通知を消せないので、plugin が使っている
//! NSUserNotificationCenter（mac-notification-sys と同じ仕組み）へ直接頼んで古いものを消す。
//! 通知の配送は plugin 側のバックグラウンドで行われるため、出した直後ではなく少し待ってから整理する。
//! 最新の1つを残すので、前回起動時から残っている通知も、最初の通知のあとに片づく。
//! 注意：開発実行（tauri dev）では plugin が通知元を Terminal に偽装するため、Terminal の通知も対象になる。
#[allow(deprecated)]
use objc2_foundation::NSUserNotificationCenter;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const SETTLE: Duration = Duration::from_millis(1500);

/// 整理する時刻。通知を出すたびに先へ延ばす。
static DUE: Mutex<Option<Instant>> = Mutex::new(None);

/// 通知を出した直後に呼ぶ。
pub fn shown() {
    *DUE.lock().unwrap() = Some(Instant::now() + SETTLE);
}

/// 定期処理（tick）から呼ぶ。待ち時間が過ぎていたら、届いている通知を最新の1つだけにする。
pub fn tidy() {
    let due = {
        let mut d = DUE.lock().unwrap();
        match *d {
            Some(t) if Instant::now() >= t => {
                *d = None;
                true
            }
            _ => false,
        }
    };
    if due {
        keep_newest();
    }
}

#[allow(deprecated)] // NSUserNotification は非推奨だが、plugin（notify-rust）が使っているのはこちら
fn keep_newest() {
    let center = NSUserNotificationCenter::defaultUserNotificationCenter();
    let list = center.deliveredNotifications();
    let n = list.count();
    let when = |i| {
        list.objectAtIndex(i)
            .actualDeliveryDate()
            .map(|d| d.timeIntervalSinceReferenceDate())
            .unwrap_or(f64::MIN)
    };
    let Some(newest) = (0..n).max_by(|&a, &b| when(a).total_cmp(&when(b))) else { return };
    for i in (0..n).filter(|&i| i != newest) {
        center.removeDeliveredNotification(&list.objectAtIndex(i));
    }
}
