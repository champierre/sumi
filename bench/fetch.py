#!/usr/bin/env python3
"""Put the benchmark input PDFs into bench/inputs/.

The invoices are copied from the repository (the 50-page one is the file that was measured;
make_batch.py rebuilds it). The rest are downloaded from Wikimedia Commons; all of them are
public domain or CC0. Standard library only.
"""
import json
import pathlib
import shutil
import urllib.parse
import urllib.request

HERE = pathlib.Path(__file__).resolve().parent
INPUTS = HERE / "inputs"
USER_AGENT = "sumi-benchmark/0.1 (https://github.com/champierre/sumi)"

# local name -> Wikimedia Commons file title
COMMONS = {
    "equal-pay-infographic.pdf": "File:Equal Pay Infographic.pdf",
    "nasa-sls-fact-sheet.pdf": "File:0080 SLS Fact Sheet 10162019 PRINT FINAL (656622902519).pdf",
    "communia-poster.pdf": "File:Best Case Scenarios for Copyright - poster.pdf",
    "europe-map.pdf": "File:Political map of Europe.pdf",
}


def get(url):
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    return urllib.request.urlopen(request, timeout=120)


def main():
    INPUTS.mkdir(exist_ok=True)
    shutil.copy(HERE.parent / "fixtures" / "chrome_invoice.pdf", INPUTS / "invoice.pdf")
    shutil.copy(HERE / "invoice-50pages.pdf", INPUTS / "invoice-50pages.pdf")
    query = urllib.parse.urlencode({
        "action": "query",
        "titles": "|".join(COMMONS.values()),
        "prop": "imageinfo",
        "iiprop": "url|extmetadata",
        "format": "json",
    })
    pages = json.load(get("https://commons.wikimedia.org/w/api.php?" + query))["query"]["pages"]
    urls = {p["title"]: p["imageinfo"][0] for p in pages.values()}
    for name, title in COMMONS.items():
        info = urls[title]
        license_name = info["extmetadata"]["LicenseShortName"]["value"]
        target = INPUTS / name
        if not target.exists():
            target.write_bytes(get(info["url"]).read())
        print(f"{name}: {license_name} ({info['descriptionurl']})")


if __name__ == "__main__":
    main()
