# Sumi 仕様書

## 1. 概要

Sumi は、PDF ファイルをグレースケールまたはモノクロへ変換するための Rust 製ライブラリ兼 CLI ツールである。

主な用途は、Docurain などの帳票生成サービスから出力されたカラー PDF を、印刷用途向けに白黒化することを想定する。

Ghostscript などの AGPL ライセンス製品へ依存せず、商用 Web サービスから利用しやすい構成を目指す。

初期バージョンでは PDF 仕様全体への完全対応を目標とせず、一般的な帳票 PDF に必要な機能へ対応範囲を限定する。

---

## 2. 目的

Sumi の目的は以下の通り。

- PDF をグレースケールへ変換する
- 必要に応じて白黒 2 値へ変換する
- テキストやベクター要素を可能な限り維持する
- PDF 全体を画像化する方式を避ける
- Ghostscript に依存しない
- Rust ライブラリとして利用できる
- CLI として利用できる
- Rails など他言語のアプリケーションから外部コマンドとして利用できる
- 商用利用しやすいライセンスで公開できる構成にする

---

## 3. 想定ユースケース

### 3.1 Rails からの利用

Rails アプリケーションが Docurain から PDF を取得し、Sumi を使ってグレースケール PDF を生成する。

```text
Docurain
   ↓
カラー PDF
   ↓
Rails
   ↓
Sumi
   ↓
グレースケール PDF
   ↓
ユーザーへ返却
```

### 3.2 CLI からの利用

```bash
sumi input.pdf -o output.pdf
```

### 3.3 白黒 2 値化

```bash
sumi input.pdf -o output.pdf --mode monochrome
```

---

## 4. プロジェクト構成

Rust Workspace として以下の構成を想定する。

```text
sumi/
├── Cargo.toml
├── crates/
│   ├── sumi-core/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── parser.rs
│   │       ├── xref.rs
│   │       ├── object.rs
│   │       ├── stream.rs
│   │       ├── content.rs
│   │       ├── color.rs
│   │       ├── image.rs
│   │       └── writer.rs
│   └── sumi-cli/
│       └── src/
│           └── main.rs
├── tests/
└── fixtures/
```

### sumi-core

PDF の解析・変換・書き出しを担当する Rust ライブラリ。

### sumi-cli

`sumi-core` を利用する CLI アプリケーション。

---

## 5. CLI 仕様

### 5.1 基本形式

```bash
sumi <INPUT> -o <OUTPUT>
```

例:

```bash
sumi invoice.pdf -o invoice-gray.pdf
```

デフォルトではグレースケール変換を行う。

---

## 6. オプション

### `--mode`

変換方式を指定する。

```bash
sumi input.pdf -o output.pdf --mode grayscale
```

指定可能値:

```text
grayscale
monochrome
```

デフォルト:

```text
grayscale
```

### `--threshold`

モノクロ 2 値化時の閾値。

```bash
sumi input.pdf -o output.pdf   --mode monochrome   --threshold 0.5
```

範囲:

```text
0.0 - 1.0
```

デフォルト:

```text
0.5
```

### `--overwrite`

出力ファイルが既に存在する場合に上書きを許可する。

```bash
sumi input.pdf -o output.pdf --overwrite
```

### `--verbose`

詳細ログを表示する。

```bash
sumi input.pdf -o output.pdf --verbose
```

---

## 7. Rust API

基本 API:

```rust
use sumi_core::{convert, ConvertOptions, Mode};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = ConvertOptions {
        mode: Mode::Grayscale,
        threshold: 0.5,
    };

    convert("input.pdf", "output.pdf", options)?;

    Ok(())
}
```

簡易 API も提供する。

```rust
sumi_core::grayscale("input.pdf", "output.pdf")?;
```

---

## 8. グレースケール変換

RGB の基本変換式には相対輝度を使用する。

```text
Gray = 0.2126 × R
     + 0.7152 × G
     + 0.0722 × B
```

RGB 値は `0.0` から `1.0` とする。

例:

```text
1 0 0 rg
```

を、

```text
0.2126 g
```

へ変換する。

---

## 9. PDF カラー演算子

初期バージョンでは以下を処理対象とする。

### DeviceRGB

```text
rg
RG
```

