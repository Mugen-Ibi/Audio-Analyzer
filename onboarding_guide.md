# Pro Audio Analyzer - Developer Onboarding Guide

ようこそ！このドキュメントは、本プロジェクトに新たに参加する開発者が、プロジェクトの「背景・美学・技術スタック」を深く理解し、**環境構築から最初のコード貢献までのハードルをゼロにする**ための羅針盤です。

## 1. プロジェクトの背景とビジョン

既存のプロフェッショナル向けオーディオアナライザー（例: iZotope Insight 2など）の多くは、DAWのプラグイン（VST/AU/AAX）として動作し、C++およびJUCEフレームワークで構築されています。

しかし、レコーディング環境の厳密な検証（マイクの位相チェック、ノイズフロアの監視など）を行う際、DAWのプロセスに依存することは以下のリスクを伴います。

- DAWの高負荷によるクラッシュに巻き込まれるリスク。
    
- DAW内部のミキサーやパンロウ（Pan Law）設定による「入力信号の意図せぬ変質」。
    

本プロジェクトは、「OSのオーディオミキサーを完全にバイパスし、ハードウェアからの入力信号をビットパーフェクトに監視する完全独立型の解析プラットフォーム」を構築することを目的としています。Rustの「メモリ安全性」と「データ競合のない並行処理（Fearless Concurrency）」を武器に、C++の既存製品を凌駕する次世代のオーディオツールを目指しています。

## 2. 設計の美学（Core Aesthetics）

コードを書く際は、常に以下の「第一原理」を念頭に置いてください。

1. **ASIO / Core Audio First (OSミキサーのバイパス)**
    
    リサンプリングによる音質劣化を防ぐため、デバイスのネイティブ・サンプリングレートを絶対的に尊重します。Windowsでは `cpal` の ASIOバックエンドを最優先とします。
    
2. **Zero-Allocation in Audio Callback (リアルタイム性の死守)**
    
    オーディオコールバックでの動的メモリ確保、ロック、待機を禁止します。DSPの作業領域も初期化時に確保し、処理中に容量拡張しないよう再利用します。
    
3. **Lock-Free Data Flow (スレッド間の非同期性)**
    
    オーディオ（DSP）スレッドとUI（描画）スレッド間の通信に `std::sync::Mutex` は使用しません。OSのスケジューラによるブロックを防ぐため、必ず Lock-free Ring Buffer（`ringbuf` クレート等）を使用します。「UIが重くても音は途切れない、音が重くてもUIは止まらない」が絶対ルールです。
    

## 3. 技術スタック

本プロジェクトは **Full Rust (100%)** で構築されています。

- **Audio I/O:** `cpal` (ASIO feature enabled)
    
- **DSP / Math:** `rustfft` (高速フーリエ変換)
    
- **Concurrency:** `ringbuf` (Lock-free SPSC Ring Buffer)
    
- **GUI Framework:** `eframe` / `egui` (Immediate Mode GUI)
    
- **Plotting:** `egui_plot`
    

## 4. アーキテクチャの全体像 (3-Thread Model)

現在は以下の3スレッドモデルで構築されています。

1. **Audio I/O Thread**: 同一入力ストリームのReference/Measurementを同期ブロックとしてRing Bufferへ流し込む。
    
2. **DSP Worker Thread**: Ring Bufferからブロックを取り出し、同期FFT、伝達関数、位相、コヒーレンスを計算する。
    
3. **UI / Render Thread**: DSPスレッドが計算した最新結果を受け取り、`egui`で描画する。
    

## 5. 実装までのハードル・ゼロ設定ガイド (Setup Guide)

開発を始めるためのステップです。特にWindows環境でのASIOビルドに注意してください。

### Step 1: Rustツールチェーンの準備

```
rustup update
```

### Step 2: C++ ビルドツールのインストール (Windowsのみ)

`cpal` の `asio` フィーチャーをビルドするためには、C++コンパイラとLLVM/Clang（`bindgen`用）が必要です。

1. **Visual Studio Build Tools**: C++によるデスクトップ開発ワークロードをインストール。
    
2. **LLVM**: 公式サイトからLLVMをインストールし、環境変数 `PATH` に `C:\Program Files\LLVM\bin` を追加。
    
3. **環境変数の設定**: `LIBCLANG_PATH` に `C:\Program Files\LLVM\bin` を設定。
    

### Step 3: クローンとビルドテスト

```
git clone https://github.com/owata2022/rust-audio-analyzer.git
cd rust-audio-analyzer
# asioフィーチャーが有効になっているため、そのままビルド可能です
cargo build
cargo run
```

## 6. 現在の課題

1. 48/96/192kHzおよびASIO/WASAPI環境での実機性能試験。
2. デバイス選択、FFTサイズ、平均方法、位相アンラップの設定UI。
3. 同一デバイスの入出力ストリーム間における遅延推定。
4. 別デバイス間のクロックドリフト検出と補正。

同期保証の対象は、同一デバイス・同一入力ストリーム内のチャンネルです。WASAPIは共有モードであり、Exclusive動作や別デバイス同期を前提にしてはいけません。
