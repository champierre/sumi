# サンプル

README の「変換例」で使っている PDF です。

| ファイル | 内容 |
|---|---|
| `equal-pay.pdf` | 元の PDF |
| `equal-pay-grayscale.pdf` | `sumi equal-pay.pdf -o equal-pay-grayscale.pdf` の出力 |
| `equal-pay-monochrome.pdf` | `sumi equal-pay.pdf -o equal-pay-monochrome.pdf --mode monochrome` の出力 |

## 出典とライセンス

- `equal-pay.pdf`: [Equal Pay Infographic](https://commons.wikimedia.org/wiki/File:Equal_Pay_Infographic.pdf)
  - 作成者: U.S. Department of Labor
  - ライセンス: パブリックドメイン（米国連邦政府の著作物）
  - 入手元: Wikimedia Commons
- README の地図の例: [Political map of Europe](https://commons.wikimedia.org/wiki/File:Political_map_of_Europe.pdf)
  - 作成者: CIA World Factbook
  - ライセンス: パブリックドメイン
  - 元の PDF は 6.7MB あるため、リポジトリには含めていません。

変換後の PDF と `docs/images/samples/` の画像は、上記の PDF を sumi で変換したものです。

## 画像の作り直し方

```bash
pdftoppm -r 72 -f 1 -l 1 -png equal-pay-monochrome.pdf out
magick out-1.png -crop 612x1060+0+0 +repage ../docs/images/samples/equal-pay-monochrome.png
```
