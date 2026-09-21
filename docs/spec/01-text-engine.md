# Appendix A — Text engine specification (port of `inkex/text/parser.py`, `font_properties.py`, `remove_kerning.py`)

Refs: `P` = `$SI/inkex1_3_0/inkex/text/parser.py`, `FP` = `.../text/font_properties.py`, `U` = `.../text/utils.py`,
`RK` = `$SI/remove_kerning.py`, `C` = `.../text/cache.py`.

## A.0 Two structural decisions

1. **Model-only edits, one writer.** Upstream mutates XML at every stage and re-parses. But its final stage
   `make_clean_textelements` (RK:114 → P:2385–2444) regenerates every processed `<text>` from the model, so
   intermediate XML never reaches the output. Port: parse XML → model once; run all stages on the model;
   serialize each touched element once. Only XML side effects outside the writer: clip-path union on
   cross-element merges (RK:640–656); `textLength`/`lengthAdjust` removal + compensating transform (P:673–699);
   debug highlight rects.
2. **No usvg text internals.** usvg follows browser semantics (ignores `sodipodi:role="line"`, immutable paths).
   Use fontdb + rustybuzz + ttf-parser directly.

## A.1 Flattener text pipeline (`remove_kerning()` RK:64–119)

Called from `flatten_plots.py:398–408` after `deswitch` (`dhelpers.py:341–352`). Option gating
`flatten_plots.py:178–184`; justification map `{1:"middle",2:"start",3:"end",4:None}` (`:398`). Only `<text>`
enters editing (RK:81); `flowRoot` only enters the char table (RK:73–75).

```
0  make_char_table               collect + measure characters
1  ParsedText per element        depathologize XML, parse lines/chunks/chars
2  remove_textlength (if rm)     undo textLength
3  precalcs                      snapshot "parsed" (original) positions
4  make_next_chain               link chunks on same baseline (intra-element)
5  differential→absolute (if rm) dx/dy → per-char chunks; make_next_chain again
6  Remove_Manual_Kerning (if rm) merge adjacent chunks within element; then 1 chunk = 1 element
7  External_Merges               merge chunks across elements incl. sub/superscripts
8  Split_Distant_Chunks, Split_Distant_Intrachunk, Split_Lines (if splitdistant)
9  Change_Justification
10 Remove_Trailing_Leading_Spaces
11 Fix_Merge_Positions
12 make_clean_textelements       rewrite every element
```
Stages 6–7 use **parsed (original)** positions; 8–11 use **current** positions (RK:96–97).

### Constants (RK:28–51, P:83–86)
| name | value | meaning |
|---|---|---|
| NUM_SPACES | 1.0 | nominal gap between merged chunks, in space widths |
| XTOLEXT | 0.6 | x tol (space widths) for external merges |
| YTOLEXT | 0.1 | y tol (fraction of chunk max cap height) for external merges |
| XTOLMKN / XTOLMKP | 1.5 / 0.99 | left(overlap)/right tol for manual-kerning merge, space widths |
| YTOLMK | 0.01 | y tol for manual-kerning merge, fraction of max cap height |
| XTOLSPLIT | 0.5 | extra gap beyond one space that triggers a split |
| SUBSUPER_THR | 0.99 | sub/super must be < 99 % of base font size |
| SUBSUPER_YTHR | 1/3 | super baseline ≥ 1/3 up from base baseline to cap top; sub cap top ≥ 1/3 down |
| FONTSIZE_THR | 0.01 | relative font-size equality for same-line merges |
| XY_TOL | 1e-6 | fuzzy equality + output rounding |
| next-chain y tol | 0.001 uu | lines with same y (P:704) |
| angle tol | 0.001° | same rotation for external merges (RK:412) |
| numeric merge | 0.25 spaces | max gap when both lines are numbers (RK:459–462) |
| space-import swap | 0.01·spw | " " chunk with same x as next chunk (P:718) |

Helpers (RK:659–690): `isnumeric(s, countminus)` = strip, `−`→`-`, drop `,`, parse float (strict decimal/
exponent regex in Rust); `countminus` makes lone `-` numeric. `twospaces(a,b)`: a ends with two spaces, or a
ends with space and b starts with space, or b starts with two spaces. `trailing_leading(a,b)` = (# trailing
`' '` of a, # leading `' '` of b). `wstrip` strips `" \n\t\r"`.

### Stage 0 — character table (P:4328–4710, FP)
For every text run (text or tail) of every element: font style `fsty` = {font-family (each family quoted,
comma-joined), font-weight, font-style, font-stretch} of the run's specified style (FP:357–370; defaults
`sans-serif/normal/normal/normal`). Collect chars per `fsty` (always add `" "`), P:4352–4363. Resolve
`true_style(fsty)` = face actually picked (FP:373–381) and per char the face that has the glyph (FP:220–246:
primary if it covers the codepoint, else first fallback face that covers it, else None = unrendered). Record
`pchrset[csty][c]` = set of chars immediately preceding `c` in the same run with same true style, plus `" "`
(P:4375–4383) — the pair set for differential kerning. Then measure → `CProp` per (true style, char).

### Stage 1 — parsing `<text>` into lines/chunks/chars
**1a Depathologize (P:294–297):**
- `remove_position_overflows` P:4759–4835: x/y/dx/dy lists longer than the text → redistribute over following
  chars (rare; v1: truncate + stderr warning).
- `cleanup_whitespace` P:4853–4882: if inherited `xml:space` ≠ `preserve`: in each text/tail longer than 1
  char, collapse `[ \t\r\n\f\v]+` → one space, strip leading/trailing; keep one trailing space in a *text*
  that has element children and ended with whitespace; keep one leading space in a *tail* that started with
  whitespace. Then (non-flows, regardless of xml:space): in each whitespace run the first `\n`/`\r` becomes a
  space, further newlines dropped (P:4888–4906); a trailing newline in any leaf span's text (`len(el)==0`) and in any last-child tail is dropped rather than converted (P:4908–4912; `cleanup_returns` with `last_span`).
