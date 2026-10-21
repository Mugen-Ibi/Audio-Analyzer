# Rust Audio Visualizer

Windowsで単独起動するRust製の音響観察アプリです。**対数周波数 × 帯域内の最大FFTビン振幅 × 時間の3Dウォーターフォール**を中心に、総合RMS・ピークと、同じ時間範囲のRMS履歴を一つの画面で確認できます。同じデータを2D表示にも切り替えられます。

## 起動と測定

```powershell
cargo run --release --locked
```

通常起動では入力を開始しません。「設定」で経路と解析条件を確認し、「測定を開始」を押すと収録を開始します。終了まで音声ファイルを保存せず、外部送信もしません。

- M2のASIO入力またはWASAPI共有の内蔵マイクなどを選択できます。明示した機器が不在ならエラーになり、別の入力へ置き換えません。自動選択時のみ、開始時にASIO→WASAPIへ切り替え、理由と適用経路を表示します。
- 要求レート・バッファと、実適用レート・ドライバーの報告バッファ・コールバック長を区別します。`None`はドライバーから取得不能です。対応形式の列挙は実機動作の保証ではありません。
- 青・緑は同一ストリームの異なる入力chです。内蔵モノラル入力は緑を無効にして利用できます。スペクトラムは各chの表示切替と重ね合わせに対応します。
- 高速条件は1024点・平均なし、精密条件は8192点・スペクトル電力の指数平均500msです。2048点、Hann/Rectangular、平均時定数も選択できます。RMS/Peakは窓内の非平均値です。窓の観測時間とビン間隔を表示します。ビン間隔は実効分解能や推定誤差ではありません。
- 履歴は初期60秒表示、最大600秒（60〜600秒を設定可能）の解析結果をメモリ保持します。各chの96対数周波数帯域の最大値とRMSを同じ時刻で記録します。履歴として生音声は保持しません。
- 「HOLD」で表示時点を固定し、履歴を調べられます。この間も入力は続きます。「入力停止」は実際の入力を停止し、過去の履歴は残します。カーソルは両周波数履歴・音量履歴で共通です。
- 入力・解析条件を変更した場合は表示停止を解除し、その理由を表示して新しい条件の結果を待ちます。旧条件の結果は履歴の区切りから確認できます。無効値・クリップ・入力欠落、表示キューの欠落、画面へ最新値を集約した数は別々に計数します。
- 欠落区間は空白、条件変更は履歴の区切りになります。設定変更時は旧平均・旧世代の結果を除外します。設定の「比較条件」→「条件変更の境界を記録」で、手動変更も平均リセットと境界として残せます。
- 「条件を保存」で `%APPDATA%\AudioAnalyzer\settings.json` に保存します。起動時に復元し、壊れた設定は理由を表示して入力停止のまま確認できます。保存済み機器の不在を表示します。

3Dはドラッグで回転、ホイールで拡大、ダブルクリックまたは「視点リセット」で初期視点へ戻ります。最新の断面は手前です。2Dは横が対数周波数、縦が時間で下が最新です。青・緑の入力を共通の固定レンジで比較でき、マイク2本を自動でL/Rとみなしません。狭い画面では表示を縮め、詳細を同じ画面でスクロールできます。「現在へ」でHOLDと過去への移動を解除できます。

履歴の曲線・2D・RMS履歴と、時刻／帯域スライダーは共通のカーソルで連動します。3Dの最大64断面、2Dの画素ごとに、時間区間内で全入力中の最大スペクトルピークを持つ**実際の一窓**を選んで描画します。元の10Hz履歴は保持し、時間幅を縮めたりカーソルで個別値を確認できます。粗い表示では弱いイベントが省略される場合があります。帯域の値は総合RMSや帯域積分値と異なり、FFTビンのない帯域は値なしです。解析下限は−120dBFSです。自動正規化は行いません。

「基準を保持」で現在/HOLD窓のスペクトルと条件を保持し、3D手前の点線と「断面 / 基準」のスペクトラムで重ね合わせます。明示したローカル保存／読込は設定ファイルの隣の `spectrum-reference.json` を使います。保存対象は解析結果のみです。解析条件の差は警告しますが、ゲイン・AGC・配置の同一性は手動で確認してください。視点と3D/2D選択は「条件を保存」で設定に含まれます。

設定は「入力」「解析・表示」「比較条件」「診断」の4タブです。変更は即時適用し、保存は次回起動用です。F5で開始／停止、HでHOLD、Lで現在へ、F1で設定、Escで設定を閉じます。文字入力中と設定を開いている間は測定用ショートカットを抑制します。

接続・停止は制御スレッドで実行します。起動途中の取消に対応しますが、ドライバー内部のブロック呼び出しを強制終了する保証はありません。

## 測定の意味

