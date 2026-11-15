# In This Moment

付箋に「いま何をやるか」を書き、30分・5分などのプリセットや「終了日時を決めて始める」1回きりのタイマーで、残り時間を20ブロックで通知する。人生のような長い期間は名前付きの期間タイマーで持つ。
Mac はメニューバー常駐、Android は一覧画面＋予約通知。

## Mac
    npm install
    npm run tauri dev                      # 起動（メニューバーに常駐）
    BLOCK_TIMER_DEBUG=1 npm run tauri dev  # 動作確認用：開始したタイマーが1/60の長さ
    npm run tauri build                    # src-tauri/target/release/bundle/macos/In This Moment.app

## Android（Android Studio・SDK・NDK と環境変数が必要）
    npm run tauri android init
    # src-tauri/gen/android/app/src/main/AndroidManifest.xml の <application> より前に追記：
    #   <uses-permission android:name="android.permission.SCHEDULE_EXACT_ALARM" />
    npm run tauri android dev                       # USB接続した実機で起動
    npm run tauri android build -- --apk --debug    # インストール用APK

## テスト
    cd src-tauri && cargo test

## 最近の追加機能

- 完了ギミック：設定で「なし・爆発・お祝い・燃え尽き・花・自由入力」を選択。自由入力は見た目で1文字。通知は最大12マス。
- 自動の色：残り75・50・25・10%以下で青→緑→黄→橙→赤。色の判定とギミックの生成は Rust に集約。
- Mac のフローティング：終了済み行を保持し、×で削除。実行中の行は■で停止。プリセットを固定ボタンとして表示。
- 行数制限を撤廃。高さ固定オンでは内容に合わせ、オフでは1行につき約28px伸縮。モニターを超える高さを制限。

`timers.json` は version 2 のまま。既存データの追加項目は既定値で読み込みます。終了済み行は保存せず、再起動・同じタイマーの再開始で消えます。フローティングを閉じる前の終了済み行は再表示時にも残り、閉じている間の終了分は追加しません。ギミックは終了時の設定を使用します。

## UI の回帰テスト

リポジトリ直下で `python3 -m http.server 8765 --bind 127.0.0.1` を実行し、[UIテスト](http://127.0.0.1:8765/tests/ui.html) を開きます。バックエンドと保存先を模擬し、実データを変更せずに Mac・Android 表示とフローティングを検証します。

2026-11-10 の確認結果：Rust 36テスト、UI 57項目が成功。Mac の隔離したテストアプリで、自動色、完了表示、固定ボタン、再開始、停止、削除、高さ固定オン／オフの伸縮を確認しました。Android 実機、通知バナーの折り返し、複数モニターでの画面端補正は未確認です。

## ライセンス

[MIT License](./LICENSE)