`rg` は塗り色、`RG` は線色。

### DeviceGray

```text
g
G
```

すでにグレースケールの場合は基本的に変更しない。

### DeviceCMYK

```text
k
K
```

CMYK を RGB または直接 Gray へ変換する。

### ColorSpace

必要に応じて以下を解析する。

```text
cs
CS
sc
SC
scn
SCN
```

---

## 10. CMYK 変換

初期実装では簡易変換を利用できる。

```text
R = 1 - min(1, C + K)
G = 1 - min(1, M + K)
B = 1 - min(1, Y + K)
```

その後 RGB から Gray へ変換する。

より正確な色管理は将来バージョンで対応する。

---

## 11. モノクロ変換

`monochrome` モードでは Gray 値を閾値で 2 値化する。

```text
Gray < threshold → black
Gray >= threshold → white
```

例:

```text
threshold = 0.5
```

の場合、

```text
0.3 → 0.0
0.7 → 1.0
```

となる。

---

## 12. テキスト

テキストそのものは変更しない。

PDF のテキスト描画命令やフォント情報は維持し、色指定のみを書き換える。

そのため以下を維持することを目標とする。

- テキスト検索
- テキストコピー
- フォント
- 文字位置
- ベクター品質

---

## 13. ベクターオブジェクト

以下の描画命令自体は変更しない。

```text
m
l
c
v
y
h
re
S
s
f
F
f*
B
B*
```

色指定のみを Gray へ置き換える。

---

## 14. 画像

PDF 内部の画像は Image XObject として処理する。

初期対応候補:

- DeviceRGB
- DeviceGray
- DeviceCMYK
- FlateDecode
- DCTDecode (JPEG)

画像についてはピクセル単位で Gray へ変換する。

ただし画像処理はテキスト・ベクター変換より複雑なため、MVP では段階的に対応する。

---

## 15. MVP 対応範囲

Version 0.1 では以下を優先する。

### 対応

- PDF 1.4〜1.7
- 非暗号化 PDF
- 通常の indirect object
- xref table
- trailer
- content stream
- FlateDecode
- DeviceRGB
- DeviceGray
- DeviceCMYK
- `rg`
- `RG`
- `g`
- `G`
- `k`
- `K`
- テキスト
- ベクター図形
- 複数ページ PDF

### 将来対応

- xref stream
- object stream
- JPEG 画像変換
- PNG 相当の Flate 画像
- ICCBased
- Indexed ColorSpace
- Separation
- DeviceN
- Pattern
- transparency
- JPEG2000
- encrypted PDF
- linearized PDF

---

## 16. PDF Parser

Sumi は最低限の PDF Parser を独自実装する。

主な解析対象:

```text
PDF Header
Indirect Object
Dictionary
Array
Name
Number
String
Boolean
Null
Reference
Stream
xref
trailer
startxref
```

例:

```text
12 0 obj
<<
  /Length 123
  /Filter /FlateDecode
>>
stream
...
endstream
endobj
```

---

## 17. Stream 処理

Content Stream が圧縮されている場合は展開する。

初期対応:

```text
FlateDecode
```

処理:

```text
compressed stream
       ↓
decompress
       ↓
tokenize
       ↓
color conversion
       ↓
compress
       ↓
new stream
```

圧縮については Rust の既存 crate を利用してよい。

候補:

```text
flate2
```

ライセンスが商用利用可能であることを確認した上で採用する。

---

## 18. PDF Writer

変換後は新しい PDF として書き出す。

初期実装では、元ファイルを in-place 編集しない。

```text
input.pdf
   ↓
parse
   ↓
modify
   ↓
rebuild
   ↓
output.pdf
```

xref と trailer は再生成する。

---

## 19. エラー処理

想定エラー:

```rust
pub enum SumiError {
    InvalidPdf,
    UnsupportedPdfVersion,
    UnsupportedFilter,
    UnsupportedColorSpace,
    EncryptedPdf,
    ParseError,
    IoError,
}
```

CLI では適切な終了コードを返す。

例:

```text
0  success
1  generic error
2  invalid arguments
3  invalid PDF
4  unsupported PDF feature
5  I/O error
```

---

## 20. Rails からの利用

Rails では `Open3` を利用する。

