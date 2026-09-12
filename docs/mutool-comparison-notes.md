# mutool recolor との比較 — 引き継ぎメモ

別の Mac で計測するための覚え書き。2026-09-12 時点。

## 何を確かめたいか

`mutool recolor` が **セグメンテーション違反で落ちる**のが、この環境固有かどうか。
1 台だけの結果では、Homebrew のビルド・CPU・OS 版と切り分けられない。

再現すればアーキテクチャ非依存の問題として Artifex に報告する。
再現しなければこの環境固有として扱いを変える。

## 計測した環境（比較の基準）

| 項目 | 値 |
|---|---|
| macOS | 14.8.7（Darwin 23.6.0） |
| CPU | Intel Core i5-8500B @ 3.00GHz |
| arch | x86_64 |
| mutool | 1.28.3（Homebrew の bottle、AGPL-3.0-or-later） |
| sumi | 0.1.0（`d863f72`） |

**別の CPU 種別（Apple Silicon）で試すと切り分けが進む。**

## 再現手順

```sh
brew install mupdf-tools
mutool -v          # 1.28.3 かどうか
```

リポジトリ同梱の fixture だけで再現する。コーパス（3.4GB、git 管理外）は不要。

```sh
cd ~/rust/sumi
for f in fixtures/chrome_invoice.pdf fixtures/gs_objstm_cmyk.pdf \
         fixtures/quartz_graphics.pdf fixtures/libreoffice_table.pdf; do
  rm -f /tmp/o.pdf
  mutool recolor -c gray -o /tmp/o.pdf "$f" >/dev/null 2>&1
  echo "$(basename "$f"): rc=$?  出力=$([ -f /tmp/o.pdf ] && echo あり || echo なし)"
done
```

この環境での結果:

| fixture | 生成器 | rc |
|---|---|---|
| chrome_invoice.pdf | Skia/PDF m152 | **139** |
| gs_objstm_cmyk.pdf | GPL Ghostscript 10.05.1 | **139** |
| quartz_graphics.pdf | macOS Quartz | **139** |
| libreoffice_table.pdf | LibreOffice 7.2 | 0 |

**`rc=139` は SIGSEGV（128+11）。** mutool はメッセージを出さずに落ちるので、
`echo $?` を見ないと成功と区別がつかない。出力ファイルもできない。

生成器が Chrome・Ghostscript・Quartz とばらばらなので、特定ツール固有ではなさそう。
同じ Canva 製でもクラッシュする 1 本と成功する 5 本があり、ファイル依存に見える。

## この環境での計測結果

### 完走率（コーパス 150 本を無作為抽出、60 秒上限）

| 結果 | 件数 | 割合 |
|---|---|---|
| 完走（5 秒以内） | 99 | 66.0% |
| 完走（5 秒超） | 28 | 18.7% |
| クラッシュ | 16 | 10.7% |
| 60 秒で未完了 | 7 | 4.7% |

出力ができないのが 23/150 = 15.3%。60 秒に達したものは 8〜19MB の PDF が並ぶ。

クラッシュした生成器: Adobe PDF Library（8.0 / 10.0.1 / 15.0 / 15.00 / 16.07 / 17.00）、
LibreOffice 7.3、Canva、PowerPoint 2010、Ghostscript 10.05.1。

### 速度・サイズ・テキスト保持（mutool が完走する 5 本、各 3 回の最小値）

| 入力 | sumi | mutool | 速度比 |
|---|---|---|---|
| 0.08 MB | 10 ms / 0.08 MB | 10 ms / 0.06 MB | 1.0x |
| 0.47 MB | 70 ms / 1.02 MB | 130 ms / 1.00 MB | 1.9x |
| 2.76 MB | 10 ms / 2.75 MB | 3,520 ms / 13.11 MB | **352x** |
| 6.07 MB | 8,080 ms / 34.72 MB | 9,890 ms / 34.49 MB | 1.2x |
| 9.94 MB | 60 ms / 9.94 MB | 6,310 ms / 19.49 MB | **105x** |

