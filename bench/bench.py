#!/usr/bin/env python3
"""Compare sumi and Ghostscript converting PDFs to grayscale.

For every PDF in bench/inputs/ each tool is run as a command line program, the same way an
application would call it. Wall-clock time and peak memory come from /usr/bin/time; after
timing, the outputs are checked with poppler (pdftoppm, pdftotext) for leftover color and for
extractable text. Standard library only.

mutool recolor needs a bigger stack than the default: one of its frames reserves 8.06 MB,
more than the 8 MB main thread stack, so it dies with SIGSEGV on any PDF holding a shading. It
is run under `ulimit -s 65520` here so that the timings cover every input; the exit code it
gives with the default stack is recorded separately as "mutool_default_exit".

Usage: bench.py [--runs 10] [--warmup 1] [--sumi PATH] [--gs PATH] [--mutool PATH]
Writes bench/results-macos.json or bench/results-linux.json.
"""
import argparse
import difflib
import json
import os
import pathlib
import platform
import re
import statistics
import subprocess
import sys
import tempfile
import time

HERE = pathlib.Path(__file__).resolve().parent
INPUTS = HERE / "inputs"
OUTPUTS = HERE / "outputs"
RESULTS = HERE / f"results-{'macos' if sys.platform == 'darwin' else sys.platform}.json"

# Stack limit mutool runs under. 65520 KB is the macOS hard limit for the main thread; Linux
# usually allows it too. If the system refuses it, the shell exits 90 and the run is recorded
# as a failure rather than silently measuring a crash.
STACK_KB = 65520


class ToolFailed(Exception):
    """A tool exited non-zero. Recorded in the report instead of stopping the run."""

    def __init__(self, name, code, stderr):
        super().__init__(f"{name} failed with exit code {code}")
        self.code, self.stderr = code, stderr


def tools(args):
    return {
        "sumi": lambda src, dst: [args.sumi, str(src), "-o", str(dst), "--overwrite"],
        "ghostscript": lambda src, dst: [
            args.gs, "-q", "-dNOPAUSE", "-dBATCH", "-dSAFER",
            "-sDEVICE=pdfwrite",
            "-sColorConversionStrategy=Gray",
            "-dProcessColorModel=/DeviceGray",
            "-o", str(dst), str(src),
        ],
        # sh execs mutool, so /usr/bin/time still measures mutool itself.
        "mutool": lambda src, dst: [
            "/bin/sh", "-c",
            f"ulimit -s {STACK_KB} || exit 90; "
            f'exec "$0" recolor -c gray -o "$1" "$2"',
            args.mutool, str(dst), str(src),
        ],
    }


def default_stack_exit(mutool, src, dst):
    """Exit code of mutool recolor with the stack left alone. 139 means SIGSEGV."""
    dst.unlink(missing_ok=True)
    result = subprocess.run([mutool, "recolor", "-c", "gray", "-o", str(dst), str(src)],
                            capture_output=True)
    dst.unlink(missing_ok=True)
    # subprocess reports a signal as a negative number; the shell would show 128 + signal.
    code = result.returncode
    return 128 - code if code < 0 else code


def timed_run(command):
    """Runs a command under /usr/bin/time; returns (seconds, peak RSS in bytes)."""
    flag = "-l" if sys.platform == "darwin" else "-v"
    start = time.perf_counter()
    result = subprocess.run(["/usr/bin/time", flag, *command], capture_output=True, text=True)
    elapsed = time.perf_counter() - start
    if result.returncode != 0:
        raise ToolFailed(command[0], result.returncode, result.stderr[-2000:])
    if sys.platform == "darwin":
        rss = int(re.search(r"(\d+)\s+maximum resident set size", result.stderr).group(1))
    else:
        rss = int(re.search(r"Maximum resident set size \(kbytes\): (\d+)", result.stderr).group(1)) * 1024
    return elapsed, rss


def page_count(pdf):
    out = subprocess.run(["pdfinfo", str(pdf)], capture_output=True, text=True).stdout
    match = re.search(r"Pages:\s+(\d+)", out)
    return int(match.group(1)) if match else None


def colored_pixels(pdf, pages=3):
    """Counts pixels whose RGB channels differ by more than 3 on the first pages at 30 dpi."""
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(["pdftoppm", "-r", "30", "-l", str(pages), str(pdf), f"{tmp}/p"], check=True, capture_output=True)
        colored = 0
        for ppm in sorted(pathlib.Path(tmp).glob("p-*.ppm")):
            data = ppm.read_bytes()
            # Header: "P6", width, height, maxval, each followed by one whitespace byte.
            pos, fields = 0, []
            while len(fields) < 4:
                while data[pos:pos + 1].isspace():
                    pos += 1
                start = pos
                while not data[pos:pos + 1].isspace():
                    pos += 1
                fields.append(data[start:pos])
            pixels = data[pos + 1:]
            colored += sum(
                1 for i in range(0, len(pixels) - 2, 3)
                if max(pixels[i], pixels[i + 1], pixels[i + 2]) - min(pixels[i], pixels[i + 1], pixels[i + 2]) > 3
            )
        return colored


