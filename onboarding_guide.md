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
    
2. **Zero-Allocation in Hot Path (リアルタイム性の死守)**
    
    オーディオコールバックやDSP処理のループ内（Hot Path）での動的メモリ確保（`Vec::push`, `collect`, `String`の生成など）を**一切禁止**します。すべてのメモリは初期化時に事前確保（Pre-allocation）し、上書き再利用します。
    
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

現在はPhase 4の移行期にあり、最終的には以下の3スレッドモデルを構築します。

1. **Audio I/O Thread**: ハードウェアからオーディオサンプルを取得し、Ring Bufferへ流し込む。（※現在実装済）
    
2. **DSP Worker Thread**: Ring Bufferからサンプルを取り出し、FFTやLUFS計算を行う。（※**現在あなたが実装すべき最優先課題**）
    
3. **UI / Render Thread**: DSPスレッドが計算した結果（Magnitude配列など）を受け取り、`egui` で60FPS描画を行う。
    

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

## 6. 現在の課題と、あなたの最初のタスク (Phase 4)

あなたが最初に着手すべき具体的なタスクは、「重いFFT処理をUIスレッドから分離すること」です。

**【現状の課題（`src/main.rs`）】**

現在、`AnalyzerApp` の `update` メソッド（UIの描画ループ）の中で `process_dsp()` と `perform_fft()` が呼ばれています。また、`perform_fft` の中で毎フレーム `collect()` によるメモリアロケーションが発生しています。

**【あなたのMission】**

1. **DSPスレッドの分離**: `std::thread::spawn` 等を用いて専用のDSPワーカースレッドを作成し、UIの `update` とは非同期に `process_dsp()` をループ実行させるアーキテクチャにリファクタリングしてください。
    
2. **計算結果の受け渡し**: DSPスレッドで計算済みの `magnitude_data` を、UIスレッドへ安全に渡すための2つ目の Ring Buffer（または `crossbeam-channel` や `Arc<RwLock>` ※可能ならLock-freeを推奨）を実装してください。
    
3. **アロケーションの排除**: `perform_fft` 内の `collect()` を削除し、事前確保したスクラッチバッファ（使い回しの `Vec` または配列）の中身を上書きする処理に変更してください。
    

これらの修正が完了すれば、どんなにFFTサイズを大きくしてもUIがカクつかず、オーディオ処理も途切れない、真の「プロ仕様」の基盤が完成します。Happy Hacking!