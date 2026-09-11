#!/usr/bin/env python3
"""corpus の PDF を sumi で変換し、4 つの指標で自動検証する。

  変換成功率  : 終了コード（0 成功 / 3 不正 PDF / 4 未対応・暗号化 / 5 入出力 / 6 時間切れ）
  色残り      : 出力をレンダリングし、彩度のある画素が残っていないか
  表示崩れ    : 元 PDF をグレー化した画像と出力画像の輝度差
  テキスト保持: pdftotext の抽出結果が変換前後で一致するか

レンダリング画像はディスクに残さず、判定したら捨てる。

  python3 validation/validate.py --jobs 5 --mode grayscale
"""

import argparse
import concurrent.futures
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time

import numpy as np
from PIL import Image

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent
CORPUS = HERE / "corpus"
MANIFEST = HERE / "corpus.json"
SUMI = ROOT / "target" / "release" / "sumi"

# 既存テスト（fixtures.rs）と同じ基準。RGB の最大と最小の差がこれを超えたら色とみなす。
COLOR_TOLERANCE = 3
# 検証するページ数の上限。長い文書の全ページを見ると時間がかかりすぎる。
MAX_PAGES = 3
RENDER_DPI = 150

# 「表示崩れ」は輝度差では測れない。sumi は色を変えるのが仕事なので、
# 輝度差で測ると CMYK 文書が軒並み引っかかる（README の制限事項にある
# 「CMYK は ICC 管理より明るくなる」ぶんで、崩れではない）。
# そこで「インクがある場所」が動いていないかで見る。紙の白より暗い画素を
# インクとみなし、その分布が変換前後で一致するかを比べる。
# 色が変わってもインクの位置は動かないので、文字や線が消えた・ずれた場合だけ出る。
INK_THRESHOLD = 200          # これより暗い画素をインクとみなす（0-255）
INK_MARGIN = 25              # 閾値の際での取りこぼしを無視する幅
LAYOUT_TOLERANCE = 0.02      # インク位置が食い違う画素の許容割合


def render(pdf, out_dir, prefix, pages=MAX_PAGES):
    """PDF を PNG に描画し、ページ順のパス一覧を返す。"""
    result = subprocess.run(
        ["pdftoppm", "-r", str(RENDER_DPI), "-f", "1", "-l", str(pages), "-png",
         str(pdf), str(out_dir / prefix)],
        capture_output=True, timeout=180,
    )
    if result.returncode != 0:
        return []
    return sorted(out_dir.glob(f"{prefix}-*.png"))


def text_of(pdf):
    result = subprocess.run(
        ["pdftotext", "-q", str(pdf), "-"], capture_output=True, timeout=120
    )
    return result.stdout if result.returncode == 0 else None


def colored_pixels(image):
    """彩度のある画素の数と、全画素数を返す。"""
    array = np.asarray(image.convert("RGB"), dtype=np.int16)
    spread = array.max(axis=2) - array.min(axis=2)
    return int((spread > COLOR_TOLERANCE).sum()), int(spread.size / 1)


def luminance(image):
    # PDF の変換式に合わせた係数（README の「仕様との差分」参照）。
    array = np.asarray(image.convert("RGB"), dtype=np.float32)
    return array[..., 0] * 0.30 + array[..., 1] * 0.59 + array[..., 2] * 0.11


def ink_mask(image):
    """紙より暗い画素（インクのある場所）を True にした配列を返す。"""
    return luminance(image) < INK_THRESHOLD


def ink_changed(before_luma, after_luma, want_gained):
    """インクの増減を数える。閾値のすぐ際にある画素は数えない。

    明るい色（黄緑など）は輝度が閾値の近くに来るため、わずかな変化でも
    インクが消えた・増えたと数えられてしまう。実際には明るい色が明るい
    グレーになっただけで正常なので、閾値をまたいだうえで MARGIN 以上
    動いた画素だけを数える。
    """
    if want_gained:
        crossed = (before_luma >= INK_THRESHOLD) & (after_luma < INK_THRESHOLD)
    else:
        crossed = (before_luma < INK_THRESHOLD) & (after_luma >= INK_THRESHOLD)
    return float((crossed & (np.abs(before_luma - after_luma) >= INK_MARGIN)).mean())