def text_similarity(original, converted):
    """Similarity (0-1) of the text extracted by pdftotext, ignoring whitespace."""
    def text(pdf):
        out = subprocess.run(["pdftotext", str(pdf), "-"], capture_output=True, text=True, errors="replace").stdout
        return re.sub(r"\s+", "", out)
    a, b = text(original), text(converted)
    if not a and not b:
        return 1.0
    return difflib.SequenceMatcher(None, a, b, autojunk=False).ratio()


def version(command):
    out = subprocess.run(command, capture_output=True, text=True)
    return (out.stdout or out.stderr).strip().splitlines()[0]


def machine():
    if sys.platform == "darwin":
        cpu = subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True).stdout.strip()
        memory = int(subprocess.run(["sysctl", "-n", "hw.memsize"], capture_output=True, text=True).stdout)
        os_name = "macOS " + subprocess.run(["sw_vers", "-productVersion"], capture_output=True, text=True).stdout.strip()
    elif sys.platform.startswith("linux"):
        cpu = re.search(r"model name\s*:\s*(.+)", pathlib.Path("/proc/cpuinfo").read_text()).group(1).strip()
        # Installed memory from the DMI data udev exposes without root; MemTotal (which leaves out
        # memory reserved for firmware and the integrated GPU) when that is not available.
        try:
            dmi = subprocess.run(["udevadm", "info", "-q", "property", "-p", "/sys/devices/virtual/dmi/id"],
                                 capture_output=True, text=True).stdout
        except FileNotFoundError:
            dmi = ""
        memory = sum(int(size) for size in re.findall(r"^MEMORY_DEVICE_\d+_SIZE=(\d+)$", dmi, re.M))
        memory = memory or int(re.search(r"MemTotal:\s+(\d+) kB", pathlib.Path("/proc/meminfo").read_text()).group(1)) * 1024
        release = platform.freedesktop_os_release()
        os_name = f"{release.get('NAME', 'Linux')} {release.get('VERSION_ID', '')}".strip() + f" (Linux {platform.release()})"
    else:
        cpu, memory, os_name = platform.processor(), 0, platform.platform()
    return {"cpu": cpu, "memory_gb": round(memory / 2**30), "os": os_name}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--runs", type=int, default=10)
    parser.add_argument("--warmup", type=int, default=1)
    parser.add_argument("--sumi", default=str(HERE.parent / "target" / "release" / "sumi"))
    parser.add_argument("--gs", default="gs")
    parser.add_argument("--mutool", default="mutool")
    args = parser.parse_args()

    OUTPUTS.mkdir(exist_ok=True)
    inputs = sorted(INPUTS.glob("*.pdf"), key=lambda p: p.stat().st_size)
    if not inputs:
        sys.exit("no PDFs in bench/inputs (run fetch.py and make_batch.py first)")

    report = {
        "machine": machine(),
        "sumi": version([args.sumi, "--version"]),
        "ghostscript": "Ghostscript " + version([args.gs, "--version"]),
        "mutool": version([args.mutool, "-v"]),
        "mutool_stack_kb": STACK_KB,
        "runs": args.runs,
        "results": [],
    }
    print(json.dumps({k: v for k, v in report.items() if k != "results"}, ensure_ascii=False), file=sys.stderr)

    for src in inputs:
        entry = {"input": src.name, "pages": page_count(src), "input_bytes": src.stat().st_size, "tools": {}}
        entry["mutool_default_exit"] = default_stack_exit(args.mutool, src, OUTPUTS / "probe.pdf")
        for name, build in tools(args).items():
            dst = OUTPUTS / f"{src.stem}.{name}.pdf"
            command = build(src, dst)
            try:
                for _ in range(args.warmup):
                    timed_run(command)
                samples = [timed_run(command) for _ in range(args.runs)]
            except ToolFailed as failure:
                entry["tools"][name] = {"failed": True, "exit_code": failure.code}
                print(f"{src.name:28s} {name:12s} failed (exit {failure.code})", file=sys.stderr)
                continue
            times = [s[0] for s in samples]
            entry["tools"][name] = {
                "median_s": statistics.median(times),
                "min_s": min(times),
                "peak_rss_mb": statistics.median(s[1] for s in samples) / 2**20,
                "output_bytes": dst.stat().st_size,
                "colored_pixels": colored_pixels(dst),
                "text_similarity": text_similarity(src, dst),
            }
            t = entry["tools"][name]
            print(f"{src.name:28s} {name:12s} {t['median_s']*1000:8.0f} ms  {t['peak_rss_mb']:6.1f} MB  "
                  f"{t['output_bytes']/1e6:6.2f} MB  colored={t['colored_pixels']}  text={t['text_similarity']:.3f}",
                  file=sys.stderr)
        report["results"].append(entry)

    RESULTS.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(f"wrote {RESULTS}", file=sys.stderr)


if __name__ == "__main__":
    main()
