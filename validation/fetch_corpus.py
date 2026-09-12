#!/usr/bin/env python3
"""検証用の PDF コーパスを validation/corpus/ に集める。

Wikimedia Commons から分野別に集め、生成系 PDF（Chrome / LibreOffice）を混ぜて
スキャン画像への偏りを補正する。標準ライブラリだけで動く。

  python3 validation/fetch_corpus.py --count 1000
"""

import argparse
import concurrent.futures
import hashlib
import json
import pathlib
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent
CORPUS = HERE / "corpus"
MANIFEST = HERE / "corpus.json"

USER_AGENT = "sumi-validation/0.1 (https://github.com/champierre/sumi)"

# 検索軸。実世界の PDF が偏らないよう、用途の違うものを並べる。
AXES = [
    "invoice", "receipt", "report", "annual report", "presentation",
    "poster", "map", "brochure", "manual", "chart", "newsletter",
    "form", "diagram", "infographic", "thesis", "catalog",
    "newspaper", "certificate", "schedule", "budget",
]

# 巨大ファイルは検証の役に立たない割にディスクと時間を食うので上限を設ける。
MAX_BYTES = 20_000_000
MIN_BYTES = 1_000

# Wikimedia のレート制限（429）を避けるため、1 件ごとに待つ秒数。
REQUEST_DELAY = 0.4

CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
SOFFICE = "/Applications/LibreOffice.app/Contents/MacOS/soffice"


def api(params):
    url = "https://commons.wikimedia.org/w/api.php?" + urllib.parse.urlencode(params)
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=60) as response:
        return json.load(response)


def search_axis(axis, want):
    """1 つの検索軸から、条件を満たす PDF のメタデータを集める。"""
    found = []
    offset = 0
    while len(found) < want and offset < 1000:
        try:
            data = api({
                "action": "query", "generator": "search",
                "gsrsearch": f"filetype:pdf {axis}", "gsrnamespace": "6",
                "gsrlimit": "50", "gsroffset": str(offset),
                "prop": "imageinfo", "iiprop": "url|size|mime|extmetadata",
                "format": "json",
            })
        except Exception as error:
            print(f"  {axis}: 検索に失敗 ({error})", file=sys.stderr)
            break
        pages = list(data.get("query", {}).get("pages", {}).values())
        if not pages:
            break
        for page in pages:
            info = (page.get("imageinfo") or [{}])[0]
            size = info.get("size", 0)
            # PDF 以外（djvu など）と、極端なサイズを除く。
            if info.get("mime") != "application/pdf":
                continue
            if not (MIN_BYTES <= size <= MAX_BYTES):
                continue
            if not info.get("url"):
                continue
            meta = info.get("extmetadata", {})
            found.append({
                "title": page["title"],
                "url": info["url"],
                "size": size,
                "license": meta.get("LicenseShortName", {}).get("value", "?"),
                "descriptionurl": info.get("descriptionurl", ""),
                "axis": axis,
                "source": "commons",
            })
            if len(found) >= want:
                break
        offset += 50
    return found


