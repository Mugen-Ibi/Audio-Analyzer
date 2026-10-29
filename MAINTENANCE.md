# 保守・依存更新の検証（2026-10-29）

対象は公開 `master` の `ed3932cd81767aef8b5b07784ceb7cac55b73370`。
以前の未公開デスクトップ作業はこの環境になく、取り込んだとは扱わない。
大規模な再設計は行わず、既存のコールバック → DSP → UI の所有権と容量制限を維持した。

## 変更

- `eframe` 0.30 → 0.37、`egui_plot` 0.30 → 0.38。`App::ui`、`Panel`、角丸、フォント設定、テクスチャ更新のAPIへ移行した。
- 新版の既定WGPUへ暗黙に切り替えず、従来のOpenGL (`glow`) を明示的に維持した。
- `cpal` 0.18.2、`ringbuf` 0.5.3へ更新。`rustfft` 6.4.1は現行のまま。推移依存もlockfileで更新した。
- CIアクションを検証したコミットへ固定。最新stable Rustの非推奨APIとClippy指摘を修正した。最低Rustバージョンは1.95。
- 入力が1秒以上更新されない場合、LIVE用の数値・ゲート・ピーク・相関を無効化する。保持中のスペクトラムと履歴は消去せず、再受信でメーターが復帰する。
- 公開 `AudioBlock` の不正な長さはpanicせず破棄し、FFTの重複窓・平均履歴をリセットする。範囲外チャンネルの診断も整数オーバーフローしない。
- 大振幅でのコヒーレンス・相関・伝達利得の中間計算を広い精度で行い、有限入力からNaN/Infinityが生じる問題を修正した。
- 描画用の点列を借用し、各描画フレームでスペクトラム全体を新しい`Vec`へコピーしない。
- 開発ガイドの古いclone URLを現在の公開リポジトリへ修正した。

## 入力の数値契約

通常の有限な浮動小数点音声は±1へクリップしない。4倍・8倍のフルスケール、`1e10`の信号、420 dBの合成伝達利得を回帰試験した。
非有限値、およびFFT/power演算の安全限界 `4e18 / FFT_SIZE`（現在約`1.95e15`、300 dBFS超）を超える値だけを、従来のNaN処理と同じく**サンプル単位でゼロへ置換**する。飽和処理ではなく、不正な長さを持つブロックの履歴リセットとも別の処理である。

## 実行した検証

Linux、Rust 1.99.1で、以下を実行した。GUIのテストはヘッドレス描画であり、実ウィンドウの目視確認ではない。

| 検証 | 結果 |
| --- | --- |
| `cargo test --locked --no-default-features` | 36件成功 |
| `cargo test --release --locked --no-default-features` | 36件成功 |
| `cargo test --locked --no-default-features --features desktop` | 51件成功、実機1件はignore |
| `cargo test --release --locked --no-default-features --features desktop` | 51件成功、実機1件はignore |
| `cargo test --locked` | 51件成功、実機1件はignore（Linux） |
| コアClippy / 全feature・全target Clippy、`-D warnings` | 成功（Linux） |
| `cargo build --release --locked` / `cargo fmt --check` / `git diff --check` | 成功 |
| Rust 1.95.0でdesktop・全targetの`cargo check` | 成功（Linux） |
| `x86_64-pc-windows-msvc`向けdesktop・全targetの`cargo check`、ASIOなし | 成功（Linuxからのcross-check、Windowsで実行したものではない） |
| コールバック飽和＋FFTの確保回数テスト | 初期化後0回を維持 |
| `cargo audit` 0.22.2 | 脆弱性エントリー6件 → 0件、保守終了警告3件は残存 |

最初に4件の新しいコア回帰試験が失敗し、入力停止後の古いメーター表示も別のUI回帰試験で再現した。修正後はすべて成功した。
同一のRustFFTを使う合成DSPベンチマーク（50,000ブロック、交互に5回）の中央値は、旧コード39.8 µs/256フレーム、新コード38.1 µsだった。共有CPU上ではばらつきが大きく、速度改善や実機レイテンシの保証とは扱わない。

## 残る制約

- Windows上のビルド・実行、ASIO SDK/ドライバー、実入力の48/96/192 kHz、ネイティブGUI、長時間負荷・レイテンシは未検証。
- Linuxの全feature成功は、Windows専用ASIOコードのビルド成功を意味しない。
- RustSecの保守終了警告は、最新版`eframe`のアクセシビリティ経路 (`accesskit` → `atspi`/`zbus`) が依存する`zbus_names`、`zvariant`、`zvariant_derive`。互換性のない推移依存の強制上書きやアクセシビリティの削除は行っていない。
- 公開ブランチの既存CIは失敗していたが、過去の原因を今回のローカル再現だけで断定しない。変更後のリモートCI・PR・mergeは未実行。
- 依存監査は既知advisoryとの照合であり、6件すべてがこのアプリから実際に悪用可能だったと証明するものではない。

依存情報: [eframe 0.37](https://github.com/emilk/egui/blob/0.37.0/crates/eframe/CHANGELOG.md)、[egui_plot 0.38](https://github.com/emilk/egui_plot/blob/0.38.0/CHANGELOG.md)、[RustSec](https://rustsec.org/)。