- **テキストは両者とも 5 本すべてで完全一致。** この指標では差がつかない。
- 速度差が大きい 2 本は、mutool の出力が 4.8 倍・2.0 倍に膨らんだ 2 本と一致する。
  画像を再エンコードしているためと見られる。
- **sumi に不利な行が 1 つある。** 6.07 MB の入力で sumi は 8,080 ms かかり、
  出力も 34.72 MB に膨らむ。README の「JPEG を Flate で再圧縮するのでサイズが
  増えることがある」という既知の制限に該当する。隠さずに載せること。

## 計測でつまずいた点（同じ轍を踏まないために）

1. **Python 経由で時間を測ると 1 回あたり約 200 ms 上乗せされる。**
   sumi が 247 ms と出たが、`/usr/bin/time -p` で測ると 40 ms。
   計測は `/usr/bin/time -p` の `real` を使うこと。
2. **`subprocess` はシグナル終了を負の値で返す。**
   `returncode == 139` で数えると 0 件になる。`returncode < 0` で判定する。
   シェルの `$?` は 139 で、Python とは値が違う。
3. **`mutool` は無言で落ちる。** stderr も空、終了コードだけが手がかり。
   出力ファイルの有無もあわせて確認する。
4. 既存の `bench/bench.py` は `tools()` でツールを名前引きしており、
   `/usr/bin/time` とウォームアップ、テキスト類似度、色残りの計測を持っている。
   手で測るより、ここに mutool を 1 項目足すほうが確実。

## 既知の報告を調べた結果

MuPDF の課題管理は GitHub ではなく **Artifex の Bugzilla**（bugs.ghostscript.com）。
GitHub 検索は 0 件だが、これは探す場所が違うだけ。

`product=MuPDF` で `recolor` を検索すると 2 件あるが、どちらも別件。

| Bug | 題 | 状態 |
|---|---|---|
| 707700 | alpha only pixmap で失敗 | 1.24.0 で FIXED |
| 707927 | `-c` の既定値が効かない | INVALID（使い方の問題） |

`colorspace.c` の SIGSEGV も 2 件（707227 / 707019）あるが、
いずれも 2024 年前半にファジングで発見・修正済みで別件。

**1.28.3 で通常の PDF が落ちる件に合致する報告は見つからなかった。**
「未報告」と断定はできないが、公開範囲には該当なし。

## 判断待ちの事項

**README への掲載は保留中。** 数字をそのまま載せることは勧めていない。

- 「完走率 84.7%」を掲げるのは、比較の体裁を借りた欠陥の指摘に見える
- こちらの計測は 1 台・150 本の抽出で、Ghostscript 比較（2 台・10 回試行）と
  厳密さが揃っていない
- 未報告の不具合の可能性が高く、公表より先に開発元へ報告するのが順序

**先に Artifex へ報告し、その後に掲載を判断する**のが筋と考えている。
修正されなくても、「同じ方式の実装は AGPL のみ」というライセンス面の記述は
完走率に触れずに書ける。

| | sumi | mutool recolor | Ghostscript |
|---|---|---|---|
| 方式 | 色指定を書き換え | 色指定を書き換え | PDF を再生成 |
| ライセンス | **MIT** | AGPL-3.0 | AGPL |

方式が同じなのは mutool のほうで、Ghostscript より比較相手として適切。

## 参考

- クラッシュしたファイル名の一覧: `/tmp/mutool-crashes.txt`（この環境のみ、
  コーパスが git 管理外なので他機では再取得が必要）
- コーパスの取得: `python3 validation/fetch_corpus.py --count 1000`（約 25 分、3.4 GB）
- `mutool recolor` の実装: MuPDF の `source/pdf/pdf-recolor.c`。
  `color_rewrite` / `image_rewrite` / `shade_rewrite` / `vertex_rewrite` があり、
  ラスタライズせずに色だけ書き換える。sumi と同じ方式。