単位はデジタル最大値基準のdBFSです。片側スペクトラムの正弦波ピーク振幅を窓のcoherent gainで補正します。DC/Nyquist以外は2倍します。sample peakと窓内RMSを別表示し、正弦波RMSはピーク振幅より約3.01dB低くなります。スペクトラムの初期表示下限は−120dBFSで、設定から変更できます。

観測スペクトルには音源・スピーカー・部屋・位置・向き・マイク・IFの特性が含まれます。部屋や楽器だけの応答、校正済みのdB SPLとしては扱いません。内蔵スピーカー等で出せない帯域を補完しません。

AGC・ノイズ抑制・アナログゲインはCPALから信頼できる値を取得できず、**不明**と表示します。機器、ゲイン、位置、向き、フィルター設定をメモに記録し、同条件で相対比較してください。自動処理が有効・不明な入力では、厳密なレベルや減衰比較に制限があります。

コヒーレンス、既存のH1・位相は参考表示として残しています。2入力の信号が十分で、平均化が有効かつ8窓以上になるまで未確定です。8窓は統計的な信頼性の保証ではありません。低レベル帯域は描画しません。コヒーレンスは音質や部屋の点数ではありません。

同期比較は**同一デバイスの同一入力ストリーム**だけが対象です。内蔵マイクとM2の同時同期、別装置間の時計補正、IR、残響、精密な伝達測定、アプリからの信号生成は今回の範囲外です。WASAPIは共有モードで、排他・bit-perfect入力は保証しません。

## 音声機器なしで確認する

```powershell
cargo run --release --locked -- --demo
cargo run --release --locked -- --list-inputs
```

`--demo`は750/1500Hzの合成データを解析経路へ投入し、マイク・スピーカーを開きません。3秒ごとの振幅変化を観察できます。設定ファイルを書き換えません。`--list-inputs`は機器・対応形式の読み取り専用列挙です。ループバック端子が一覧にあっても、ループバック機能全体の検証済みを意味しません。

## ビルドと検証

最小化・リサイズ中の画面撮影で起きていた幅0の停止は、同じ0.30.0版のローカル `egui_glow` 修正で対処しています。共有Cargoキャッシュは変更しません。変更箇所と再現・回帰試験は [修正記録](docs/ZERO_FRAMEBUFFER_FIX.md) にあります。

Rust Edition 2024、Windows MSVC C++ Build Tools、LLVM/Clangが必要です。既定featureは `desktop + asio`。ASIOなしのWindows GUI:

```powershell
cargo build --release --locked --no-default-features --features desktop
```

ASIO SDKが不完全、または日本語を含むOUT_DIRで`asio-sys`が失敗する環境では、完全なSDKとビルド生成先をASCIIパスへ指定してください。共有キャッシュや他タスクのファイルを削除する必要はありません。

```powershell
$env:CPAL_ASIO_DIR = 'C:\path\to\ASIOSDK' # common/ と host/ を含む場所
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
$env:CARGO_TARGET_DIR = 'C:\path\to\ascii-target'
cargo build --release --locked
```

通常検証（マイク・出力を開きません）:

```powershell
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo test --release --locked --all-features
cargo test --locked --no-default-features
cargo test --locked --no-default-features --features desktop
cargo run --release --locked --no-default-features --example dsp_timing
```

実マイクを使う `live_audio` テストはignoreされています。**明示的な実機収録の許可を得た場合だけ**次を実行します。

```powershell
cargo test --locked --test live_audio -- --ignored --nocapture
```

今回のVisualizer設計・画面確認・検証は [AUDIO_VISUALIZER.md](docs/AUDIO_VISUALIZER.md) を参照してください。基礎実装の設計と受入対応は [IMPLEMENTATION_V03.md](docs/IMPLEMENTATION_V03.md)、以前の検証結果は [VALIDATION_V03.md](docs/VALIDATION_V03.md) にあります。以前の [ARCHITECTURE.md](ARCHITECTURE.md) と [REVIEW_PLAN.md](REVIEW_PLAN.md) は過去時点の記録です。

## ASIO-enabled release distribution

The ASIO-enabled release executable and combined corresponding source are distributed under GNU GPL version 3. Original project MIT and dependency/font notices are retained. See [distribution terms](LICENSE-DISTRIBUTION.md) and [GPLv3](COPYING.GPLv3). Use the release corresponding-source ZIP, including locked dependencies and ASIO build sources, for a complete offline build; GitHub automatic source archives do not include those additional build inputs.

With Rust/Cargo 1.99, MSVC/Windows SDK and LLVM/libclang installed, extract that ZIP and run:

```powershell
.\scripts\build-corresponding-source.ps1 -TargetDirectory C:\Build\AudioVisualizer-beta2
```

Choose an ASCII target directory for ASIO. This command builds only; it does not start audio input or output. Run the resulting executable with `--demo` for a synthetic demonstration.
