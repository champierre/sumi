# Sumi

PDF の色をグレースケールまたは白黒 2 値に変換する Rust 製ライブラリ兼 CLI です。
ページを画像化せず、色指定だけを書き換えるので、テキスト検索・コピー・フォント・ベクター品質はそのまま残ります。
Ghostscript などの AGPL 製品には依存していません。

仕様は [sumi_spec.md](sumi_spec.md) を参照してください。仕様から変えた点は「[仕様との差分](#仕様との差分)」にまとめています。

## 変換例

Wikimedia Commons で公開されているパブリックドメインの PDF を変換した例です。画像は変換前後の PDF を poppler でレンダリングしたもので、インフォグラフィックは上部だけを切り出しています。

### インフォグラフィック

| 元の PDF | グレースケール | モノクロ |
|---|---|---|
| <img src="docs/images/samples/equal-pay-original.png" width="260" alt="元のインフォグラフィック。オレンジと青緑で色分けされている"> | <img src="docs/images/samples/equal-pay-grayscale.png" width="260" alt="グレースケールに変換したインフォグラフィック"> | <img src="docs/images/samples/equal-pay-monochrome.png" width="260" alt="モノクロに変換したインフォグラフィック。文字とグラフが黒と白で残っている"> |

```bash
sumi samples/equal-pay.pdf -o samples/equal-pay-grayscale.pdf
sumi samples/equal-pay.pdf -o samples/equal-pay-monochrome.pdf --mode monochrome
```

モノクロでも、見出し、数字、グラフはそのまま読めます。変換前後の PDF は [samples/](samples/) にあります。

出典: [Equal Pay Infographic](https://commons.wikimedia.org/wiki/File:Equal_Pay_Infographic.pdf)（U.S. Department of Labor、パブリックドメイン）

### 地図

| 元の PDF | グレースケール | モノクロ |
|---|---|---|
| <img src="docs/images/samples/europe-original.png" width="260" alt="国ごとに淡い色で塗り分けられたヨーロッパの地図"> | <img src="docs/images/samples/europe-grayscale.png" width="260" alt="グレースケールに変換した地図。国ごとの濃淡の違いが残っている"> | <img src="docs/images/samples/europe-monochrome.png" width="260" alt="モノクロに変換した地図。塗り分けと海岸線が消え、地名だけが残っている"> |

グレースケールでは、国ごとの濃淡の違いが残ります。モノクロでは淡い色がすべて白になり、塗り分けも海岸線も消えて地名だけが残ります。色の違いで情報を伝える資料は、グレースケールで変換してください。

出典: [Political map of Europe](https://commons.wikimedia.org/wiki/File:Political_map_of_Europe.pdf)（CIA World Factbook、パブリックドメイン）

## Ghostscript と mutool との比較

同じ PDF をグレースケールに変換し、Ghostscript および MuPDF の `mutool recolor` と、実行時間、メモリ使用量、出力を比べました。Ghostscript とは Mac（Apple M1 Pro）と Linux（AMD Ryzen 7 6800H）の 2 台で、mutool とは Linux の 1 台だけで計測しています。表の入力名のリンクから、計測に使った PDF を開けます。

`mutool recolor` は sumi と同じく、ページを画像化せずに色指定だけを書き換えます。Ghostscript のように描き直さないので、比較の相手としては sumi にいちばん近いツールです。ただし MuPDF も Ghostscript と同じ Artifex 製で、ライセンスは AGPL-3.0（または商用ライセンス）です。sumi が MIT なのは、この点が理由です。

### 実行時間

#### Mac（Apple M1 Pro、Ghostscript 10.05.1）

| 入力 | ページ | sumi | Ghostscript | 速度比 |
|---|---:|---:|---:|---:|
| [請求書](fixtures/chrome_invoice.pdf)（Chrome、0.34 MB） | 2 | 20 ms | 150 ms | 7.5 倍 |
| [請求書 50 ページ](bench/invoice-50pages.pdf)（Chrome、1.0 MB） | 50 | 59 ms | 2,441 ms | 41.2 倍 |
| [インフォグラフィック](https://upload.wikimedia.org/wikipedia/commons/f/f8/Equal_Pay_Infographic.pdf)（Adobe、0.30 MB） | 1 | 32 ms | 175 ms | 5.5 倍 |
| [NASA ファクトシート](https://upload.wikimedia.org/wikipedia/commons/7/79/0080_SLS_Fact_Sheet_10162019_PRINT_FINAL_%28656622902519%29.pdf)（Acrobat Distiller、0.30 MB） | 2 | 55 ms | 794 ms | 14.4 倍 |
| [ポスター](https://upload.wikimedia.org/wikipedia/commons/9/91/Best_Case_Scenarios_for_Copyright_-_poster.pdf)（cairo、5.9 MB） | 1 | 511 ms | 1,201 ms | 2.4 倍 |
| [地図](https://upload.wikimedia.org/wikipedia/commons/1/12/Political_map_of_Europe.pdf)（Aspose、6.7 MB） | 1 | 1,371 ms | 3,181 ms | 2.3 倍 |

#### Linux（AMD Ryzen 7 6800H、Ghostscript 10.07.1、mutool 1.28.0）

| 入力 | ページ | sumi | Ghostscript | mutool | Ghostscript 比 | mutool 比 |
|---|---:|---:|---:|---:|---:|---:|
| [請求書](fixtures/chrome_invoice.pdf)（Chrome、0.34 MB） | 2 | 21 ms | 150 ms | 194 ms | 7.2 倍 | 9.3 倍 |
| [請求書 50 ページ](bench/invoice-50pages.pdf)（Chrome、1.0 MB） | 50 | 62 ms | 2,574 ms | 2,285 ms | 41.2 倍 | 36.6 倍 |
| [インフォグラフィック](https://upload.wikimedia.org/wikipedia/commons/f/f8/Equal_Pay_Infographic.pdf)（Adobe、0.30 MB） | 1 | 36 ms | 154 ms | 57 ms | 4.3 倍 | 1.6 倍 |
| [NASA ファクトシート](https://upload.wikimedia.org/wikipedia/commons/7/79/0080_SLS_Fact_Sheet_10162019_PRINT_FINAL_%28656622902519%29.pdf)（Acrobat Distiller、0.30 MB） | 2 | 59 ms | 621 ms | 92 ms | 10.6 倍 | 1.6 倍 |
| [ポスター](https://upload.wikimedia.org/wikipedia/commons/9/91/Best_Case_Scenarios_for_Copyright_-_poster.pdf)（cairo、5.9 MB） | 1 | 596 ms | 1,192 ms | 886 ms | 2.0 倍 | 1.5 倍 |
| [地図](https://upload.wikimedia.org/wikipedia/commons/1/12/Political_map_of_Europe.pdf)（Aspose、6.7 MB） | 1 | 1,585 ms | 3,320 ms | 1,914 ms | 2.1 倍 | 1.2 倍 |

10 回実行した中央値です。「Ghostscript 比」「mutool 比」は、sumi が何倍速かったかです。

- Ghostscript に対しては、どちらの環境でも 6 件すべてで sumi のほうが速く、差は Mac で 2.3〜41.2 倍、Linux で 2.0〜41.2 倍でした。Ghostscript は PDF を解釈して描き直しますが、sumi は色の命令だけを書き換えるので、ページ数の多い帳票ほど差が開きます。
- mutool に対しても 6 件すべてで sumi のほうが速く、差は 1.2〜36.6 倍でした。mutool も色指定を書き換える方式なので、Ghostscript ほどの差はつきません。1 ページの大きな PDF（ポスター、地図）では 1.2〜1.5 倍と僅差です。
- 差がいちばん開くのは、ページ数の多い帳票です。同じ請求書の 2 ページ版と 50 ページ版（内容を 25 回繰り返したもの）を比べると、mutool は 194 ms から 2,285 ms へ 11.8 倍、Ghostscript は 150 ms から 2,574 ms へ 17.2 倍になりましたが、sumi は 21 ms から 62 ms へ 3.0 倍でした。

### メモリ使用量

#### Mac（Apple M1 Pro）

| 入力 | sumi | Ghostscript | sumi / Ghostscript |
|---|---:|---:|---:|
| [請求書](fixtures/chrome_invoice.pdf)（0.34 MB） | 9.9 MB | 30.7 MB | 32% |
| [請求書 50 ページ](bench/invoice-50pages.pdf)（1.0 MB） | 20.6 MB | 86.0 MB | 24% |
| [インフォグラフィック](https://upload.wikimedia.org/wikipedia/commons/f/f8/Equal_Pay_Infographic.pdf)（0.30 MB） | 6.2 MB | 28.0 MB | 22% |
| [NASA ファクトシート](https://upload.wikimedia.org/wikipedia/commons/7/79/0080_SLS_Fact_Sheet_10162019_PRINT_FINAL_%28656622902519%29.pdf)（0.30 MB） | 8.1 MB | 43.0 MB | 19% |
| [ポスター](https://upload.wikimedia.org/wikipedia/commons/9/91/Best_Case_Scenarios_for_Copyright_-_poster.pdf)（5.9 MB） | 34.2 MB | 37.7 MB | 91% |
| [地図](https://upload.wikimedia.org/wikipedia/commons/1/12/Political_map_of_Europe.pdf)（6.7 MB） | 85.8 MB | 32.5 MB | 264% |

#### Linux（AMD Ryzen 7 6800H）

| 入力 | sumi | Ghostscript | mutool |
|---|---:|---:|---:|
| [請求書](fixtures/chrome_invoice.pdf)（0.34 MB） | 10.5 MB | 31.6 MB | 44.6 MB |
| [請求書 50 ページ](bench/invoice-50pages.pdf)（1.0 MB） | 20.2 MB | 78.8 MB | 462.5 MB |
| [インフォグラフィック](https://upload.wikimedia.org/wikipedia/commons/f/f8/Equal_Pay_Infographic.pdf)（0.30 MB） | 6.8 MB | 29.0 MB | 21.3 MB |
| [NASA ファクトシート](https://upload.wikimedia.org/wikipedia/commons/7/79/0080_SLS_Fact_Sheet_10162019_PRINT_FINAL_%28656622902519%29.pdf)（0.30 MB） | 8.6 MB | 35.4 MB | 31.1 MB |
| [ポスター](https://upload.wikimedia.org/wikipedia/commons/9/91/Best_Case_Scenarios_for_Copyright_-_poster.pdf)（5.9 MB） | 30.8 MB | 36.6 MB | 63.1 MB |
| [地図](https://upload.wikimedia.org/wikipedia/commons/1/12/Political_map_of_Europe.pdf)（6.7 MB） | 72.7 MB | 32.8 MB | 87.8 MB |

プロセスの最大常駐メモリ（maximum resident set size）の、10 回実行した中央値です。

- Ghostscript より少なかったのは、どちらの環境でも 6 件中 5 件で、Ghostscript の 19〜91%（Mac）、23〜84%（Linux）で済みました。1 MB 前後までの PDF では 19〜32%（Mac）、23〜33%（Linux）です。
- mutool より少なかったのは 6 件すべてで、mutool の 4〜83% でした。
- sumi は PDF 全体をメモリに読み込んで変換するので、ファイルが大きいほどメモリを多く使います。6.7 MB の地図では、Ghostscript より多く使いました（Mac で 85.8 MB と 32.5 MB、Linux で 72.7 MB と 32.8 MB）。
- Ghostscript は 1〜2 ページの PDF では 28〜43 MB（Mac）、29〜35 MB（Linux）でしたが、50 ページの請求書では 86.0 MB（Mac）、78.8 MB（Linux）を使いました。
- mutool は 50 ページの請求書で 462.5 MB を使い、6 件の中で群を抜いて多くなりました。同じ内容の 2 ページ版が 44.6 MB なので、ページ数が増えるとメモリも大きく増えるようです。sumi は同じ PDF で 20.2 MB でした。

### 出力

| 入力 | 元の PDF | sumi | Ghostscript 10.05.1（Mac） | Ghostscript 10.07.1（Linux） | mutool 1.28.0（Linux） |
|---|---:|---:|---:|---:|---:|
| [請求書](fixtures/chrome_invoice.pdf) | 0.34 MB | 0.31 MB | 0.18 MB | 0.18 MB | 0.25 MB |
| [請求書 50 ページ](bench/invoice-50pages.pdf) | 1.03 MB | 0.98 MB | 1.13 MB | 1.12 MB | 0.88 MB |
| [インフォグラフィック](https://upload.wikimedia.org/wikipedia/commons/f/f8/Equal_Pay_Infographic.pdf) | 0.30 MB | 0.30 MB | 0.26 MB | 0.26 MB | 0.28 MB |
| [NASA ファクトシート](https://upload.wikimedia.org/wikipedia/commons/7/79/0080_SLS_Fact_Sheet_10162019_PRINT_FINAL_%28656622902519%29.pdf) | 0.30 MB | 0.84 MB | 0.51 MB | 0.24 MB | 0.84 MB |
| [ポスター](https://upload.wikimedia.org/wikipedia/commons/9/91/Best_Case_Scenarios_for_Copyright_-_poster.pdf) | 5.90 MB | 5.90 MB | 4.51 MB | 4.65 MB | 5.77 MB |
| [地図](https://upload.wikimedia.org/wikipedia/commons/1/12/Political_map_of_Europe.pdf) | 6.70 MB | 7.11 MB | 7.20 MB | 7.20 MB | 7.06 MB |

sumi の出力は、Mac と Linux で同じサイズでした。

- **出力サイズ（Ghostscript）**: 写真を含む NASA ファクトシートやポスターは、Ghostscript のほうが小さくなりました。sumi は JPEG 画像を可逆圧縮（Flate）で保存し直し、Ghostscript は画像を JPEG のまま再圧縮するためです。NASA ファクトシートは、Linux の Ghostscript 10.07.1 では 0.24 MB と、Mac の 10.05.1（0.51 MB）の半分以下になりました。
- **出力サイズ（mutool）**: mutool は 6 件すべてで sumi より小さく、差は 0.5〜20.1% でした。ただし mutool も JPEG 画像を可逆圧縮で保存し直すので、写真の多い PDF では同じように大きくなります（NASA ファクトシートは sumi が 843,510 バイト、mutool が 839,491 バイトで、どちらも元の 0.30 MB から 0.84 MB に増えました）。差が大きいのは、写真を含まない請求書（20.1%）や 50 ページの請求書（10.0%）です。写真の多い PDF を小さくしたいなら、画像を JPEG のまま再圧縮する Ghostscript のほうが向いています。
- **見た目**: 出力をレンダリングして比べたところ（Mac は sumi と Ghostscript、Linux はそれに mutool を加えた 3 つ）、見た目はほぼ同じで、どれにも色は残っていませんでした。sumi と mutool の描画結果の差は、sumi と Ghostscript の差と同程度で、文字の縁のアンチエイリアスがほとんどです。
- **テキスト**: sumi の出力から抽出したテキストは、どちらの環境でも 6 件すべてで元の PDF と完全に一致しました。mutool も 6 件すべてで完全に一致しています。Ghostscript の出力では、請求書の文字の抽出順が変わり、「発行日」が一続きの文字列として見つからなくなりました（文字自体の欠落はありません。Mac、Linux とも）。Mac の Ghostscript 10.05.1 では、NASA ファクトシートの合字「fi」「fl」が「Þ」「ß」として抽出され、「first」で検索できなくなりました。Linux の 10.07.1 ではこの問題は起きず、段落 1 つの抽出順が変わっただけでした。

### 比較方法

- **環境**: 次の 2 台で計測しました。sumi はどちらも 0.1.0（`cargo build --release`）です。Mac は 2026 年 9 月 11 日、Linux は 2026 年 9 月 12 日（mutool を加えて取り直したもの）です。

  | | Mac | Linux |
  |---|---|---|
  | マシン | Apple M1 Pro（メモリ 16 GB） | GEEKOM A6、AMD Ryzen 7 6800H（メモリ 32 GB） |
  | OS | macOS 26.5.2 | Omarchy 4.0.1（Arch Linux ベース、Linux 7.1.9） |
  | Ghostscript | 10.05.1（Homebrew） | 10.07.1（Arch Linux のパッケージ） |
  | mutool | 計測していない | 1.28.0（Arch Linux の mupdf-tools） |

- **実行方法**: アプリケーションから呼び出すのと同じく、どちらもコマンドとして実行し、プロセスの起動時間も含めて計測しました。PDF ごとに 1 回ウォームアップしてから 10 回実行しています。時間とメモリは `/usr/bin/time` で取得しました（macOS は `-l`、Linux は GNU time の `-v`）。
- **コマンド**:

  ```bash
  sumi input.pdf -o output.pdf --overwrite

  gs -q -dNOPAUSE -dBATCH -dSAFER -sDEVICE=pdfwrite \
     -sColorConversionStrategy=Gray -dProcessColorModel=/DeviceGray \
     -o output.pdf input.pdf

  mutool recolor -c gray -o output.pdf input.pdf
  ```

- **出力の確認**: poppler の `pdftoppm` でレンダリングして色の付いたピクセルがないこと、`pdftotext` で抽出したテキストが元の PDF と一致するかを調べました。
- **比べていないもの**: Ghostscript にも `mutool recolor` にも、ベクターのまま白黒 2 値にする機能がないため（`mutool recolor -c` は gray、rgb、cmyk のみ）、比べたのはグレースケール変換だけです。mutool は Linux の 1 台でしか計測していないので、Mac の表には入っていません。
- **入力**:

  | 入力 | PDF | 出典 |
  |---|---|---|
  | 請求書 | [chrome_invoice.pdf](fixtures/chrome_invoice.pdf) | [fixtures/src/invoice.html](fixtures/src/invoice.html) を Chrome で PDF にしたもの |
  | 請求書 50 ページ | [invoice-50pages.pdf](bench/invoice-50pages.pdf) | `fixtures/src/invoice.html` を 25 回繰り返して Chrome で PDF にしたもの |
  | インフォグラフィック | [Equal_Pay_Infographic.pdf](https://upload.wikimedia.org/wikipedia/commons/f/f8/Equal_Pay_Infographic.pdf) | [Equal Pay Infographic](https://commons.wikimedia.org/wiki/File:Equal_Pay_Infographic.pdf)（パブリックドメイン） |
  | NASA ファクトシート | [0080_SLS_Fact_Sheet_…pdf](https://upload.wikimedia.org/wikipedia/commons/7/79/0080_SLS_Fact_Sheet_10162019_PRINT_FINAL_%28656622902519%29.pdf) | [SLS Fact Sheet](https://commons.wikimedia.org/wiki/File:0080_SLS_Fact_Sheet_10162019_PRINT_FINAL_(656622902519).pdf)（パブリックドメイン） |
  | ポスター | [Best_Case_Scenarios_for_Copyright_-_poster.pdf](https://upload.wikimedia.org/wikipedia/commons/9/91/Best_Case_Scenarios_for_Copyright_-_poster.pdf) | [Best Case Scenarios for Copyright - poster](https://commons.wikimedia.org/wiki/File:Best_Case_Scenarios_for_Copyright_-_poster.pdf)（CC0） |
  | 地図 | [Political_map_of_Europe.pdf](https://upload.wikimedia.org/wikipedia/commons/1/12/Political_map_of_Europe.pdf) | [Political map of Europe](https://commons.wikimedia.org/wiki/File:Political_map_of_Europe.pdf)（パブリックドメイン） |

- **再現手順**:

  ```bash
  cargo build --release
  python3 bench/fetch.py        # 入力 PDF を bench/inputs に用意（Wikimedia Commons からダウンロード）
  python3 bench/bench.py        # 計測。結果は bench/results-macos.json または bench/results-linux.json
  ```

  `bench.py` は `gs` と `mutool` を PATH から探し、入っていないものは飛ばします。別の場所にあるときは `--gs`、`--mutool` でパスを渡してください。

  50 ページの請求書は、計測に使ったものを `bench/invoice-50pages.pdf` に置いています。Mac と Linux のどちらも、このファイルで計測しました。作り直すときは `python3 bench/make_batch.py` を実行してください（Google Chrome が必要）。

2 台での計測なので、数値は環境によって変わります。特に小さな PDF では、プロセスの起動時間が大きな割合を占めます。Ghostscript と mutool の結果は、バージョンやオプションによっても変わります。

## インストール

[GitHub Releases](https://github.com/champierre/sumi/releases/latest) からビルド済みの CLI をダウンロードできます。

| OS | ファイル |
|---|---|
| macOS（Apple Silicon） | `sumi-aarch64-apple-darwin.tar.gz` |
| macOS（Intel） | `sumi-x86_64-apple-darwin.tar.gz` |
| Windows（x64） | `sumi-x86_64-pc-windows-msvc.zip` |
| Linux（x86_64） | `sumi-x86_64-unknown-linux-musl.tar.gz` |
| Linux（arm64） | `sumi-aarch64-unknown-linux-musl.tar.gz` |

```bash
# 例: macOS（Apple Silicon）
curl -L https://github.com/champierre/sumi/releases/latest/download/sumi-aarch64-apple-darwin.tar.gz | tar xz
sudo mv sumi-aarch64-apple-darwin/sumi /usr/local/bin/
```

Linux 版は静的リンク（musl）なので、ディストリビューションを問わず動きます。ブラウザでダウンロードした macOS 版がブロックされる場合は、`xattr -d com.apple.quarantine sumi` を実行してください。

### ソースからビルド

```bash
cargo install --git https://github.com/champierre/sumi sumi-cli
# または
cargo build --release   # => target/release/sumi
```

Rust 1.88 以降が必要です（edition 2024）。

## CLI

```bash
sumi input.pdf -o output.pdf
sumi input.pdf -o output.pdf --mode monochrome --threshold 0.5
cat input.pdf | sumi - -o - > output.pdf
```

| オプション | 説明 |
|---|---|
| `-o, --output <FILE>` | 出力先。`-` で標準出力 |
| `--mode grayscale\|monochrome` | 変換方式（既定: `grayscale`） |
| `--threshold <0.0-1.0>` | モノクロ時の閾値（既定: `0.5`）。この値未満の濃さは黒になる |
| `--dither` | モノクロ時、画像を閾値ではなく誤差拡散（Floyd–Steinberg）で 2 値化する |
| `--strict` | 変換できない箇所があれば、元の色のまま出力せずにエラーにする |
| `--overwrite` | 出力ファイルが既にあれば上書きする |
| `--timeout <SECONDS>` | 指定秒数を超えたら中断する |
| `-v, --verbose` | 変換した件数を表示する |

変換できなかった箇所は、元の色のまま残して `sumi: warning: ...` を標準エラーに出します（`--strict` を付けるとエラー終了）。
出力はいったん一時ファイルに書いてから rename するので、失敗時に壊れたファイルが残りません。

### 終了コード

| コード | 意味 |
|---|---|
| 0 | 成功 |
| 1 | その他のエラー（リソース上限超過など） |
| 2 | 引数が不正（出力ファイルが既に存在する、入出力が同じファイル、など） |
| 3 | PDF として読めない |
| 4 | 未対応の PDF（暗号化 PDF、または `--strict` で変換できない箇所があった） |
| 5 | 入出力エラー |
| 6 | タイムアウト |

## Rust API

```rust
use sumi_core::{convert, convert_bytes, ConvertOptions, Mode};

let mut options = ConvertOptions::default();
options.mode = Mode::Monochrome;
options.threshold = 0.6;
let report = convert("input.pdf", "output.pdf", &options)?;

// メモリ上で変換する
let converted = convert_bytes(&std::fs::read("input.pdf")?, &ConvertOptions::grayscale())?;
println!("{} images converted, {} warnings", converted.report.images, converted.report.warnings.len());

// 簡易 API
sumi_core::grayscale("input.pdf", "output.pdf")?;
```

`ConvertOptions` と `Report` は `#[non_exhaustive]` です。将来フィールドを追加しても破壊的変更にならないよう、`ConvertOptions::default()` などで作ってからフィールドを変更してください。

## Rails からの利用

```ruby
require "open3"

class PdfGrayscaleConverter
  class ConversionError < StandardError; end

  SUMI = ENV.fetch("SUMI_PATH", "sumi")

  def self.call(input_path:, output_path:)
    # Tempfile のパスは作成済みなので --overwrite が必要
    _stdout, stderr, status = Open3.capture3(
      SUMI, input_path.to_s, "-o", output_path.to_s, "--overwrite", "--timeout", "30"
    )
    raise ConversionError, stderr unless status.success?

    output_path
  end
end
```

ファイルを介さずに、`Open3.capture3(SUMI, "-", "-o", "-", stdin_data: pdf_bytes, binmode: true)` のように標準入出力で渡すこともできます。

## 変換対象

| 対象 | 処理 |
|---|---|
| コンテンツストリームの `rg` `RG` `k` `K` `g` `G` | `g` / `G` に置き換え |
| `cs` `CS` `sc` `SC` `scn` `SCN` | 色空間を追跡し（`q`/`Q` の退避・復元を含む）、`/DeviceGray` に置き換え |
| 色空間 | DeviceGray/RGB/CMYK、CalGray/CalRGB、Lab、ICCBased（N=1/3/4）、Indexed、Separation、DeviceN（Type 0/2/3/4 関数を評価）、Pattern（下地色あり） |
| Form XObject、タイリングパターン、Type 3 フォントのグリフ | 再帰的に変換 |
| 画像 XObject | Indexed はパレットだけをグレーに差し替え（画素データは維持）。その他は 8bit グレー（モノクロ時は 1bit）で Flate 圧縮し直す。FlateDecode（Predictor 付きを含む）、LZW、ASCII85、ASCIIHex、RunLength、DCTDecode（JPEG）に対応。カラーキーの `/Mask` は SMask に変換 |
| インライン画像（`BI`…`EI`） | 同上 |
| シェーディング（グラデーション） | 関数を 256 点（関数型シェーディングは 33×33 点）でサンプリングし、グレーのサンプル関数に置き換え |
| 透明グループの `/CS` | `/DeviceGray` に変更 |
| 注釈 | 外観ストリーム（`/AP`）、`/C` `/IC`、`/MK` の `/BG` `/BC`、`/DA` |
| フォームフィールド | AcroForm と各フィールドの `/DA` |

テキスト描画命令、フォント、パス、文字列、コメントなど色以外のバイト列は、元のままコピーします。

### 変換式

PDF 仕様（ISO 32000-1 10.3）が示す DeviceRGB/DeviceCMYK → DeviceGray の式を使っています。Acrobat や Ghostscript（ICC を使わない場合）と同じ式です。

```text
Gray = 0.30 × R + 0.59 × G + 0.11 × B
Gray = 1 − min(1, 0.30 × C + 0.59 × M + 0.11 × Y + K)
```

モノクロでは `Gray < threshold` を黒、それ以外を白にします。

## 制限事項

- **CMYK の見た目**: CMYK には上記の簡易式を使うため、ICC で色管理した表示よりシアンなどが明るくなります（例: シアン 100% は 0.70。poppler の表示をグレー化すると約 0.5）。
- **モノクロ化による情報の欠落**: 閾値で 2 値化するため、黄色など明るい色の文字は白い紙の上で消えます。薄い背景色も消えます。白抜き文字が背景と同化することもあります。
- **未対応で元の色のまま残るもの**（警告を出力）: 関数を持たないメッシュシェーディング（Type 4〜7）、JPEG 2000 / JBIG2 / CCITT で符号化されたカラー画像、複数のフィルタにパラメータがあるストリーム。
- **暗号化 PDF** は変換しません（権限設定だけの PDF も含む）。
- **壊れた PDF**: パーサ（lopdf）が読めないオブジェクトがページの描画に必要な場合はエラーにします。描画に関係しないオブジェクト（構造ツリーなど）であれば、警告を出したうえで取り除いて変換します。
- JPEG 画像は Flate で可逆圧縮し直すので、写真が多い PDF はサイズが増えることがあります。
- ソフトマスク（`/SMask` のグループ）の中身は、表示色に影響しないので変換しません。

## 仕様との差分

| 仕様 | 実装 | 理由 |
|---|---|---|
| PDF パーサ・xref・writer を自前実装 | [lopdf](https://crates.io/crates/lopdf)（MIT）を使用し、コンテンツストリームの字句解析と色変換だけを自前で実装 | incremental update、xref stream、object stream、壊れた xref の修復などを最初から扱えるため。xref stream と object stream は v0.3 予定だったが、この構成で最初から読める |
| 変換式に 0.2126 / 0.7152 / 0.0722 を使用 | 0.30 / 0.59 / 0.11 | PDF の色値はガンマ補正済みの値で、Rec.709 の係数はリニア値向けのため。PDF 仕様の式に合わせた |
| DeviceGray は変更しない | モノクロ時は `g` / `G` も 2 値化 | 仕様どおりだとモノクロに中間調が残るため |
| `SumiError` のバリアント | `InvalidPdf(String)`、`EncryptedPdf`、`Unsupported(Vec<String>)`、`InvalidOptions`、`LimitExceeded`、`Io(io::Error)`、`Internal` | エラー内容を保持するため。`UnsupportedPdfVersion` はヘッダのバージョンがあてにならないので廃止 |
| `convert(input, output, options)` | `convert(input, output, &options)`、`convert_bytes`、`monochrome` を追加 | Web サービスでメモリ上で変換できるように |
| CLI オプション | `--dither` `--strict` `--timeout`、標準入出力（`-`）、終了コード 6 を追加 | |
| 画像対応は v0.2 | v0.1 で対応 | 帳票の社印などが画像であることが多いため |

## 安全性

- 展開後のストリームサイズ（既定 256MB）、画像の画素数（既定 1 億）、フォームなどの入れ子の深さ（既定 32）に上限があります（`Limits`）。上限を超えるストリームはエラーになります。
- 変換中の panic は `SumiError::Internal` として返します。

## テスト

```bash
cargo test
```

- `crates/sumi-core/tests/convert.rs`: メモリ上で組み立てた PDF による各機能のテスト
- `crates/sumi-core/tests/fixtures.rs`: `fixtures/` の実物に近い PDF（Chrome、Quartz、Ghostscript の object stream 版、LibreOffice）で、ページ数・MediaBox・テキスト命令が変わらないこと、色演算子が残らないことを確認します。`pdftoppm`（poppler）がインストールされていれば、出力をレンダリングして色付きのピクセルがないことも確認します（poppler はテスト時に外部コマンドとして呼ぶだけです）。
- `crates/sumi-cli/tests/cli.rs`: 終了コード、上書き確認、標準入出力

fixture の再生成方法は [fixtures/README.md](fixtures/README.md) を参照してください。

## リリース

`v0.1.0` のように `Cargo.toml` のバージョンと同じタグを push すると、GitHub Actions（`.github/workflows/release.yml`）が各プラットフォーム向けの CLI をビルドし、GitHub Releases に公開します。

```bash
git tag v0.1.0
git push origin v0.1.0
```

## ライセンス

[MIT](LICENSE)。依存クレートはすべて MIT / Apache-2.0 / BSD / Zlib 系のライセンスです。
