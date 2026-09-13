# mutool recolor との比較 — 引き継ぎメモ

別の Mac で計測するための覚え書き。2026-09-12 時点。

> **2026-09-12 追記 — 決着した。**
> Apple Silicon（M1 Pro / macOS 26.5.2 / arm64）でも同じ 3 本が SIGSEGV。**環境固有ではない。**
> さらに原因を特定した。`mutool recolor` は Shading を含む PDF で必ずスタックを溢れさせる。
> 詳しくは「[2 台目での結果](#2-台目での結果apple-silicon)」と「[原因](#原因--8-mb-のスタック配列)」。

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
  rc=$?   # ← 先に控える。同じ行で $(basename ...) を使うと $? が 0 に潰れる
  echo "$(basename "$f"): rc=$rc  出力=$([ -f /tmp/o.pdf ] && echo あり || echo なし)"
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

**このファイル依存の正体は「Shading（グラデーション）を含むかどうか」だった。** 下記参照。

## 2 台目での結果（Apple Silicon）

| 項目 | 1 台目 | 2 台目 |
|---|---|---|
| macOS | 14.8.7（Darwin 23.6.0） | 26.5.2（Darwin 25.5.0、build 25F84） |
| CPU | Intel Core i5-8500B | Apple M1 Pro |
| arch | x86_64 | **arm64** |
| mutool | 1.28.3（bottle） | 1.28.3（bottle `arm64_tahoe`、ネイティブ arm64） |

同じ fixture、同じ結果:

| fixture | 1 台目 rc | 2 台目 rc |
|---|---|---|
| chrome_invoice.pdf | 139 | **139** |
| gs_objstm_cmyk.pdf | 139 | **139** |
| quartz_graphics.pdf | 139 | **139** |
| libreoffice_table.pdf | 0 | 0 |

**CPU 種別も OS 版も 12 世代違うのに一致した。環境固有ではない。**
メモの目的だった切り分けはこれで済んだ。

## 原因 — 8 MB のスタック配列

`lldb` で止めるとスタックオーバーフローだった。再帰ではなく、**関数 1 つ分のフレームが
スタック全体より大きい**。

```
stop reason = EXC_BAD_ACCESS (code=2, address=0x16f603ff8)
  frame #0: libsystem_pthread.dylib`___chkstk_darwin + 60
  frame #1: mutool`pdf_recolor_shade + 64
  ...
  frame #11: mutool`pdf_recolor_page + 136
  frame #12: mutool`pdfrecolor_main + 576
  frame #13: mutool`main + 492
```

`___chkstk_darwin` はスタックの伸長を確かめる処理で、ここで落ちるのは
「確保しようとした量が残りより大きい」という意味。フレームは 15 段しかない。

`pdf_recolor_shade` の冒頭を逆アセンブルすると、確保量がそのまま読める。

```
<+44>: mov  w9, #0x360                ; 確保量を w9 に
<+48>: movk w9, #0x81, lsl #16        ;   → 0x00810360 = 8,455,008
<+60>: blr  x16                       ; ___chkstk_darwin（ここで落ちる）
<+64>: sub  sp, sp, #0x810, lsl #12
<+68>: sub  sp, sp, #0x360
```

| | バイト |
|---|---|
| `pdf_recolor_shade` の 1 フレーム | 8,455,008（8.06 MB） |
| macOS のメインスレッドのスタック（`ulimit -s` 8176 KB） | 8,372,224（7.98 MB） |
| **超過分** | **82,784** |

出どころは `source/pdf/pdf-shade-recolor.c`。

```c
#define FUNSEGS 256 /* size of sampled mesh for function-based shadings */

static void
fz_recolor_shade_type1(fz_context *ctx, pdf_obj *shade, pdf_function **func, recolor_details *rd)
{
	...
	float out[(FUNSEGS+1)*(FUNSEGS+1)*FZ_MAX_COLORS];
```

`FZ_MAX_COLORS` は 32（`include/mupdf/fitz/color.h`）なので
257 × 257 × 32 × 4 = **8,454,272 バイト**。実測フレーム 8,455,008 の 99.99% がこれ 1 本。

`fz_recolor_shade_type1` は `pdf_recolor_shade` にインライン展開されており、
**確保は関数の入口で無条件に行われる。** シェーディングの種別を見る前に落ちる。

証拠として、落ちた 3 本はどれも `ShadingType 2`（軸）と `3`（放射）で、
この配列を使う `ShadingType 1`（関数ベース）は 1 本も含まれていない。

```
chrome_invoice.pdf   : /PatternType 2 /ShadingType 2
gs_objstm_cmyk.pdf   : /ShadingType 2
quartz_graphics.pdf  : /ShadingType 2, /ShadingType 3
libreoffice_table.pdf: Shading なし          ← 唯一成功する
```

### 検証 1 — リポジトリ内の PDF 27 本で例外なし

`fixtures/` `samples/` `bench/` にある PDF 27 本すべてについて、Shading の数と
終了コードを突き合わせた。

| Shading | 本数 | 既定スタックでの rc | `ulimit -s 32768` での rc |
|---|---|---|---|
| あり（1〜25 個） | 9 | **全 9 本が 139** | 全 9 本が 0 |
| なし | 18 | 全 18 本が 0 | 全 18 本が 0 |

**例外ゼロ。** 相関ではなく因果と見てよい。

ついでに分かったこと: Ghostscript を通した出力（`*.ghostscript.pdf`）は Shading が
消えているため落ちない。sumi の出力（`*.sumi.pdf`）は Shading を残すので、元が
落ちるファイルは変換後も落ちる。色指定だけ書き換える方式である裏付けにもなっている。

### 検証 2 — スタックを広げると通る

```sh
for kb in 8176 16384; do
  rm -f /tmp/o.pdf
  ( ulimit -s $kb && mutool recolor -c gray -o /tmp/o.pdf fixtures/chrome_invoice.pdf )
  echo "ulimit -s ${kb}KB : rc=$?"
done
```

| `ulimit -s` | rc | 出力 |
|---|---|---|
| 8176 KB（既定） | 139 | なし |
| 16384 KB | **0** | あり |

16 MB にすると 4 本とも rc=0 で出力ができる。診断はこれで確定。

### 結論

**`mutool recolor` は、Shading を 1 つでも含む PDF を、スタックが 8,455,008 バイトに
満たない環境で必ず落とす。** ファイル依存に見えたのは Shading の有無だっただけで、
生成器も PDF の壊れ具合も関係ない。

**環境依存であって、版依存ではない。** 閾値はフレームの大きさ 8,455,008 バイト
（約 8,257 KB）。これを下回る `ulimit -s` なら落ち、上回れば動く。

| 環境 | `ulimit -s` | 結果 |
|---|---|---|
| macOS 14.8.7 / x86_64 / mutool 1.28.3 | 8176 KB | 落ちる |
| macOS 26.5.2 / arm64 / mutool 1.28.3 | 8176 KB | 落ちる |
| Omarchy 4.0.1（Arch）/ x86_64 / mutool 1.28.0 | 8,257 KB より大きい（未確認） | **落ちない** |

Linux 機（`bench/results-linux.json` の計測環境）では 6 件とも完走している。
`pdf-shade-recolor.c` は 1.28.0 と 1.28.3 で**同一**なので、版の違いではなく
スタック上限の違い。この機の `ulimit -s` は未確認なので、機会があれば控えること。

`master`（2026-09-12 時点）でも `pdf-shade-recolor.c` の当該行は 1.28.3 と同じで、**未修正**。

直し方は素直で、`out` をスタックではなく `fz_malloc` で取れば済む。

### 検証 3 — コーパス 230 本でも例外なし（2 台目、arm64）

`ulimit -s` の有無で同じ PDF を 2 回変換した。Wikimedia Commons から集めた実 PDF 230 本。

| 結果 | 既定スタック | `ulimit -s 65520` |
|---|---:|---:|
| 完走（5 秒以内） | 180 (78.3%) | 214 (93.0%) |
| 完走（5 秒超） | 14 (6.1%) | 15 (6.5%) |
| クラッシュ | **35 (15.2%)** | **0 (0.0%)** |
| 60 秒で未完了 | 1 (0.4%) | 1 (0.4%) |
| **出力ができた** | **194 (84.3%)** | **229 (99.6%)** |

- 落ちた 35 本は **35 本ともスタックを広げるだけで正常終了**した
- Shading あり 35 本 → 全部クラッシュ。なし 195 本 → 1 本もクラッシュせず。**例外ゼロ**
- **スタックを広げても直らないクラッシュは 1 件も見つからなかった**
- 60 秒で終わらない 1 本は既定でも拡大後でも同じなので、クラッシュとは別の話（単に重い）

出力ができた割合 84.3% は、1 台目の 84.7%（150 本）とほぼ一致した。

これで「完走率」という指標は意味を失った。**差はこの不具合 1 件で説明がつく。**

計測に使ったスクリプトは、コーパスが git 管理外なのでリポジトリには入れていない。
やり直す場合は同じ PDF を 2 回変換し、`/usr/bin/time -p` の `real` と、シェルの `$?`
（`$(...)` で潰さないよう先に控える）を記録する。

## 1 台目での計測結果（x86_64）

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

原因が分かったあとに `fz_recolor_shade_type1` / スタックオーバーフローで
再検索しようとしたが、Bugzilla は bot 対策（Anubis）で機械的に引けない。
手で確かめる場合は `product=MuPDF`、`Component=mutool`、`short_desc` に
`recolor` または `shade` で検索する。

## 掲載（決着済み）

**2026-09-12 に README と GitHub Pages へ掲載した。** 以下は、そこに至るまでの経緯。

掲載のしかたは「完走率」を前面に出す形にせず、**「Shading を含む PDF で落ちる。原因はこれ、
回避は `ulimit -s`」と具体に書く**形にした。原因が単一の不具合と分かったので、そのほうが
正確で、かつ相手をあげつらう形にならない。速度・メモリ・出力・テキストは対等な比較として
並べてある。

main では PR #1 が Linux での mutool 比較を先に入れており、「Ghostscript と mutool
との比較」という 1 つのセクションに再構成されていた。**こちらの Mac の計測はその構造に
差し込む形でマージした。** 章を分けない。

- README: 「Ghostscript と mutool との比較」。Mac の表に mutool の列を追加し、
  「mutool が Mac で落ちる件」を `###` として足した
- Pages: `docs/index.html` の `#benchmark`。Mac の棒を 3 本にした
- 数値は `bench/results-*.json` から生成している。手で書き写すとずれるので
  `bench/bench.py` を通すこと
- **比率の出し方は main の流儀に合わせた。** 丸める前の中央値から計算する
  （表示上の ms を割った値とは 0.1 ずれることがある）
- Pages の棒は「控えめなほう」を出す。時間は 2 つの比の小さいほう、
  メモリは大きいほう

### 掲載前に保留していた理由

**数字をそのまま載せることは勧めていなかった。**

- 「完走率 84.7%」を掲げるのは、比較の体裁を借りた欠陥の指摘に見える
- こちらの計測は 1 台・150 本の抽出で、Ghostscript 比較（2 台・10 回試行）と
  厳密さが揃っていない
- 未報告の不具合の可能性が高く、公表より先に開発元へ報告するのが順序

原因が特定できたことで、この判断は固まった。**完走率の差はほぼ単一の不具合 1 件に
帰着する。** 実装方式や設計の優劣ではないので、比較表の指標には向かない。直れば消える
数字を掲げることになる。

報告用の草稿を `docs/mutool-shade-stack-overflow.md` に用意した。
Bugzilla（bugs.ghostscript.com）にアカウントを作って投稿する。投稿は人手で行うこと。

当初は「先に Artifex へ報告し、その後に掲載を判断する」つもりだったが、
**掲載を先に進める判断になった**（2026-09-12）。原因・回避策・未報告であることを
すべて書いたうえでの掲載なので、隠している事実はない。報告はこれから出す。

| | sumi | mutool recolor | Ghostscript |
|---|---|---|---|
| 方式 | 色指定を書き換え | 色指定を書き換え | PDF を再生成 |
| ライセンス | **MIT** | AGPL-3.0 | AGPL |

方式が同じなのは mutool のほうで、Ghostscript より比較相手として適切。

## 参考

- クラッシュしたファイル名の一覧: `/tmp/mutool-crashes.txt`（この環境のみ、
  コーパスが git 管理外なので他機では再取得が必要）
- コーパスの取得: `python3 validation/fetch_corpus.py --count 1000`。
  スクリプトのコメントは「1000 件で 15 分ほど」だが、2 台目での実測は約 12 本/分で、
  1000 件なら 70 分以上かかった。**この比較には 1000 本は不要。**
  クラッシュ例が数十件集まれば判定できるので、230 本で足りた
- 2 台目の計測で使ったベンチ: `bench/bench.py`（mutool を追加済み。`ulimit -s 65520` の
  下で実行し、既定スタックでの終了コードを `mutool_default_exit` に別途記録する）
- `mutool recolor` の実装: MuPDF の `source/pdf/pdf-recolor.c`。
  `color_rewrite` / `image_rewrite` / `shade_rewrite` / `vertex_rewrite` があり、
  ラスタライズせずに色だけ書き換える。sumi と同じ方式。
- クラッシュする側の実体は `source/pdf/pdf-shade-recolor.c` の `pdf_recolor_shade`。
  ソースは GitHub ミラーから取れる（`git.ghostscript.com` は HTTP 401 を返す）:

  ```sh
  gh api "repos/ArtifexSoftware/mupdf/contents/source/pdf/pdf-shade-recolor.c?ref=1.28.3" \
    --jq .content | base64 -d
  ```
- 逆アセンブルと再現に使った道具は `lldb`（Xcode Command Line Tools 付属）。
  `lldb -b -o run -k "bt 60" -k quit -- mutool recolor -c gray -o /tmp/o.pdf <PDF>`
  のように `-k` でクラッシュ時のコマンドを渡す。`-o "bt"` では出力されない。
- 報告草稿: `docs/mutool-shade-stack-overflow.md`
