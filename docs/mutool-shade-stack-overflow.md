# Artifex Bugzilla への報告 — 草稿

投稿先: https://bugs.ghostscript.com/ （Product: MuPDF, Component: mutool）
投稿は人手で行う。以下は本文としてそのまま貼れる形にしてある。

参考にした先例の書式: 既存の MuPDF バグ（707700 / 707927）は
「Summary / Version / Platform / Steps / Actual / Expected」の素直な英文。

---

## Summary

`mutool recolor` crashes (SIGSEGV) on any PDF containing a shading:
8 MB stack array in `fz_recolor_shade_type1` overflows the default stack

## Version

MuPDF 1.28.3 (Homebrew bottle). The relevant code is unchanged on `master`
as of 2026-09-12.

## Platform

Reproduced on two machines with different CPU architectures and OS versions:

| | machine A | machine B |
|---|---|---|
| macOS | 14.8.7 (Darwin 23.6.0) | 26.5.2 (Darwin 25.5.0) |
| CPU | Intel Core i5-8500B | Apple M1 Pro |
| arch | x86_64 | arm64 |

Default `ulimit -s` is 8176 KB on both. Any platform with an 8 MB default
stack is affected, which includes most Linux distributions.

## Steps to reproduce

Any PDF that contains at least one shading. For example, a one-page PDF
printed from Chrome with a CSS `linear-gradient` background, or output from
Ghostscript or macOS Quartz containing `/ShadingType 2`.

```sh
mutool recolor -c gray -o out.pdf input.pdf
echo $?
```

## Actual result

```
$ mutool recolor -c gray -o out.pdf input.pdf
$ echo $?
139
```

Exit code 139 = SIGSEGV. Nothing is printed on stdout or stderr, and no
output file is produced, so the failure is silent unless the exit status is
checked.

## Expected result

Either a recolored PDF, or a diagnostic message and a non-zero exit code.

## Analysis

The crash is a stack overflow in a single stack frame, not runaway
recursion. The backtrace is only 15 frames deep:

```
stop reason = EXC_BAD_ACCESS (code=2, address=0x16f603ff8)
  * frame #0: libsystem_pthread.dylib`___chkstk_darwin + 60
    frame #1: mutool`pdf_recolor_shade + 64
    frame #2: mutool`<static> + 204
    frame #3: mutool`<static> + 420
    frame #4: mutool`<static> + 1660
    frame #5: mutool`<static> + 404
    frame #6: mutool`pdf_process_raw_contents + 212
    frame #7: mutool`pdf_process_contents + 128
    frame #8: mutool`<static> + 472
    frame #9: mutool`pdf_filter_page_contents + 144
    frame #10: mutool`<static> + 280
    frame #11: mutool`pdf_recolor_page + 136
    frame #12: mutool`pdfrecolor_main + 576
    frame #13: mutool`main + 492
    frame #14: dyld`start + 6992
```

The prologue of `pdf_recolor_shade` requests 8,455,008 bytes (arm64):

```
<+44>: mov  w9, #0x360
<+48>: movk w9, #0x81, lsl #16     ; w9 = 0x00810360 = 8455008
<+60>: blr  x16                    ; ___chkstk_darwin -> EXC_BAD_ACCESS
<+64>: sub  sp, sp, #0x810, lsl #12
<+68>: sub  sp, sp, #0x360
```

That is 82,784 bytes more than the 8,372,224-byte default main-thread stack.

The allocation comes from `source/pdf/pdf-shade-recolor.c`:

```c
#define FUNSEGS 256 /* size of sampled mesh for function-based shadings */

static void
fz_recolor_shade_type1(fz_context *ctx, pdf_obj *shade, pdf_function **func, recolor_details *rd)
{
	...
	float out[(FUNSEGS+1)*(FUNSEGS+1)*FZ_MAX_COLORS];
```

With `FZ_MAX_COLORS == 32`, that array is
257 * 257 * 32 * 4 = 8,454,272 bytes -- 99.99% of the observed frame.

`fz_recolor_shade_type1` is inlined into `pdf_recolor_shade`, so the array is
reserved unconditionally in the prologue, before the shading type is examined.
This is why shadings that never reach the type-1 code path still crash: all
three PDFs I reproduced with contain only `/ShadingType 2` and `/ShadingType 3`,
and none contains a `/ShadingType 1`.

Raising the stack limit confirms the diagnosis:

```sh
$ ( ulimit -s 8176  && mutool recolor -c gray -o out.pdf input.pdf ); echo $?
139
$ ( ulimit -s 16384 && mutool recolor -c gray -o out.pdf input.pdf ); echo $?
0
```

At 16 MB the same files convert successfully and produce valid output.

## Suggested fix

Heap-allocate `out` instead of placing it on the stack, e.g.
`fz_malloc_array(ctx, (FUNSEGS+1)*(FUNSEGS+1)*FZ_MAX_COLORS, float)` with a
matching `fz_free` in an `fz_always` block. Sizing it by the actual
`rd->dst_cs->n` rather than `FZ_MAX_COLORS` would also cut the allocation by
roughly an order of magnitude in the common case.

Moving the array out of `pdf_recolor_shade`'s inlined prologue alone would be
enough to stop non-type-1 shadings from crashing, but function-based shadings
would still overflow, so heap allocation looks like the right fix.

## Impact

The failure is deterministic. Across 27 PDFs I had on hand, the 9 that contain
at least one shading all exited 139, the 18 that contain none all exited 0, and
all 27 exited 0 once the stack limit was raised to 32 MB. There were no
exceptions in either direction.

In a separate sample of 150 PDFs collected from the web, 16 (10.7%) crashed
`mutool recolor` this way on machine A. Files containing gradients are common
output from Chrome, LibreOffice, PowerPoint, Canva and Ghostscript.

---

## 投稿時のメモ

- 添付する再現ファイルは、リポジトリ同梱の以下を使う（いずれも自前生成で、
  第三者の著作物を含まない）:
  - `fixtures/chrome_invoice.pdf`（Skia/PDF m152、`/PatternType 2 /ShadingType 2`）
  - `fixtures/quartz_graphics.pdf`（macOS Quartz、`/ShadingType 2` と `3`）
- sumi については書かない。競合製品の宣伝と受け取られる書き方は避ける。
- 投稿後、Bug 番号をこのファイルと `docs/mutool-comparison-notes.md` に控える。
