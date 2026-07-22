# Pro Audio Analyzer

Rustで実装された、同期2チャンネル入力対応のリアルタイム・オーディオアナライザーです。現在は同一入力ストリーム上のReference/Measurement信号を解析し、スペクトラム、H1伝達関数、位相、コヒーレンスを表示できます。

## 設計

処理は互いにブロックしない3つの実行コンテキストに分離されています。

1. **Audio I/O callback**: CPALから入力されたチャンネルを同一時刻のステレオフレームとして固定長ブロックへ格納します。
2. **DSP worker**: 同期FFT、Hann窓補正、伝達関数、位相、コヒーレンスを計算します。
3. **UI thread**: DSPが生成した最新結果のみを描画します。

Audio→DSPおよびDSP→UI間は、容量制限されたロックフリーSPSCリングバッファで接続されています。Audioコールバックはヒープ確保や待機を行いません。入力キューが満杯の場合は2チャンネルを含むブロック全体を破棄し、欠落フレーム数を画面に表示します。

## 現在の機能

- WindowsでASIOを優先し、利用できない場合はWASAPIへフォールバック
- デバイスのサンプリングレートと入力チャンネル数を自動取得
- 任意のReference/Measurement入力チャンネル割り当て
- モノラル入力またはReference無効時のスペクトラム専用モード
- 2048ポイントFFT、50%オーバーラップ、Hann窓の振幅補正
- Measurementスペクトラム（dBFS）
- H1伝達関数、位相、コヒーレンス
- 真の対数周波数表示
- 入力欠落、ストリームエラー、表示結果欠落の監視
- ストリームとDSPワーカーの明示的な終了処理

## 同期保証の範囲

ReferenceとMeasurementは、**同一デバイスの同一入力ストリーム**に含まれるチャンネルである必要があります。別デバイス間、または独立した入力・出力ストリーム間のクロック同期やドリフト補正は未対応です。

WASAPIフォールバックはCPALの共有モードを使用します。WASAPI Exclusiveやビットパーフェクト入力を保証する実装ではありません。

## ビルド

Rust Edition 2024を使用します。WindowsでASIOを有効にしているため、Visual Studio C++ Build ToolsとLLVM/Clangが必要です。

```powershell
cargo run --release
```

検証コマンド:

```powershell
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## 次の課題

- 実機による48/96/192kHz動作検証と性能計測
- デバイス・サンプルレート・バッファサイズ選択UI
- 音響遅延の推定と位相アンラップ
- 平均方法とFFTサイズの設定
- ASIO入出力を利用したリファレンス信号生成
- 別デバイス対応に必要なタイムスタンプ処理とドリフト補正