- `condense_comments` P:4914–4930: move comment tails onto previous sibling's tail / parent's text.

**1b Text-run sequence** (P:2453–2516): pre-order walk; emit `Text(node)` on entry, `Tail(node)` on exit
(skip root's tail; skip comment text but not comment tails). Style source = node for text, parent for tails.
`CharLoc {node, is_tail, index}`.

**1c Positions per element** (P:379–469). `get_xy` (P:821–827): split on whitespace, `"none"` → None, else
unit→px (`units.py:31–44`, unitless = px). Effective `sodipodi:role="line"` (`esprl`, P:384–394): tspan is a
*direct child* of `<text>`, has role=line, exactly one x and one y; disabled if it has no own text and a
descendant with own text has x or y. Inactive roles stripped (P:396–401): everything downstream must read the
*pruned* flag, i.e. `esprl`, not the raw attribute (the anchor rule of 1d is the one place this shows). The
attribute-removal half of P:396–401 mutates the document, so it is deferred to Plan 4's writer; a measure-only
pipeline needs only the pruned flag. Types (P:403–416): `NORMAL` (not
esprl); `PRECEDEDSPRL` (esprl but preceded by a non-None tail, or first child while `<text>` has own text —
**continues the previous line**); `TLVLSPRL` otherwise. Missing x/y inheritance (P:419–469): walk
parent/child links across empty-text non-esprl elements; prefer ancestors, then nearest; record `xsrc/ysrc`.
`<text>` (index 0) defaults to `[0]`.

**1d Line creation** (P:477–585). A run with text (or a new TLVLSPRL even if empty) starts a **new line**
when: no lines yet; or run is `Text` and (TLVLSPRL, or NORMAL with x or y present after inheritance).
- New TLVLSPRL line: `x=[sprl_inherits.x[0]]`, `y=[sprl_inherits.y[0] + lht/scf]`, where `sprl_inherits` =
  first line or last TLVLSPRL line; `lht = max(composed_lineheight(tspan), composed_lineheight(<text>))`
  (U:90–114: `normal`→1.25, `%`→/100, unitless/em→number, length→`ipx(v)/font-size`; × transformed font
  size); `scf` = sqrt|det| of composed transform.
- New NORMAL line: x/y from element (inherited); x None → `continuex` (x = end of previous line, P:2640–2653:
  `anchor_x = (1+anfr)·prevlast.right − anfr·prevlast.left` — replicate, verify: risk 7); y None →
  `continuey` (previous line's last y).
- Anchor (P:543–557): line style `text-anchor` (default start); a non-sprl line after the first inherits the
  previous line's anchor; `direction:rtl` swaps start/end. `anfr` = {start:0, middle:0.5, end:1}.

**1e Characters** (P:587–628). Per char: `(fsz, scf, utfs) = composed_width(sel,"font-size")` (U:45–87:
`%`/`em` against nearest ancestor that set it; small/medium/large = 10/12/14 px; other keywords → 12 px;
`tfs = utfs·scf`); `csty = font_picker` (P:727–754: non-space → true style; a space between two chars both
rendered by the same *fallback* face takes that face; at run edges takes neighbour's); `prop = ctable[csty][c]`;
`dx[j]`, `dy[j]` from element lists (Text runs only; missing → 0). `cwd = prop.charw·utfs`,
`caph = prop.caph·utfs`, `spw = prop.spacew·utfs`, `lsp` = letter-spacing (P:3928–3940: em→×utfs, else ipx,
normal→0), `bshft` = baseline shift (P:3962–4019: walk up while `baseline-shift` in specified style; outermost
down: element *specifies* → add local (`super`→+40 %, `sub`→−20 % of **parent's** untransformed font size,
`%` × parent size, length → ipx); only *inherits* → add running sum again — Inkscape's compounding; keep).
All chars of one run share the first char's `lsp/bshft` (P:623–628). `dadvs(cL,cR) = prop.dadvs[(cL,cR)]·utfs`
only if both chars in the same text/tail node (P:3803).

**1f Chunks** (P:2696–2716): within a line a new chunk starts at char i>0 whenever `x[i]` or `y[i]` is not
None; chunk x/y = `x[i]/y[i]` (fallback: last list entry). Chunk lists (P:2886–2980): `cwd, dy, caph, bshft,
lsp`; `dx = [c0.dx … c(n−1).dx, 0]` (n+1); `dxlsp = [0, lsp0, lsp1, …]` (letter-spacing added *before* char
i); `dadv[0]=0, dadv[i]=dadvs(c[i−1],c[i])`.

**1g Flags:** `isflow` (P:286–290: flowRoot, `shape-inside` link, or nonzero `inline-size`); `isinkscape`
(P:308–317: the set of top-level lines after the first is **non-empty** and every one of them is sprl, **and**
every line's style has `-inkscape-font-specification`). The non-emptiness is load-bearing and easy to lose:
Python evaluates `all(… for line in tlvllns) and tlvllns and …`, where an empty `tlvllns` list is falsy, so a
single-line element is never `isinkscape` even though `all([])` is `True`. `ismlinkscape` = isinkscape ∧
>1 lines. Empty lines pruned (P:642–644).
`textLength` (P:648–671): `spacingAndGlyphs` → scale all `cwd` by `textLength/Σwidths`; else add
`(textLength − Σwidths)/(nchars − nchunks)` to every `lsp`.

### Geometry (P:3581–3651; vectorized P:109–250)
```
wds[i]  = cwd[i] + dx[i] + dxlsp[i] + (dx[i]==0 ? dadv[i] : 0)     // any dx overrides pair kerning
cstop   = prefix_sum(wds);  cstrt[i] = cstop[i] − cwd[i]
offx    = −anfr·(cstop[n−1] − (unrenderedspace ? cwd[n−1] : 0) − (rtl ? 2·Σdx : 0))
left[i] = x + cstrt[i] + offx;  right[i] = x + cstop[i] + offx
base[i] = y + prefix_sum(dy)[i] − bshft[i];  top[i] = base[i] − caph[i]
char pts_ut = [(left,base),(left,top),(right,top),(right,base)]        // BL, TL, TR, BR
chunk: lx2 = min_i(left[i]) − dx[0] − dxlsp[0] (P:3640); rx2 = lx2 + (right[n−1] − left[0]); by2 = max base; ty2 = min top
pts_t = composed_transform ∘ pts_ut
```
Upstream is internally inconsistent here: the **scalar** `pts_ut` (P:3689–3692) uses `chkw = rgtx[−1] − lftx[0]`,
i.e. `right[n−1] − left[0]`, while the **vectorised** path (P:180–181) uses `cstop[n−1]` — the two differ by
`dx[0]` whenever `dx[0] ≠ 0`. The scalar form above is the one ported and the one Plan 4 must use for
`get_ut_pts`.

`unrenderedspace` (P:3560–3579): chunk has >1 chars, its last char is the line's last and is `" "`/NBSP.
Ink bbox of a char (P:4202–4210): `x = left + inkbb.x·utfs`, `y_bottom = base + (inkbb.y+inkbb.h)·utfs`,
size `inkbb.w·utfs × inkbb.h·utfs`. Extent APIs P:1690–1793: `get_full_extent` = union of char extents
(skip NaN y); `get_full_inkbbox` = union of ink boxes. `get_ut_pts(w, w2)` (P:3421–3454): `tr1, br1` = TR/BR
of w's rightmost char (w's frame); `bl2, tl2` = BL/TL of w2's leftmost char from w2's **transformed** points
mapped through inverse of **w's** transform.

### Stage 2 — `remove_textlength` (P:673–699), only when `removemanual`
`spacingAndGlyphs`: restore `cwd`, append `translate(cx_with,0)·scale(adj,1)·translate(−cx_without,0)` to the
element transform. `spacing`: keep adjusted `lsp`. Remove `textLength`/`lengthAdjust`.

### Stage 3 — `precalcs`: snapshot `parsed_pts_ut` / `parsed_pts_t` per char.

### Stage 4 — next chain (P:701–725)
Per element: group lines by `y[0]` (tol 0.001); all chunks of those lines sorted by centre
`(pts_ut[0].x + pts_ut[3].x)/2`; if a chunk is exactly `" "` and its left edge equals the next chunk's within
`0.01·spw`, swap (PDF-import bug); link `prev.nextw/next.prevw`, `prevsametspan = (prev.last.loc.sel ==
next.first.loc.sel)`.

### Stage 5 — differential → absolute kerning (P:1665–1687, `write_axay` P:865–957)
Skip flows and elements with no nonzero dx. Per line: fuse positions onto the first char's element. For each
char j: if `|dx|>XY_TOL`: `lc` = index of last char before the next char with nonzero dx (or last);
`ax = left[j]·(1−anfr) + right[lc]·anfr`; dx=0. Else `ax = chunk.x` if j==0 else None. If `|dy|>XY_TOL`:
`ay = base[j]`, dy=0; else `ay = chunk.y` if j==0 else None. Rebuild x/y lists; a coordinate that follows a
character without one opens a new **line** (P:886–893), any coordinate opens a new chunk (P:2704–2716); a
new line without x continues from the end of the previous line, without y takes the previous line's last y.
Recompute next chain.

### Stage 6 — `Remove_Manual_Kerning` (RK:318–376)
For each chunk w with `w2 = w.nextw` (processed, and not `twospaces(w.txt,w2.txt)`):
```
trl, ldg = trailing_leading(w.txt, w2.txt)
dx = spw·(1 − trl − ldg);  if isnumeric(w.txt) && isnumeric(w2.txt, countminus) { dx = 0 }
xtoln = 1.5·spw; xtolp = 0.99·spw; ytol = 0.01·mch      // spw = max spw in w, mch = max caph in w
(tr1,br1,tl2,bl2) = w.get_ut_pts(w2)  (parsed)
valid = br1.x − xtoln ≤ bl2.x ≤ br1.x + dx + xtolp  &&  |bl2.y − br1.y| ≤ ytol
if w.txt == " " && w.prevw exists && !valid:            // weirdly kerned space
    (…,br1p,…,bl2p) = w.prevw.get_ut_pts(w2); dx = spw·(1−trl−ldg+1)
    valid = br1p.x − xtoln ≤ bl2p.x ≤ br1p.x + dx + xtolp   // no y test on retry
if valid: candidate (w2, "same", br1, bl2)
```
Then `Perform_Merges(mk=true)`. Afterwards every line with >1 chunk is split so **each chunk becomes its own
`<text>`** (RK:363–375, via `split_off_characters`).

### `Perform_Merges` (RK:535–656)
1. Each chunk keeps the candidate with smallest `|bl2.x − br1.x|`.
2. Chains from unmerged chunks following merge links.
3. Type state machine: `normal`→`sub`/`super` on `sub`/`super`/`suborsuperreturn`/`superorsubreturn`;
   `super`→`normal` on `superreturn`/`suborsuperreturn`; `sub`→`normal` on `subreturn`/`superorsubreturn`;
   `same` keeps state; anything else → chain dropped. Each merged chunk gets `wtype ∈ {normal, sub, super}`.
4. `maxspaces = 0` if (mk ∧ combined text contains a space ∧ `mrg.prevsametspan`) or w ends with `" "` or
   wtype is sub/super; else unlimited. `w.append_chks(...)`.
5. Clip union when merged elements have differing `clip-path`s: any None → remove clip; else duplicate main
   clip, append each other clip's contents grouped and transformed by `(main_ctm)⁻¹·other_ctm` (RK:640–656).

### `append_chks` (P:3120–3326)
Map merged chars' `parsed_pts_t` into w's frame. For each incoming chunk: `bl2x` = min parsed left of its
chars; `br1x` = max parsed right of w (first) or previous incoming chunk; `numsp = clamp(round((bl2x−br1x)/
lchr.spw), 0, maxspaces)` (`lchr` = w's last char). Insert `numsp` spaces (copies of lchr with `c=' '`,
`dx = −lchr.lsp`, `dy = 0`), then the chars. First char of each incoming chunk gets `dx = −prev.lsp`; others
keep dx (P:3297–3299). If `anfr ≠ 0`: `line.x[first_char_of_w] += anfr·Σ(cwd+dx of new chars)`
(P:3300–3303). Styles of moved chars (P:3266–3293): `ntype` = existing `baseline-shift` if super/sub and
stype normal, else stype. If ntype ∈ {super, sub}: `baseline-shift: super|sub` and `font-size: 65%`
(Inkscape's convention). Else if transformed size differs from new home by > 1e-4: `font-size:
round(c.tfs/fsz·100)%`. Else if style differs: `font-size: 100%` + copied style. Model: set
`bshft = ±0.4/−0.2·utfs_parent`, `utfs = 0.65·parent`, keep style diff; writer emits tspans. Prune empty
source lines/elements.

### Stage 7 — `External_Merges` (RK:382–532)
Candidates: all chunks of processed elements. `bb` = bbox of chars' `parsed_pts_t`; `bb_big` = bb grown by
`spw·scf·(1+0.6)` on all sides. Pairs (w,w2), w≠w2, `|angle_w − angle_w2| < 0.001°` (`angle = atan2(c,d)` of
the transform, P:2635–2638) and `w.bb_big ∩ w2.bb`. In w's frame:
```
trl,ldg = trailing_leading;  dx = spw·(1−trl−ldg);  xtol = 0.6·spw;  ytol = 0.1·mch
w1fs = (utfs·√(a²+b²), utfs·√(c²+d²)) per chunk
(tr1,br1,tl2,bl2) = w.get_ut_pts(w2)  (parsed)
xpen = br1.x − xtol ≤ bl2.x ≤ br1.x + dx + xtol
go   = xpen && wstrip(w.txt)≠"" && wstrip(w2.txt)≠"" && !twospaces
weight_match = w.last.tsty[font-weight] == w2.first.tsty[font-weight];  letterinpar = w.txt ~ ^\([a-zA-Z]\)$
SAME : |bl2.y−br1.y| < ytol && |w1fs−w2fs| < 1% (both axes) && mergenearby
       → if isnumeric(w.line.txt) && isnumeric(w2.line.txt, countminus): only if |(bl2.x−br1.x)/spw| < 0.25
SUPER: br1.y+ytol ≥ bl2.y ≥ tr1.y−ytol && mergesupersub && weight_match && !letterinpar
       aboveline = bl2.y ≤ br1.y·(2/3) + tr1.y·(1/3) + ytol
       w2.tfs < 0.99·w.tfs && aboveline → "super";  w.tfs < 0.99·w2.tfs → "subreturn"
SUB  : br1.y+ytol ≥ tl2.y ≥ tr1.y−ytol && mergesupersub && weight_match && !letterinpar
       belowline = tl2.y ≥ br1.y·(1/3) + tr1.y·(2/3) − ytol
       w2.tfs < 0.99·w.tfs && belowline → "sub";    w.tfs < 0.99·w2.tfs → "superreturn"
```
Then `Perform_Merges(mk=false)`. Incoming element's chars re-expressed in target's frame with `font-size:%`
corrections when scale differs (P:3283–3289).

### Stage 8 — splits
- `Split_Distant_Chunks` (RK:205–253): per line, chunks sorted by x; split before chunk ii if
  `bl2.x > br1.x + spw·(1−trl−ldg) + 0.5·spw` (current points). Each split range → new element.
- `Split_Distant_Intrachunk` (RK:257–315), skip ismlinkscape/flow: per chunk, chars sorted by current left;
  compare each char c2 to last non-space char c: split if `bl2.x > br1.x + 1.5·spw` **or** `numbersplit` =
  `isnumeric(chunk.txt[prevsplit:ii])` ∧ `c2 ∈ {" ","-","−"}` ∧ remainder starts numeric ∧ same XML node
  (tick labels like "0.5 0.1").
- `Split_Lines` (RK:183–201), skip ismlinkscape/flow: every line after the first → own element.

`split_off_characters` (P:1258–1440): partition requested chars into runs contiguous within a chunk; each run →
new element at `x = anfr·max_right + (1−anfr)·min_left` of the run's current extents, `y` = first char's
baseline (P:1207–1210), anchor/transform copied; nested style tspans where `sty`, `utfs` or `bshft` change.
Remaining chunk: recompute `dadv` for newly adjacent chars (P:1276–1285), then fix positions (P:1404–1431):
`err[i] = old_left[i] − new_left[i]`; `Δ[i] = err[i]−err[i−1]` → `dx[i] += Δ[i]` for i>0; chunk
`x += err[0] − (−anfr·ΣΔ[1:])`; same for y (`dy[i] += Δy[i]`). New elements inserted right after the source.

### Stage 9 — `Change_Justification` (RK:167–179, P:2751–2787)
Skip ismlinkscape/flow/None. Per line with `newanch ≠ anchor`: per chunk `minx/maxx` of current x's,
`maxx −= last.cwd` if unrenderedspace and chunk holds the line's last char; `newx = (1−anfr_new)·minx +
anfr_new·maxx`; set chunk x, anchor, `text-anchor`/`text-align` (`middle`→`center`) on the chunk's first-char
element and on `<text>` (RK:175–178).

### Stage 10 — `Remove_Trailing_Leading_Spaces` (RK:139–159), skip ismlinkscape/flow
Delete trailing then leading `' '` chars of each line. Deleting a char (P:4029–4118) shifts the chunk anchor
so the rest stays: `cwo = cwd + (dko1 + dko2 − dkn) + dx + (windex≠0 ? lsp : 0)`; `cwo = tdk` if it was the
unrendered trailing space; first-of-chunk → `x −= (anfr−1)·cwo`, else `x −= anfr·cwo`.

### Stage 11 — `Fix_Merge_Positions` (P:3505–3529)
Per chunk over non-space chars: `deltaanch = ((1−anfr)·new_minx + anfr·new_maxx) − ((1−anfr)·parsed_minx +
anfr·parsed_maxx)`; `x −= deltaanch`.

### Stage 12 — writer `make_clean_textelement` (P:2385–2444) + `make_tspan` (P:3456–3503)
Element deleted if no chars. Else new `<text xml:space="preserve">` inserted after the old, old deleted,
**id reused**; attributes copied except `baseline-shift, shape-inside, direction, style, font-family`;
style = old *local* style minus `{baseline-shift, shape-inside, direction}` plus `font-family:'<true family of
first char>'`. One `<tspan>` per chunk (NaN-y chunks dropped): `x`, `y`, `dx`/`dy` = chunk lists with trailing
zeros trimmed (omit if all zero); style = specified style of first char written as a **diff against the
`<text>`'s specified style** (C:206–221), plus `font-size:<chunk max utfs>`, `text-align`, `text-anchor`,
minus `{line-height, direction, baseline-shift, shape-inside}`. If styles/utfs/bshft differ along the chunk or
first char has `|bshft|>XY_TOL`: nested `<tspan>` per run with `font-size: round(fs/chunk_utfs·100, 3)%`
(omit if equal within 1e-3) and `baseline-shift: super` if `bs/utfs ≈ 0.4`, `sub` if `≈ −0.2`, else
`round(bs/utfs·100,3)%`; drop attributes equal to parent tspan's specified style and `{text-align,
text-anchor, direction, shape-inside}`. Finally, if all chunk x's are equal and consecutive y-gaps divided by
`max(fsz[i+1], min fsz)` are all equal (tol 0.001): `<text>` gets `font-size:<min utfs>`,
`line-height:<(y1−y0)/max(fsz1,min)>` (1.25 for a single line), first chunk's x/y, and every tspan gets
`sodipodi:role="line"` — so Inkscape's on-load re-layout reproduces the same y's. Numbers: round to 1e-6,
shortest repr, `-0` → `0` (P:4749–4757); emitted **lengths** carry their unit — `font-size` (on the `<text>`
and on every chunk `<tspan>`) and `letter-spacing` (stage 2) are written as `<number>px`, where upstream
writes a bare number (see "Deliberate defensive deviations"). Ratios and percentages are unitless as before:
`line-height`, and the nested-tspan `font-size: …%`.

### Sub/superscripts summary
Detected geometrically in stage 7; represented as `TChar.bshft` (uu) + `TChar.utfs`; written as nested
`<tspan style="font-size:65.0%;baseline-shift:super">` (65 % fixed by upstream on merge, P:3282). Homogenizer
(`homogenizer.py:189–199`) detects relative text as `bshftfunc ≠ 0` or `%` in own `font-size` and rewrites as
`font-size: dfs/pfs·100 %` of parent.

## A.2 Rust text model

Dependencies on the document layer: arena DOM preserving text/tail verbatim; `specified_style(node)` =
parent specified + own cascaded (presentation attrs < `<style>` sheet rules < `style` attr, C:338–364), with
`font:` shorthand expansion (C:239–264; matplotlib emits `style="font: 10px 'DejaVu Sans'"`);
`composed_transform(node) -> Affine`; inherited `xml:space`; id map; clip-path/href resolution.

```rust
pub type NodeId = u32;
pub struct CharLoc { pub node: NodeId, pub tail: bool, pub idx: u32 }
pub struct TChar {
    pub c: char, pub loc: CharLoc, pub sty: Arc<Style>, pub fsty: FontSpec, pub face: Option<FaceKey>,
    pub prop: Arc<CProp>, pub utfs: f64, pub tfs: f64,
    pub cwd: f64, pub caph: f64, pub spw: f64, pub dx: f64, pub dy: f64, pub lsp: f64, pub bshft: f64,
    pub parsed_ut: Option<[Point; 4]>, pub parsed_t: Option<[Point; 4]>,
}
pub struct TChunk { pub x: f64, pub y: f64, pub chars: Vec<CharId>, pub next: Option<ChunkRef>,
                    pub prev: Option<ChunkRef>, pub prev_same_tspan: bool, cache: OnceCell<ChunkGeom> }
pub struct TLine  { pub x: Vec<Option<f64>>, pub y: Vec<Option<f64>>, pub xsrc: NodeId, pub ysrc: NodeId,
                    pub sprl: bool, pub anchor: Anchor, pub rtl: bool, pub transform: Affine,
                    pub tlvlno: Option<usize>, pub style: Arc<Style>, pub continue_x: bool,
                    pub continue_y: bool, pub chunks: Vec<TChunk> }
pub struct ParsedText { pub el: NodeId, pub chars: Vec<TChar>, pub lines: Vec<TLine>, pub is_flow: bool,
                        pub is_inkscape: bool, pub is_ml_inkscape: bool,
                        pub text_length: Option<TextLengthAdj>, pub transform: Affine }
pub enum Anchor { Start, Middle, End }   // anfr() -> 0.0 | 0.5 | 1.0
```
Arena + indices; `ChunkRef = (line_idx, chunk_idx)`; `CharId -> (line, chunk, pos)` reverse index rebuilt
after structural edits. Geometry lazy per chunk, invalidated on edit. Positions in the element's untransformed
frame; cross-element comparisons via `parsed_t` + `transform.inverse()`.

Not handled (same as upstream; warn on stderr): `word-spacing` (cheap to add, off by default),
`dominant-baseline`, `alignment-baseline`, vertical writing-mode, `unicode-bidi`, `<textPath>` (skip
element), complex-script shaping.

**Flowed text v1:** `flowRoot` never edited; bbox = flowRegion rect (approximate, flagged). SVG2
`shape-inside`/`inline-size`: Inkscape writes fallback `<tspan x y>` lines and re-flows on load, so parse
the fallback tspans as positioned text for **bbox only**, exclude from stages 5–11 (deliberate deviation:
upstream lets flows participate in merges). Do not port `parse_lines_flow` (P:1808–2383) in v1.

### Deliberate defensive deviations from upstream

The port is otherwise a faithful transcription, so record the places where it is *deliberately* safer
than the Python — a future parity reviewer must not "correct" them back:

- `get_xy` on a whitespace-only attribute (`x=" "`) returns `[None]`; upstream returns `[]` and the next
  `xvs[i][0]` raises `IndexError`.
- Chunk x/y carry the last non-`None` coordinate forward when a list entry is `None`; upstream would put
  `None` into the arithmetic.
- `local_baseline` resolves a `%`/`super`/`sub` shift against the parent's **`utfs`**; upstream's `fs2/sf2`
  is `0/0 = NaN` under a singular (e.g. `scale(0)`) parent transform.
- `continue_x`/`continue_y` and stage-5 continuation coordinates are resolved once when the model is built
  (upstream recomputes them on every access).
- `change_alignment` measures every chunk with the old anchor before moving any (upstream mutates the
  anchor inside its per-chunk loop, P:2785–2787).
- Merged sub/superscript characters get `bshft = ±0.4/−0.2·host utfs` and `utfs = 0.65·host utfs` in the
  model at merge time (upstream leaves the model stale until Inkscape re-renders).
- Inserted spaces carry no parsed points (upstream copies the neighbour's); `split_off` always shifts the
  chunk anchor (upstream skips chunks without an own `x` entry).
- Split-off elements are written in creation order directly after their own source (pre-order via
  `ParsedText.split_src`; upstream `addnext`s every new element on the source, which emits the runs of one
  range reversed and cannot place a split-off of a split-off after its own source); emptied elements are
  removed; `remove_kerning` returns live node ids instead of upstream's stale handles.
- Stage 5 positions the first character of every chunk with the anchor-weighted formula (upstream keeps
  the old chunk `x`, displacing middle/end-anchored first segments until stage 11).
- The regenerated `<text>` does not inherit the old element's `x`/`y`/`dx`/`dy`/`rotate` lists (upstream
  copies every attribute; an ancestor list would re-apply to characters whose tspan list was trimmed);
  `xmlns:sodipodi` is declared on the root when `sodipodi:role` is written.
- Emitted `font-size` and `letter-spacing` carry `px` (`write.rs` `make_tspan` + the `role="line"` block,
  `edit.rs` `remove_textlength`); upstream writes `str(self.utfs)`, an invalid unitless CSS length that
  Chrome and Firefox drop — the whole element then renders at the inherited/initial 16 px. Inkscape and
  librsvg are lenient, so this changes nothing in Inkscape and fixes the browser case.
- `perform_merges` records a clip union only when at least one merged element carries a resolvable
  `clip-path` (upstream RK:640–656 runs after every cross-element merge; for unclipped participants its
  only action, clearing the target's absent clip, is a no-op); a dangling `clip-path` reference on an
  otherwise unclipped set is left alone (upstream would clear the target's).

## A.3 Metrics layer

`CProp` (P:4244–4287), em units: `charw` (advance of char in isolation: Pango `width("I="+c+"=I") −
width("I==I")`), `spacew` (advance of `' '`), `caph` (ink top of "I==I" ≈ yMax('I')), `dadvs[(prev,c)]`
(`width(prev+c) − width(prev) − width(c)`, covers GPOS kerning and ligatures; only pairs in `pchrset`),
`inkbb = (x, y, w, h)` of the bare char's ink rect relative to the pen at baseline, y-down; whitespace →
zero-size box at the pen.

```rust
pub struct FaceKey(u32);
pub struct LoadedFace { data: Arc<dyn AsRef<[u8]>+Send+Sync>, index: u32, info: FaceInfo }
pub struct FaceInfo { family: String, weight: u16, style: Style, stretch: Stretch, upem: f64,
                      ascent: f64, descent: f64 /* normalized asc+desc = 1 (FP:960–971) */,
                      ascent_max: f64, descent_max: f64, x_height: f64, cap_height: f64 }
```
- Load: `fontdb::Database::load_system_fonts()`; face bytes via `make_shared_face_data(id)`; re-parse
  `ttf_parser::Face::parse` / `rustybuzz::Face::from_slice` per cache miss (cheap; results cached at CProp).
- `FaceInfo` = port of `find_font_metrics` (FP:950–1007 ← Inkscape `font-instance.cpp`): typo ascender/
  descender else hhea; normalize; `x_height` = OS/2 sxHeight (v≥2) else glyph `x` else 0.5; `cap_height`:
  `yMax('I')/upem` first (matches Pango-based refs), then OS/2 sCapHeight, then 0.7.
- `charw(c)`: shape single char with rustybuzz (default features), sum `x_advance/upem`; glyph id → `inkbb`
  via `glyph_bounding_box` (glyf) or `outline_glyph` bbox (CFF). `pair(prev,c)` = `shape(prev+c).advance −
  charw(prev) − charw(c)`, cached. rustybuzz = HarfBuzz port = what Pango uses → agreement ~1e-4 em.
- Variable fonts: if `fvar` present and requested weight ≠ default, set `wght`/`wdth` via
  `set_variations`/`set_variation` before measuring (FP:880–894).

Font selection (`FontSpec { families, weight, style, stretch }`; weight map FP:1143–1168: numeric,
normal=400, bold=700, other keywords → 400):
1. Per family in order: fontdb `query(Query{ families:[Name(f)], weight, stretch, style })`.
2. Metric-compatible aliases (fontconfig `30-metric-aliases.conf`): Helvetica↔Arial↔Liberation Sans↔Nimbus
   Sans; Times↔Times New Roman↔Liberation Serif↔Nimbus Roman; Courier↔Courier New↔Liberation Mono↔Nimbus
   Mono; Calibri→Carlito; Cambria→Caladea; Georgia→Gelasio.
3. Generic families (`60-latin.conf` order): sans-serif → DejaVu Sans, Bitstream Vera Sans, Verdana, Arial,
   Albany AMT, Luxi Sans, Nimbus Sans L, Nimbus Sans, Helvetica, Lucida Sans Unicode, Tahoma, Noto Sans;
   serif → DejaVu Serif, Bitstream Vera Serif, Times New Roman, Thorndale AMT, Luxi Serif, Nimbus Roman No9
   L, Nimbus Roman, Times, Noto Serif; monospace → DejaVu Sans Mono, Bitstream Vera Sans Mono, Inconsolata,
   Andale Mono, Courier New, Cumberland AMT, Luxi Mono, Nimbus Mono L, Nimbus Mono PS, Courier, Noto Sans
   Mono. Unknown families fall through to sans-serif.
4. Last resort: first face with requested style; stderr warning `font-family "X" not installed; measured with
   "Y"`.
Per-char fallback (FP:220–246): if primary lacks glyph → remaining families of the spec → generic list →
curated wide-coverage list (Noto Sans / Noto Sans Symbols / Noto Sans Math, DejaVu Sans, Arial Unicode MS,
Segoe UI Symbol, Cambria Math, Apple Symbols, STIX Two Math, Symbola) → all faces sorted by (same style,
|weight diff|, family). Caches: `(FaceKey,char)→CProp`, `(FaceKey,char,char)→f64`, `FontSpec→FaceKey`,
`(FontSpec,char)→Option<FaceKey>`. No size dimension (scale by `utfs`).

Where exact Inkscape/Pango parity is impossible: font *selection* (biggest error source: whole-font mismatch →
5–15 % widths); Pango fallback itemization; synthetic bold/oblique; complex-script shaping; cap-height source.
Target when the same font file is found: per-char positions within 0.01 px at 12 px.

## A.4 Public API

```rust
// text/fonts.rs
pub struct FontSystem { db: fontdb::Database, faces: Vec<LoadedFace>, /* caches */ }
impl FontSystem {
    pub fn load_system() -> FontSystem;
    pub fn resolve(&mut self, spec: &FontSpec) -> FaceKey;
    pub fn resolve_for_char(&mut self, spec: &FontSpec, c: char) -> Option<FaceKey>;
    pub fn face_info(&self, k: FaceKey) -> &FaceInfo;
    pub fn prop(&mut self, k: FaceKey, c: char) -> Arc<CProp>;
    pub fn pair_adv(&mut self, k: FaceKey, prev: char, c: char) -> f64;   // em
}
pub fn font_spec(style: &Style) -> FontSpec;                                   // FP:357–370
// text/style.rs
pub fn composed_font_size(doc: &Doc, node: NodeId) -> FontSize { tfs, scf, utfs }   // U:45–87
pub fn composed_line_height(doc: &Doc, node: NodeId) -> f64;                        // U:90–114
pub fn baseline_shift(doc: &Doc, style: &Style, node: NodeId) -> f64;               // P:3962–4019
pub fn letter_spacing(style: &Style, utfs: f64) -> f64;                             // P:3928–3940
// text/table.rs
pub fn build_char_table(doc: &Doc, els: &[NodeId], fs: &mut FontSystem) -> CharTable;
// text/parse.rs + layout.rs
impl ParsedText {
    pub fn parse(doc: &mut Doc, el: NodeId, ct: &CharTable, fs: &mut FontSystem, o: ParseOpts) -> Option<ParsedText>;
    pub fn snapshot_parsed(&mut self);
    pub fn full_extent(&self, which: Which /*Current|Parsed*/) -> Option<Rect>;   // P:1776
    pub fn full_ink_bbox(&self) -> Option<Rect>;                                  // P:1701
    pub fn char_extents(&self) -> Vec<(usize, Rect)>;   // index into `chars`: NaN-baseline chars are skipped
    pub fn chunk_extents(&self) -> Vec<Rect>; pub fn line_extents(&self) -> Vec<Rect>;
    pub fn chars(&self) -> impl Iterator<Item = &TChar>;
    pub fn max_tfs(&self) -> Option<f64>;
}
pub fn text_bbox(doc: &Doc, el: NodeId, ct: &CharTable, fs: &mut FontSystem, which: Which) -> Option<Rect>;
// text/kerning.rs
pub struct KerningOptions { pub remove_manual: bool, pub merge_supersub: bool, pub split_distant: bool,
                            pub merge_nearby: bool, pub justification: Option<Anchor> }
pub fn remove_kerning(doc: &mut Doc, els: &[NodeId], o: &KerningOptions, fonts: FontSystem,
                       warn: &mut Warnings) -> Vec<NodeId>;
// text/write.rs
pub fn write_clean_text(doc: &mut Doc, pts: &[ParsedText], idx: usize, ct: &CharTable,
                         slots: &mut HashMap<usize, Slot>) -> Option<NodeId>;   // P:2385–2444
```
`snapshot_parsed` and `get_ut_pts` live in `text::layout`; the editing primitives (`remove_textlength`,
`make_next_chain`, `rechunk_absolute`, `split_off`, `change_alignment`, …) live in `text::edit`; the stage
drivers (`remove_manual_kerning`, `external_merges`, `split_distant_chunks`, `change_justification`, …) live
in `text::kerning`, which also assembles them into `remove_kerning`.

Consumers: Flattener → `remove_kerning`; Homogenizer → `chars()` (`tfs`, `bshft`, `utfs`), `baseline_shift`,
`composed_font_size`, `text_bbox` before/after restyle (`homogenizer.py:248–263`); Text Ghoster →
`text_bbox` + max `composed_font_size` over descendants (`text_ghoster.py:70–99`); Scaler/bbox code →
`text_bbox` (extent, not ink).

## A.5 Risks & de-risking experiments
1. **Font resolution parity** (highest impact): `sciink font-probe` printing fontdb's choice for every
   family/weight/stretch in the fixtures vs `fc-match` (Inkscape's bundled fontconfig at
   `/Applications/Inkscape.app/Contents/Resources/bin/fc-match` if present, else Homebrew) and Inkscape's
   Text & Font dialog for a handful.
2. **Advance/kerning parity**: (A) the `--debugparser` ref
   `tests/data/refs/flatten_plots__…--debugparser__True__Text_tests__svg.out` has 3684 `<rect>`s = per-char
   extents → oracle for `char_extents()` on locally installed fonts (DejaVu Sans, Arial, Tahoma);
   (B) `inkscape --export-text-to-path` on a matplotlib file, compare glyph-path ink bboxes with ours; target
   ≤ 0.02 em drift over a 20-char line.
3. Variable fonts (Bahnschrift, Roboto Flex): `set_variations` advances vs fontTools instancer.
4. Per-char fallback: fixture chars not covered by primary face (`−` U+2212 in Tahoma/Helvetica, `≥`, Greek,
   Cambria Math) — which face Inkscape uses (export PDF, `pdffonts`).
5. `load_system_fonts()` startup cost on macOS/Windows; if > 300 ms, cache keyed by dir mtimes (v2) or scan
   restricted to families named in the document + generics.
6. Writer round-trip through Inkscape 1.2–1.5: open, save, diff x/y of role=line tspans; `font-size:65.0%`
   and `baseline-shift:super` survive 1.5.
7. `continuex` anchor formula (P:2649–2651) looks non-physical for `middle` — verify in Inkscape first.
8. Whitespace rules vs Inkscape for default `xml:space` with newlines between tspans (svglite): 5 micro-SVGs,
   compare Inkscape PNG export vs our char count/positions.
9. Cascaded-style correctness: class stylesheets (`class="st38 st39"` in Text_tests.svg), `font:` shorthand,
   presentation attributes — unit tests against `cspecified_style` semantics (C:185–197, 338–364).
10. External_Merges O(n²) on ~5k chunks ≈ 25 M cheap checks — fine; y-bucketed sweep only if profiling says so.
11. Numeric formatting: 1e-6 rounding + shortest repr; `-0` → `0`.

## A.6 LOC and build order
| module | LOC | notes |
|---|---|---|
| text/style.rs | 350 | font-size/line-height/letter-spacing/baseline-shift, FontSpec, `font:` shorthand |
| text/fonts.rs | 450 | fontdb loading, resolver + alias tables, per-char fallback, FaceInfo |
| text/metrics.rs | 350 | CProp, rustybuzz/ttf-parser measurement, caches |
| text/table.rs | 150 | char/pair collection |
| text/whitespace.rs | 200 | depathologize |
| text/parse.rs | 650 | run sequence, x/y inheritance, sprl types, lines, chars, chunks, flags, textLength |
| text/layout.rs | 300 | chunk geometry, extents, snapshots, get_ut_pts, ink boxes |
| text/edit.rs | 600 | dx→abs, split runs, append chunks, delete chars, re-anchor, change alignment |
| text/kerning.rs | 650 | stages 4, 6–11, Perform_Merges, clip union |
| text/write.rs | 250 | clean writer, style diffing, number formatting |
| text/flow.rs | 80 | v1 detection/skip/approximate bbox |
| tests | 500 | fixture-driven |
| **total** | **≈ 4.5k** | vs ~7.5k Python |

Order: (1) style+fonts+metrics+table → `font-probe`/`text-metrics` debug subcommands, experiments 1–4;
(2) whitespace+parse+layout → `text_bbox`, `char_extents`, debug rects → unblocks Scaler/Ghoster/Homogenizer
re-centring; validate vs debugparser ref; (3) write + minimal edit → Flattener with splitdistant +
justification + trailing-space removal (useful on matplotlib/MATLAB/Excel SVGs); (4) edit (dx→abs, append,
re-anchor) + kerning stages 4–6 → PDF imports; (5) stage 7 (external merges, sub/super, clip union);
(6) Homogenizer char-size/bshft API, textLength, flow v1, word-spacing behind a flag.

---

