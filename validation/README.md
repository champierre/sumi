# 実世界 PDF による自動検証

「どんな PDF なら安全に sumi へ渡せるか」を、実物の PDF を大量に変換して確かめます。

```bash
python3 validation/fetch_corpus.py --count 1000   # コーパスを集める
cargo build --release
python3 validation/validate.py --mode grayscale   # 検証
python3 validation/validate.py --mode monochrome
```

結果は `validation/results-<mode>.json` に出ます。コーパスと結果は git 管理外です。

## コーパス

Wikimedia Commons から 20 の検索軸（請求書、報告書、プレゼン、地図、論文など）で均等に集め、
Chrome で生成した PDF と `fixtures/` の既知 PDF を加えています。すべてパブリックドメインか CC0 です。

- `mime` が `application/pdf` のものだけを採り、`%PDF-` で始まることも確認します（djvu や HTML が混ざるため）
- 1KB 未満と 20MB 超は除きます
- Wikimedia は並列に叩くと 429 を返すので、逐次で 0.4 秒ずつ空けて取得します

## 4 つの指標

### 変換成功率

CLI の終了コードで判定します（0 成功 / 3 不正な PDF / 4 未対応・暗号化 / 5 入出力 / 6 時間切れ）。

### 色残り

出力を 150dpi で描画し、RGB の最大と最小の差が 3 を超える画素を数えます。
`crates/sumi-core/tests/fixtures.rs` と同じ基準です。

### 表示崩れ

**輝度差では測りません。** sumi は色を変えるのが仕事なので、輝度で比べると
CMYK の文書が軒並み引っかかります（README の制限事項にある「CMYK は ICC 管理より明るくなる」ぶん。
実測で最大 50/255 の差が出て、崩れと区別できませんでした）。

代わりに「インクのある場所」（輝度 200 未満の画素）が動いていないかで見ます。
色が変わってもインクの位置は動かないので、文字や線が消えたり塗り潰されたりした場合だけ出ます。
さらに方向を分けて記録します。

- `ink_gained_ratio`: インクが**増えた**割合。紙が黒くなる、図形が塗り潰されるなどの異常。**これが本当の崩れ**
- `ink_lost_ratio`: インクが**消えた**割合。monochrome で薄い色が白に飛ぶのは仕様どおりなので、参考値として出します

### テキスト保持

`pdftotext` の抽出結果が変換前後でバイト単位で一致するかを見ます。

## 既知の「異常ではない」パターン

検証で引っかかっても仕様どおりのものがあります。

| 現象 | 原因 |
|---|---|
| 色残りが大きい | JPEG 2000（JPXDecode）画像。README の未対応リストにあり、警告も出ます |
| monochrome でインクが大きく消える | 閾値処理で薄い色が白に飛ぶ（仕様） |
| monochrome で背景が真っ黒になる | 中間グレーの背景が閾値 0.5 を下回る（仕様）。インク被覆は変わりません |
| CMYK 文書が元より明るい | CMYK の簡易式（README の制限事項） |
