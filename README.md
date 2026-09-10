# Pro Audio Analyzer

Rustで実装された、同期2チャンネル入力対応のリアルタイム・オーディオアナライザーです。現在は同一入力ストリーム上のReference/Measurement信号を解析し、スペクトラム、H1伝達関数、位相、コヒーレンスを表示できます。

## 設計

要件から3案を比較し、**解析コア・入力アダプター・実行制御・UIを分離する構成**を採用しました。比較表、依存方向、所有権、容量・失敗時の契約は [ARCHITECTURE.md](ARCHITECTURE.md) に記載しています。

- `capture` / `dsp` / `model`: デバイスとGUIに依存しない入力・解析コア。
- `source` / `audio`: 入力ポートとCPALアダプター。別の入力実装も同じ解析経路に接続できます。
- `pipeline`: 入力セッションとDSPワーカーの組立・終了。
- `controller`: 接続・停止を専用スレッドで処理し、状態と結果受信端をUIへ渡します。
- `ui`: 最新の解析結果と接続状態を表示します。

音声コールバック→DSP→UIのデータ経路は、容量制限されたロックフリーSPSCリングバッファ2本です。制御スレッドは解析データを中継しません。コールバックはヒープ確保・ロック・待機を行わず、FFT作業領域も初期化時に確保して再利用します。

入力キューが満杯の場合は2チャンネルを含むブロック全体を破棄し、欠落を計数します。入力切替時に破棄した未完成ブロックも加算します。表示キューが満杯の場合は最新の未送信結果を1個保持し、空きができたら再送します。入力切替前の結果は世代番号で除外します。

接続ごとにキューと統計を作り直し、停止時は入力を停止してからDSPを終了します。入力が使えなくても画面は起動し、接続失敗の理由を確認して「接続 / 再試行」できます。「停止」は接続処理中のキャンセルにも使えます。ドライバー内部の呼び出し自体は強制中断できません。

## 現在の機能

- WindowsでASIOを優先し、利用できない場合はWASAPIへフォールバック
- 開始後2秒以内に解析窓1個分のデータが届かない入力バックエンドもフォールバック対象
- デバイスのサンプリングレートと入力チャンネル数を自動取得
- 任意のReference/Measurement入力チャンネル割り当て
- モノラル入力またはReference無効時のスペクトラム専用モード
- 2048ポイントFFT、50%オーバーラップ、Hann窓の振幅補正
- Measurementスペクトラム（dBFS）
- H1伝達関数、位相、コヒーレンス
- 真の対数周波数表示
- 入力欠落、ストリームエラー、表示結果欠落の監視
- ストリームとDSPワーカーの明示的な終了処理
- 非同期接続、失敗理由の表示、停止・再接続、起動途中のキャンセル

## 同期保証の範囲

ReferenceとMeasurementは、**同一デバイスの同一入力ストリーム**に含まれるチャンネルである必要があります。別デバイス間、または独立した入力・出力ストリーム間のクロック同期やドリフト補正は未対応です。

WASAPIフォールバックはCPALの共有モードを使用します。WASAPI Exclusiveやビットパーフェクト入力を保証する実装ではありません。

## ビルド

Rust Edition 2024を使用します。WindowsでASIOを有効にしているため、Visual Studio C++ Build ToolsとLLVM/Clangが必要です。

```powershell
cargo run --release
```

`asio-sys` は初回ビルド時にSteinberg ASIO SDKを `%TEMP%\asio_sdk` へキャッシュします。以前の取得が中断されるなどして空のキャッシュだけが残ると、`asiodrivers.h: No such file or directory` でビルドが失敗します。次の確認が `False` の場合はキャッシュを退避してから再実行してください。SDKは次回ビルド時に公式配布元から再取得されます。

```powershell
Test-Path "$env:TEMP\asio_sdk\host\asiodrivers.h"
Rename-Item "$env:TEMP\asio_sdk" "asio_sdk.invalid"
cargo build --release --locked
```

`asio_sdk.invalid` がすでに存在する場合は、別の退避名を指定してください。LLVMを標準以外の場所へインストールした場合は、あわせて `LIBCLANG_PATH` をLLVMの `bin` ディレクトリへ設定します。

ASIO SDKを利用しないWindows GUI（WASAPI）:

```powershell
cargo run --release --locked --no-default-features --features desktop
```

解析・キャプチャ・実行制御だけを検証（オーディオデバイス、GUI、ASIO SDK不要）:

```powershell
cargo test --locked --no-default-features
```

デフォルトfeatureは `desktop + asio` です。`audio-device` はCPALのみ、`desktop` はCPALとGUI、`asio` はASIOを追加します。SDKを明示する場合は `CPAL_ASIO_DIR` をSDKの `common` / `host` があるディレクトリへ設定してください。

検証コマンド:

```powershell
cargo fmt --check
cargo test
cargo test --release --locked
cargo clippy --all-targets --all-features -- -D warnings
```

入力デバイスを使用する任意の結合テスト（音声ファイルは保存しません）:

```powershell
cargo test --locked --test live_audio -- --ignored --nocapture
```

このテストは制御スレッドでの接続、入力受信、Reference無効化・復元、停止・再接続、終了処理を確認します。通常のテストとCIでは実行しません。デバイスが利用できなければ失敗し、バックエンド別の理由を表示します。

再設計の検証結果とASIO SDKの環境設定は [ARCHITECTURE.md](ARCHITECTURE.md)、以前のレビュー記録は [REVIEW_PLAN.md](REVIEW_PLAN.md) を参照してください。

## 今後の機能拡張・実機検証

- 実機による48/96/192kHz動作検証と性能計測
- デバイス・サンプルレート・バッファサイズ選択UI
- 音響遅延の推定と位相アンラップ
- 平均方法とFFTサイズの設定
- ASIO入出力を利用したリファレンス信号生成
- 別デバイス対応に必要なタイムスタンプ処理とドリフト補正
