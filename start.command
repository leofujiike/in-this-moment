#!/bin/bash
# In This Moment 起動スクリプト（ダブルクリックで実行）
cd "$(dirname "$0")" || exit 1
set -e

if ! xcode-select -p >/dev/null 2>&1; then
  echo "Xcode Command Line Tools をインストールします。ダイアログで「インストール」を押し、終わったらもう一度このファイルを開いてください。"
  xcode-select --install || true
  read -n 1 -s -r -p "何かキーを押すと閉じます"; exit 0
fi

if ! command -v cargo >/dev/null 2>&1; then
  [ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
fi
if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust をインストールします…"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$HOME/.cargo/env"
fi

if ! command -v npm >/dev/null 2>&1; then
  echo "Node.js が見つかりません。https://nodejs.org から入れてから、もう一度開いてください。"
  read -n 1 -s -r -p "何かキーを押すと閉じます"; exit 1
fi

[ -d node_modules ] || npm install

if [ "$1" = "debug" ]; then
  echo "動作確認モードで起動します（タイマーが1/60の長さ：5分→5秒）。"
  BLOCK_TIMER_DEBUG=1 npm run tauri dev
else
  echo "起動します。"
  npm run tauri dev
fi