def layout_shift(original, converted):
    """インクの増減を方向別に返す: (消えた割合, 増えた割合)。

    monochrome では薄い色が白に飛ぶ（README の制限事項）。これは仕様どおりなので
    「消えた」側に出る。一方インクが増えるのは、紙が黒くなる・図形が塗り潰される
    といった異常で、こちらが本当の崩れにあたる。両者を混ぜると区別できないため
    分けて返す。
    """
    before, after = luminance(original), luminance(converted)
    lost = ink_changed(before, after, want_gained=False)
    gained = ink_changed(before, after, want_gained=True)
    return lost, gained


def check_one(entry, mode, timeout):
    """1 つの PDF を変換し、4 指標を測る。"""
    path = CORPUS / entry["file"]
    record = {
        "file": entry["file"], "title": entry.get("title", ""),
        "axis": entry.get("axis", ""), "source": entry.get("source", ""),
        "size": entry.get("size", 0),
    }
    if not path.exists():
        record["status"] = "missing"
        return record

    work = pathlib.Path(tempfile.mkdtemp(prefix="sumi-val-"))
    try:
        out = work / "out.pdf"
        run = subprocess.run(
            [str(SUMI), str(path), "-o", str(out), "--mode", mode,
             "--overwrite", "--verbose", "--timeout", str(timeout)],
            capture_output=True, timeout=timeout + 60,
        )
        record["exit_code"] = run.returncode
        stderr = run.stderr.decode("utf-8", "replace").strip()
        record["message"] = stderr.splitlines()[0][:200] if stderr else ""
        # 未対応部分の警告は「... left unchanged」などの形で出る。
        record["warnings"] = [
            line[:160] for line in stderr.splitlines()
            if "warning" in line.lower() or "unchanged" in line.lower()
        ]

        if run.returncode != 0 or not out.exists():
            record["status"] = "failed"
            return record
        record["status"] = "converted"
        record["out_size"] = out.stat().st_size

        # --- テキスト保持 ---
        before, after = text_of(path), text_of(out)
        if before is None or after is None:
            record["text"] = "unknown"
        elif before == after:
            record["text"] = "same"
        else:
            record["text"] = "changed"
            b = before.split()
            record["text_ratio"] = round(len(after.split()) / len(b), 4) if b else None

        # --- 色残りと表示崩れ（同じレンダリングを使い回す）---
        originals = render(path, work, "orig")
        converted = render(out, work, "conv")
        if not converted:
            record["render"] = "failed"
            return record
        record["render"] = "ok"
        record["pages_checked"] = len(converted)

        colored_total = pixels_total = 0
        worst_lost = worst_gained = 0.0
        for index, conv_png in enumerate(converted):
            with Image.open(conv_png) as conv_image:
                colored, total = colored_pixels(conv_image)
                colored_total += colored
                pixels_total += total
                size = conv_image.size
                if index < len(originals):
                    with Image.open(originals[index]) as orig_image:
                        if orig_image.size != size:
                            orig_image = orig_image.resize(size)
                        lost, gained = layout_shift(orig_image, conv_image)
                        worst_lost = max(worst_lost, lost)
                        worst_gained = max(worst_gained, gained)

        record["colored_pixels"] = colored_total
        record["colored_ratio"] = round(colored_total / pixels_total, 8) if pixels_total else 0.0
        # 消えた側は monochrome の仕様どおりの挙動、増えた側が本当の崩れ。
        record["ink_lost_ratio"] = round(worst_lost, 6)
        record["ink_gained_ratio"] = round(worst_gained, 6)
        return record
    except subprocess.TimeoutExpired:
        record["status"] = "timeout"
        return record
    except Exception as error:  # 検証側の失敗で全体を止めない
        record["status"] = "error"
        record["message"] = f"{type(error).__name__}: {error}"[:200]
        return record
    finally:
        shutil.rmtree(work, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", default="grayscale", choices=["grayscale", "monochrome"])
    parser.add_argument("--jobs", type=int, default=5)
    parser.add_argument("--timeout", type=int, default=120, help="1 件あたりの秒数")
    parser.add_argument("--limit", type=int, help="先頭から この件数だけ検証する")
    parser.add_argument("--out", default=None, help="結果の JSON 出力先")
    args = parser.parse_args()

    if not SUMI.exists():
        sys.exit(f"{SUMI} がありません。cargo build --release を実行してください。")
    if not MANIFEST.exists():
        sys.exit(f"{MANIFEST} がありません。fetch_corpus.py を先に実行してください。")

    entries = json.loads(MANIFEST.read_text(encoding="utf-8"))["entries"]
    if args.limit:
        entries = entries[:args.limit]
    print(f"== {len(entries)} 件を {args.mode} で検証（並列 {args.jobs}）==")

    # 進捗は別ファイルにも書く。標準出力はパイプ越しだとバッファされて
    # 届かないことがあり、長い実行では進み具合が分からなくなるため。
    progress_path = HERE / f"progress-{args.mode}.txt"
    started = time.time()

    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        futures = [pool.submit(check_one, e, args.mode, args.timeout) for e in entries]
        for i, future in enumerate(concurrent.futures.as_completed(futures), 1):
            results.append(future.result())
            if i % 10 == 0 or i == len(entries):
                ok = sum(1 for r in results if r.get("status") == "converted")
                elapsed = time.time() - started
                rate = i / elapsed * 60
                left = (len(entries) - i) / rate if rate else 0
                line = (f"{i}/{len(entries)}（変換成功 {ok}）"
                        f" {rate:.1f}件/分 残り約{left:.0f}分")
                print(f"  {line}", flush=True)
                progress_path.write_text(line + "\n", encoding="utf-8")

    out_path = pathlib.Path(args.out) if args.out else HERE / f"results-{args.mode}.json"
    out_path.write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\n結果: {out_path}")
    summarize(results, args.mode)


def summarize(results, mode):
    total = len(results)
    converted = [r for r in results if r.get("status") == "converted"]
    print(f"\n== 集計（{mode}、{total} 件）==")
    print(f"変換成功      {len(converted)}/{total}（{len(converted) / total:.1%}）")

    from collections import Counter
    codes = Counter(r.get("exit_code") for r in results if r.get("status") != "converted")
    for code, count in sorted(codes.items(), key=lambda x: -x[1]):
        label = {3: "不正な PDF", 4: "未対応・暗号化", 5: "入出力", 6: "時間切れ",
                 None: "検証側の問題"}.get(code, f"終了コード {code}")
        print(f"  失敗 {label:16s} {count}")

    rendered = [r for r in converted if r.get("render") == "ok"]
    if rendered:
        clean = [r for r in rendered if r.get("colored_pixels", 0) == 0]
        print(f"色残りなし    {len(clean)}/{len(rendered)}（{len(clean) / len(rendered):.1%}）")
        worst = sorted(rendered, key=lambda r: -r.get("colored_ratio", 0))[:5]
        for r in worst:
            if r.get("colored_ratio", 0) > 0:
                print(f"    色残り {r['colored_ratio']:.6f}  {r['file']}  {r['title'][:44]}")

        intact = [r for r in rendered if r.get("ink_gained_ratio", 1) <= LAYOUT_TOLERANCE]
        print(f"表示崩れなし  {len(intact)}/{len(rendered)}（{len(intact) / len(rendered):.1%}）"
              "  ※インクが増えた＝塗り潰し・黒化")
        worst = sorted(rendered, key=lambda r: -r.get("ink_gained_ratio", 0))[:5]
        for r in worst:
            if r.get("ink_gained_ratio", 0) > LAYOUT_TOLERANCE:
                print(f"    崩れ {r['ink_gained_ratio']:.4f}  {r['file']}  {r['title'][:44]}")

        # 消失は monochrome では仕様どおり（薄い色が白に飛ぶ）。参考値として出す。
        faded = [r for r in rendered if r.get("ink_lost_ratio", 0) > 0.10]
        if faded:
            print(f"参考: インクが1割以上消えた文書 {len(faded)}/{len(rendered)} 件"
                  f"（{mode} の閾値処理による。grayscale で多い場合は要調査）")
            for r in sorted(faded, key=lambda r: -r.get("ink_lost_ratio", 0))[:3]:
                print(f"    消失 {r['ink_lost_ratio']:.4f}  {r['file']}  {r['title'][:44]}")

    same = [r for r in converted if r.get("text") == "same"]
    checked = [r for r in converted if r.get("text") in ("same", "changed")]
    if checked:
        print(f"テキスト保持  {len(same)}/{len(checked)}（{len(same) / len(checked):.1%}）")
        for r in [x for x in checked if x.get("text") == "changed"][:5]:
            print(f"    差分あり  {r['file']}  比 {r.get('text_ratio')}  {r['title'][:40]}")

    warned = [r for r in converted if r.get("warnings")]
    if warned:
        print(f"\n未対応の警告が出た文書 {len(warned)} 件")
        messages = Counter(w for r in warned for w in r["warnings"])
        for message, count in messages.most_common(8):
            print(f"  {count:4d}  {message[:96]}")


if __name__ == "__main__":
    main()