```ruby
require "open3"

stdout, stderr, status = Open3.capture3(
  "sumi",
  input_path,
  "-o",
  output_path
)

unless status.success?
  raise "Sumi conversion failed: #{stderr}"
end
```

ユーザー入力を含むパスを shell string として組み立てず、引数を個別に渡す。

---

## 21. Rails Service Object 例

```ruby
class PdfGrayscaleConverter
  class ConversionError < StandardError; end

  def self.call(input_path:, output_path:)
    stdout, stderr, status = Open3.capture3(
      "sumi",
      input_path.to_s,
      "-o",
      output_path.to_s
    )

    unless status.success?
      raise ConversionError, stderr
    end

    output_path
  end
end
```

---

## 22. パフォーマンス目標

一般的な帳票 PDF を対象にする。

初期目標:

```text
10ページ以下:
1秒以内を目標

メモリ:
100MB以下を目標
```

ただし画像量などに依存するため、厳密な保証値とはしない。

---

## 23. セキュリティ

PDF は外部入力となる可能性があるため、以下を考慮する。

- 不正な object offset
- 循環参照
- 巨大 stream
- zip bomb
- 異常に深い object tree
- integer overflow
- malformed xref
- 異常な画像サイズ

Parser では panic を避け、可能な限り `Result` でエラーを返す。

---

## 24. 依存ライブラリ方針

依存ライブラリは最小限とする。

採用条件:

- MIT
- BSD
- Apache-2.0
- ISC

など、商用サービスで利用しやすいライセンスを原則とする。

AGPL / GPL 系ライブラリへの依存は避ける。

---

## 25. ライセンス

Sumi 本体のライセンス候補:

```text
MIT License
```

または、

```text
Apache License 2.0
```

Rust ecosystem で一般的な、

```text
MIT OR Apache-2.0
```

のデュアルライセンスも候補とする。

---

## 26. テスト

fixture PDF を用意し、自動テストを行う。

例:

```text
fixtures/
├── rgb_text.pdf
├── rgb_shapes.pdf
├── cmyk.pdf
├── multipage.pdf
├── compressed_stream.pdf
└── docurain_sample.pdf
```

検証項目:

- PDF が正常に開ける
- ページ数が変化しない
- ページサイズが変化しない
- テキスト内容が変化しない
- RGB 色指定が残っていない
- CMYK 色指定が残っていない
- Gray 指定へ変換されている

---

## 27. Docurain 対応

開発初期に Docurain が生成する実際の PDF を解析する。

調査項目:

- PDF Version
- xref table / xref stream
- object stream の有無
- Content Stream の Filter
- 使用 ColorSpace
- 画像フォーマット
- フォント形式
- transparency の使用
- ICC profile の使用

Docurain の出力仕様が限定的であれば、それに合わせて MVP の実装範囲をさらに絞る。

---

## 28. 開発ロードマップ

### Version 0.1

テキスト・ベクター主体の帳票 PDF をグレースケール化する。

対応:

```text
PDF parser
xref table
FlateDecode
DeviceRGB
DeviceCMYK
DeviceGray
content stream
PDF writer
CLI
```

### Version 0.2

画像対応。

```text
Image XObject
JPEG
Flate image
RGB → Gray
CMYK → Gray
```

### Version 0.3

PDF 対応範囲を拡張。

```text
xref stream
object stream
Indexed ColorSpace
ICCBased
```

### Version 1.0

一般的な帳票 PDF に対して安定して利用できる状態を目標とする。

---

## 29. 非目標

Sumi は以下を目標としない。

- PDF Viewer
- PDF Editor
- PDF Renderer
- OCR
- PDF/A 変換
- PostScript 変換
- 完全な PDF 仕様実装

あくまで、

> PDF の色をグレースケールまたは白黒へ変換する

ことに特化する。

---

## 30. 最終的な利用イメージ

```bash
sumi color.pdf -o gray.pdf
```

Rails:

```ruby
PdfGrayscaleConverter.call(
  input_path: "color.pdf",
  output_path: "gray.pdf"
)
```

Rust:

```rust
sumi_core::grayscale(
    "color.pdf",
    "gray.pdf"
)?;
```

Sumi は小さく、依存が少なく、商用利用しやすい PDF 白黒変換ツールを目指す。