def collect_metadata(count):
    """全軸から均等に集めて、重複を除く。"""
    per_axis = max(5, count // len(AXES) + 5)
    entries = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        futures = {pool.submit(search_axis, axis, per_axis): axis for axis in AXES}
        for future in concurrent.futures.as_completed(futures):
            axis = futures[future]
            try:
                results = future.result()
            except Exception as error:
                print(f"  {axis}: {error}", file=sys.stderr)
                continue
            for entry in results:
                entries.setdefault(entry["title"], entry)
            print(f"  {axis:16s} {len(results):4d} 件")
    return list(entries.values())[:count]


def download(entry):
    # ファイル名は URL から決める。hash() は実行ごとに変わるため使えない
    # （同じ PDF を毎回別名で取り直すことになる）。
    digest = hashlib.sha1(entry["url"].encode("utf-8")).hexdigest()[:12]
    name = f"{digest}.pdf"
    target = CORPUS / name
    if target.exists() and target.stat().st_size > 0:
        entry["file"] = name
        return entry
    # Wikimedia は並列に叩くと 429 を返す。間隔を空け、429 なら待って数回試す。
    data = None
    for attempt in range(4):
        try:
            request = urllib.request.Request(entry["url"], headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(request, timeout=180) as response:
                data = response.read()
            break
        except urllib.error.HTTPError as error:
            if error.code == 429:
                time.sleep(REQUEST_DELAY * (attempt + 2) * 3)
                continue
            entry["error"] = f"HTTP {error.code}"
            return entry
        except Exception as error:
            entry["error"] = str(error)[:200]
            return entry
    if data is None:
        entry["error"] = "HTTP 429（リトライしても取得できず）"
        return entry
    time.sleep(REQUEST_DELAY)
    # 中身が本当に PDF か確かめる（リダイレクト先が HTML のことがある）。
    if not data.startswith(b"%PDF-"):
        entry["error"] = "PDF ヘッダがない"
        return entry
    target.write_bytes(data)
    entry["file"] = name
    return entry


def generate_synthetic():
    """Chrome と LibreOffice で生成系 PDF を作り、スキャン偏りを補正する。"""
    made = []
    src = HERE / "synthetic_src"
    src.mkdir(exist_ok=True)

    # 色と要素の組み合わせを変えた HTML を用意する。
    cases = {
        "invoice": """<h1 style="color:#c0392b">請求書</h1>
<table border=1 style="border-collapse:collapse">
<tr style="background:#3498db;color:#fff"><th>品目<th>数量<th>金額
<tr><td>設計<td>2<td style="color:#27ae60">¥120,000
<tr><td>実装<td>5<td style="color:#27ae60">¥380,000</table>
<p style="color:#888">備考: 振込手数料は貴社負担でお願いします。</p>""",
        "chart": """<h2 style="color:#8e44ad">四半期の売上</h2>
<svg width="420" height="220">
<rect x="20" y="40" width="60" height="150" fill="#e74c3c"/>
<rect x="100" y="70" width="60" height="120" fill="#f39c12"/>
<rect x="180" y="20" width="60" height="170" fill="#2ecc71"/>
<rect x="260" y="90" width="60" height="100" fill="#3498db"/>
<line x1="10" y1="190" x2="410" y2="190" stroke="#333" stroke-width="2"/>
</svg>""",
        "gradient": """<div style="width:400px;height:120px;
background:linear-gradient(90deg,#ff0000,#00ff00,#0000ff)"></div>
<p style="color:#e67e22">グラデーションと<span style="color:#16a085">複数色</span>の文字。</p>""",
        "lighttext": """<p style="background:#fff">
<span style="color:#ffff00">黄色の文字（モノクロで消えるはず）</span><br>
<span style="color:#000">黒い文字（残るはず）</span></p>
<div style="background:#2c3e50;color:#fff;padding:12px">白抜き文字</div>""",
    }
    if pathlib.Path(CHROME).exists():
        for name, body in cases.items():
            html = src / f"{name}.html"
            html.write_text(
                f'<meta charset="utf-8"><body style="font-family:sans-serif">{body}</body>',
                encoding="utf-8",
            )
            out = CORPUS / f"synthetic-chrome-{name}.pdf"
            if not out.exists():
                subprocess.run(
                    [CHROME, "--headless=new", "--disable-gpu", "--no-pdf-header-footer",
                     "--virtual-time-budget=8000", f"--print-to-pdf={out}", html.as_uri()],
                    capture_output=True, timeout=90,
                )
            if out.exists():
                made.append({"title": f"synthetic chrome {name}", "file": out.name,
                             "size": out.stat().st_size, "license": "生成物",
                             "axis": name, "source": "chrome"})
    # リポジトリ内の既存 fixture も、既知の正解として corpus に含める。
    for fixture in sorted((ROOT / "fixtures").glob("*.pdf")):
        out = CORPUS / f"fixture-{fixture.name}"
        shutil.copy(fixture, out)
        made.append({"title": f"fixture {fixture.name}", "file": out.name,
                     "size": out.stat().st_size, "license": "リポジトリ同梱",
                     "axis": "fixture", "source": "fixture"})
    return made


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--count", type=int, default=1000, help="集める PDF の数")
    args = parser.parse_args()

    CORPUS.mkdir(parents=True, exist_ok=True)

    print(f"== メタデータを集める（目標 {args.count} 件）==")
    entries = collect_metadata(args.count)
    print(f"  重複を除いて {len(entries)} 件\n")

    # 429 を避けるため逐次に落とす。1 件あたり 1 秒弱なので 1000 件で 15 分ほど。
    print("== ダウンロード ==")
    done = []
    for i, entry in enumerate(entries, 1):
        done.append(download(entry))
        if i % 50 == 0 or i == len(entries):
            ok = sum(1 for e in done if "file" in e)
            print(f"  {i}/{len(entries)} 件（成功 {ok}）")

    print("\n== 生成系 PDF を追加 ==")
    synthetic = generate_synthetic()
    print(f"  {len(synthetic)} 件")

    corpus = [e for e in done if "file" in e] + synthetic
    failed = [e for e in done if "file" not in e]
    MANIFEST.write_text(
        json.dumps({"entries": corpus, "failed": failed}, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    total = sum(e.get("size", 0) for e in corpus)
    print(f"\n合計 {len(corpus)} 件 / {total / 1e9:.2f} GB（取得失敗 {len(failed)} 件）")
    print(f"一覧: {MANIFEST}")


if __name__ == "__main__":
    main()
