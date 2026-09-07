# Appendix B — Geometry, ops and tools specification (non-text)

`inkex/` = `$SI/inkex1_3_0/inkex/`. Contracts assumed from the DOM layer: `cascaded(n)` (own style =
presentation attrs [only names in `inkex/text/cache.py:267-332`, minus clip/clip-path/mask/transform] <
stylesheet rules < inline `style`; cache.py:337-364), `specified(n)` = `specified(parent) + cascaded(n)` with
`font` shorthand expansion (cache.py:185-197), `css_rule_value(n, prop)`, `set_inline`, subtree invalidation,
memoized `composed_transform(n)`, comments as nodes, `new_id(prefix)` = prefix + counter (cache.py:934-955).
From the text engine: `text_extent_local(el, parsed)` (union of char boxes in the element's own coordinates,
before `el.transform`, P:1776-1793), `max_char_font_size_pt(el)`, `remove_kerning`, `character_fixer`,
`inkscape_spec_to_css`.

## B.1 `geom` — primitives and numeric policy
kurbo `Affine::new([a,b,c,d,e,f])` == SVG `matrix(a b c d e f)`; upstream `A @ B` = kurbo `A * B`; `-A` =
`A.inverse()`.

| item | rule (upstream ref) |
|---|---|
| identity / transform equality | every component within **1e-5** (`inkex/transforms.py:360,611-622`); write no `transform` when identity |
| transform serialization | `translate(e, f)` if a≈1,d≈1,b≈c≈0; `scale(a, d)` if e≈f≈b≈c≈0; else `matrix(a b c d e f)` |
| singular transform | `inverse()` → None when \|det\| < 1e-12; callers skip (Ghoster skips element) |
| scale factor `sf` | `sqrt(|a·d − b·c|)` (`inkex/text/utils.py:86`) |
| `ipx(s)` | number+unit: in 96, pt 1.3333333, px 1, mm 3.7795276, cm, m, km, Q 0.9448819, pc 16, yd 3456, ft 1152, none→px; `%`/`em`/unknown → None (utils.py:565-582) |
| numbers | one formatter: 8 significant digits, trailing zeros trimmed, `-0`→`0`, no exponent in 1e-8..1e15 |
| paths | `parse_d(&str) -> ParsedPath { path: BezPath, cmd_start: Vec<usize> }`: svgtypes → absolute; H/V→L; S/T→C/Q; A→cubics via `kurbo::Arc::from_svg_arc` + `append_iter(1e-4)`; degenerate arcs (rx/ry==0 or start==end) → line (`dhelpers.py:1801`). `cmd_start[i]` = index of first BezPath element of source command i (len ncmds+1). Serialize absolute `M L Q C Z`. **Never re-serialize a `d` we did not modify** |
| shape geometry (`cpath`) | rect: `M l,t h w v h h −w z`, rounded if rx or ry (rx=min(rx>0?rx:ry, w/2), ry likewise; 4 lines + 4 arcs, cache.py:426-450); circle/ellipse: `M cx,cy−ry a rx,ry 0 1 0 rx,ry a rx,ry 0 0 0 −rx,−ry z`; line `M x1,y1 L x2,y2`; polyline `M points`; polygon `M points Z` |
| end points | one per command; `Z` yields subpath start (`inkex/paths.py:1446-1457`) |
| bbox | exact: `BezPath::bounding_box()`; rough: `control_box()` (control points; arcs → cubics first, dhelpers.py:1482-1494) |
| `Option<Rect>` algebra | `union` null-absorbing (utils.py:655-667). `intersection(a,b)`: if a null returns **b** (quirk, :669-681); empty when maxx<minx or maxy<miny (touching → zero-size). `transform`: bbox of 4 transformed corners. `intersects`: strict `|xc1−xc2|·2 < w1+w2 && |yc1−yc2|·2 < h1+h2` (:649-653) |
| `uniquetol(xs, tol)` | sort, keep first, append when `> tol` from the **last kept**; return count (utils.py:130-153) |

## B.2 `ops` — algorithms

**Style composition** (`compose_all` style step, dhelpers.py:359-366): `child.inline := cascaded(group)
overridden by cascaded(child)`; `opacity := opacity(child,1) × opacity(group,1)`. Written as the full merged
map into the child's `style` attribute (materializes CSS-class styles during flattening).

**Clip/mask merging** (`merge_clipmask` :445-504; `compose_clips` :434-443; `intersect_paths` :416-429).
`merge(node, newclip, kind) -> clipped_out`:
1. If node has non-identity `transform` T: duplicate newclip into root `<defs>` (record in `created`); for
   each child k: `k.transform := T⁻¹ · k.transform` (:450-461).
2. Unlink every `<use>` child of newclip (:463-466) (Acid_tests has 39 clipPaths starting with `<use>`).
3. `old` := node's existing clip/mask of same kind. None → set `clip-path/mask = url(#newclip)`; return false.
4. Else: unlink `<use>` children of old; `d := duplicate(old)` (recorded); point node at d. `newclipisrect :=
   len(newclip)==1 && is_rectangle(newclip[0], including_transform=true)`. For each child k of d (reverse):
   - if `newclipisrect && is_rectangle(k) && kind==Clip`: intersect axis-aligned boxes of end points of
     `path(newclip[0])·newclip[0].transform` and `path(k)·k.transform`; if w>0 && h>0 append
     `<path d="M x1,y1 h w v h h −w Z">` to d and delete k → not clipped out; else delete k → clipped out.
   - else recurse `merge(k, newclip, kind)` (nested clipping).
   - return all(children clipped out). Masks never rectangle-intersected.

**`fix_css_clipmask`** (:397-413): if the stylesheet supplies clip-path/mask for the node differing from the
attribute, append `\n#<id>{clip-path:<attr value>}` to the root `<style>` (create as first child of `<svg>` if
absent, cache.py:915-927); delete the property from inline style. (Risk R1: verify precedence in Inkscape.)

**`compose_all(el, clip, mask, T, style, remove_text_clip)`** (:359-391): style first; then if
`remove_text_clip && el is text/flowRoot` → drop clip-path and mask, not clipped out; else `cout = merge(el,
clip, Clip)` if clip, `merge(el, mask, Mask)` if mask, `fix_css` per kind used; then if T not identity:
`el.transform := T · el.transform`. Return cout only when a clip was given.

**`ungroup(g, remove_text_clip)`** (:292-316): children in reverse: comments removed; namedview/defs/metadata/
foreignObject stay inside g; others: `compose_all(child, g.clip, g.mask, g.transform, cascaded(g),
remove_text_clip)`; copy `xml:space` from g if child lacks it; clipped out → delete, else move to right after
g. Delete g if empty. `group(els)` (:320-338) inserts a new `<g>` after the first element, appends the rest.

**`deswitch(sw)`** (:341-352): keep first child whose `systemLanguage` matches UI language (fallback first
child), delete the rest, remove its `systemLanguage`, ungroup. Language: `INKSCAPE_PROFILE_DIR/
preferences.xml` `<group id="ui" language>`, else `$LANG` prefix, else `en`.

**Unlink clones** (`unlink2` :238-282): target via `href`/`xlink:href`; missing → delete use, None. Else `d :=
duplicate(target)` (new ids for all descendants); `compose_all(d, None, None, translate(x,y), None)` with use's
x/y (default 0), then `compose_all(d, use.clip, use.mask, use.transform, cascaded(use))`. Move d to use's
position, give it use's id, delete use; set `unlinked_clone="True"`; recurse into descendants (nested uses).
If d is `<symbol>`: wrap children in `<g>`, ungroup the symbol (viewBox ignored, as Inkscape's Unlink Clone).

**Bounding boxes** (`bounding_box2` :1426-1552): `bbox(el, opts{transform, stroke, rough, parsed, clip})`,
memoized per (node, opts). Local result → clip → transform:
- text/flowRoot: `text_extent_local(el, parsed)`.
- shapes with non-empty path: `swd = ipx(specified stroke-width, default "0px")`; `swd = 0` if stroke
  none/absent or `!stroke` (:1454-1458; `%` widths → 0). `line`: box of endpoints; else exact/rough path
  box; expand by swd/2.
- group-like (svg, g, clipPath, symbol, mask, + `a`, `switch`): union over non-comment children of
  `bbox(child, transform=false)` transformed by `child.transform`.
- image: `(x, y, width, height)` via ipx; `%` → fraction of viewBox (cache.py:520-541).
- use: `bbox(target, transform=false)` transformed by `translate(x,y) · target.transform`. Others → null.
- clip/mask clamp (:1525-1541): for each of clip-path, mask whose target is not the parent: `cbb =
  bbox(clip, transform=false, stroke=false)`; `result = intersection(result, cbb)`; null cbb → null result.
- if transform: by `composed_transform(el)`.
- `has_bbox(el)`: el and ancestors not in {namedview, defs, metadata, foreignObject, sodipodi:guide,
  clipPath, style, tspan, flowRegion, flowPara, mask, rdf:RDF, cc:Work, dc:format, dc:type} (:561-583).
  `is_drawn(el)`: has_bbox && not group-like && `specified(display) != none`. `BB2(els, rough, parsed)`: id →
  bbox for supported tags (text, flowRoot, image, use, svg, g + 7 shapes) with has_bbox and non-null.

**Stroke/fill** (`get_strokefill` :1171-1225; `composed_width` utils.py:45-87; `composed_list` :182-194):
```
StrokeFill { stroke: Option<Rgba>, fill: Option<Rgba>, stroke_is_url, fill_is_url,
             stroke_width: Option<f64> /*visual*/, dasharray: Option<Vec<f64>> /*visual*/,
             marker_start/mid/end: Option<String> /*raw specified*/ }
Rgba { r,g,b: u8, alpha: f64, efflightness: f64 }
```
`stroke := specified("stroke") or "none"`, `fill := specified("fill") or "black"`, `op := specified("opacity")
or 1`. Per paint: `none` → None; `url(#…)` → None + `*_is_url`; `currentColor` → resolve via
`specified("color")` (upstream: None); else svgtypes::Color; `alpha = (stroke|fill)-opacity × op`;
`L255 = floor((max+min)/2)` of rgb (integer HSL lightness); `efflightness = alpha·L255/255 + (1−alpha)`.
`composed_width(el, prop) -> (visual, sf, ut)`: `v = specified(prop)` or default (`stroke-width` "1",
`font-size` "medium"); `%`/`em` → walk to the ancestor owning that value, take its parent's composed_width ×
fraction (utils.py:67-78); else `ut = ipx(v)` or small/medium/large = 10/12/14 px; `visual = ut·sf`.
`composed_list(el, "stroke-dasharray")`: "none" → None; else split spaces/commas, ipx each, × sf. Finally: if
stroke_width None/0 or stroke None → stroke = None, stroke_width = None, dasharray = None (:1216-1219).

**`is_rectangle(el, including_transform)`** (utils.py:180-242): `rect` with including_transform=false →
candidate. Else path/rect/line/polyline: `<path>` needs 1..=6 command letters in raw `d` and ≥4 parsed
commands; end points (optionally × el.transform); `tol = 1e-3·max(xrange, yrange)`; `uniquetol(x)==2 &&
uniquetol(y)==2`. `use` → recurse into target. Then reject if el has a `mask`, a resolvable `filter`, or a
`clip-path` any of whose children is not a rectangle (:232-241).

**`fuse`** (`fuseTransform`, `applytransform_mod.py:95-234`): only rect, ellipse, circle, polygon, polyline,
line, path — **groups, text, use, image untouched, children not visited**.
1. `transform_clipmask` for clip then mask (:67-86): if attribute present and non-identity T: duplicate
   clipPath/mask into defs, re-point, `fix_css`, `k.transform := T · k.transform` per child.
2. `transf := extra · el.transform`; remove `el.transform`. For `<path>` only, drop every `sodipodi:`/
   `inkscape:` attribute except `inkscape-academic*`/`inkscape-scientific*` (:20-32).
3. If transf has shear/rotation (b≠0 or c≠0) and el is rect/ellipse/circle → convert to `<path>` via cpath.
4. If transf identity and no ranges → nothing. Else: polygon/polyline → transform points; ellipse/circle →
   center = midpoint of transformed (cx−rx,cy−ry),(cx+rx,cy+ry); `edgex/edgey` = lengths of transformed
   top/right edges; `|edgex−edgey| ≤ 1e-5` → `<circle r=edgex/2>` else `<ellipse rx ry>`; line → both points;
   rect → bbox of transformed corners, **scale rx,ry by |a|,|d|** (improvement); path → `apply_affine`, or
   with ranges: `T_i` applied to elements `cmd_start[i0]..cmd_start[i1]`.
5. If `apply_to_stroke`: multiply inline `stroke-width` and inline non-none `stroke-dasharray` by
   `sf(transf)`; for an *inherited* non-none stroke write explicit `stroke-width = specified·sf` (improvement).
6. Gradient fix-up (:224-231): `fill`/`stroke` = `url(#g)` with linear/radialGradient whose `gradientUnits` ≠
   `objectBoundingBox`: duplicate g, `gradientTransform := transf · old`, re-point.

**`global_transform(el, T, ranges, preserve_stroke)`** (:1065-1107): `P := composed_transform(parent)`;
`el.transform := P⁻¹ · T · P · el.transform`; ranges likewise; then `fuse(el, identity, ranges)` (no-op for
groups/text/use/image). Stroke preservation: upstream always writes `stroke-width = sw_before/sf_after`; we
write only when `|sf_after/sf_before − 1| > 1e-9`. Same for non-none dasharray.

**`combine_paths(els, merge_idx)`** (:1111-1147): `pnew` = concatenation of each element's cpath in **global**
coordinates; `si`: element without `inkscape-scientific-combined-by-color` → push `len(pnew)` before
appending; else push its ints `+ len(pnew)` for all but the last; finally push total. Target `mel`: if it
lacks `d` convert to `<path>`; `d := pnew · composed_transform(mel)⁻¹` (transform attr stays). Set attributes
`clip-path="none"`, `mask="none"`, `fix_css` both; set `inkscape-scientific-combined-by-color = si.join(" ")`;
delete others with `delete_up`. Indices count BezPath elements of the `d` we write.

**Housekeeping**: `delete_up(n)` (cache.py:685-717): delete n and now-childless parents up to (not including)
root `<svg>`; drop clip-path/mask attributes referencing deleted ids. `gc_created_clips(created)`
(flatten_plots.py:513-527): repeat until stable: delete every created clipPath/mask referenced by no
clip-path/mask attribute or style value. `strip_whitespace` (:529-548): remove `tail` unless tag ∈ {tspan,
textPath, flowPara, flowRegion, flowSpan}; remove `text` unless tag ∈ {style, text, tspan, textPath, flowRoot,
flowPara, flowRegion, flowSpan}. Document scale (cache.py:1182-1277): `px_per_uu = min(wpx/vbw, hpx/vbh)` for
`meet` (max for slice; `none`/non-uniform → geometric mean); `Other_tests_nonuniform.svg` relies on `meet`.

## B.3 Tools
CLI: `sciink --tool=<flatten|scale|homogenize|ghost|combine|markers> [--tab=..] [--param=v]... [--id=ID]...
input.svg`. `--id` order = Inkscape selection order, significant for the Scaler. Tools bail with upstream's
messages when selection is empty/invalid.

### Flattener (`flatten_plots.py:151-548`)
Params: `deepungroup, fixtext, removerectw, revertpaths, removeduppaths, splitdistant, mergenearby,
removemanualkerning, mergesubsuper, reversions, removetextclips, setreplacement, replacement="Arial",
justification (1 centre, 2 left, 3 right, 4 unchanged), markexc (1/2), tab`. Text sub-options ANDed with
`fixtext`.
1. Exclusions tab: set `inkscape-scientific-flattenexclude="True"` (markexc=1) or remove (2); stop. Otherwise
   drop selected elements with non-empty value and descendants carrying one. `sel` in document order.
2. `seld` = selection + descendants. With deepungroup: move every selected `<defs>` and every selected
   clipPath/mask outside defs to the root `<defs>`.
3. Nothing but groups/defs/metadata → error "No objects selected!".
4. Unlink every `<use>` in seld whose target is not a `<symbol>`; add results' descendants to seld.
5. Deep ungroup: groups sorted ascending by child count (static). A group with a comment child whose children
   are all comments/defs/`unlinked_clone="True"` = Matplotlib glyph group: set `mpl_comment` = comment texts
   joined by ";", remove comments, keep grouped; groups already having `mpl_comment` kept; else
   `ungroup(g, removetextclips)`.
6. Rectangle pass over non-group selected descendants with tag ∈ {path, rect, line}, parent not flow*,
   `is_rectangle(el, false)`, stroke none/absent, fill not none:
   - fill == (255,255,255) with alpha exactly 1 → *white rect*.
   - Minus-sign reversion (`reversions`): `d` whose first three parsed commands equal `M 106,355 H 732 V 272`
     exactly → `t0 = composed_transform`; if `det(t0)<0`: `t0 := t0 · translate(c) · scale(1,−1) ·
     translate(−c)`, c = centre of local no-stroke bbox; replace by `<text transform=P⁻¹·t0 x="19.3964"
     y="626.924" style="font-size:999.997;font-family:sans-serif;fill:#rrggbb[;fill-opacity:a]">−</text>`.
   - Thin rect → stroke (`revertpaths`): local bbox bb; `dark = bb && !fill_is_url && fill.efflightness <
     16/255`; `dark && bb.w < bb.h/2.49` → `<path d="m xc,y1 v h">`, `stroke = fill rgb`, `stroke-width =
     bb.w`; `dark && bb.h < bb.w/2.49` → `d="m x1,yc h w"`, `stroke-width = bb.h`; both: `fill:none`,
     `stroke-linecap:butt`, if alpha≠1: `stroke-opacity = alpha`, `opacity = 1`. Guard fill == None.
7. Font replacement: for text/tspan delete `-inkscape-font-specification`; empty family → repl; family list
   gets repl appended unless last entry equals it (case-insensitive). Then `deswitch` every `<switch>`, then
   `remove_kerning` (Appendix A), then `removetextclips` drops clip-path/mask from text/flowRoot.
8. Bbox stage when `removerectw || removeduppaths`: `ngs2` = drawn elements in document order; `bbs =
   BB2(rough=true, parsed=true)`.
   - Duplicate removal: candidates path/rect/line with bbox, excluding shapes referenced by any text's
     `shape-inside`. Boxes equal when neither empty and all four edges differ ≤ `1e-6·max(size_i,size_j)`,
     `size = max(w,h)`. Pairs (i<j) ordered by j desc, i asc; skip removed i. `top = j` must have stroke or
     fill; if stroke: not url, alpha == 1, i has identical rgba stroke; same for fill; `specified(i) ==
     specified(j)`; global absolute paths equal or equal to reverse (per-point tol `1e-6·size`). Delete **i**
     with `delete_up`. (8460²/2 box compares ≈ 50 ms in Rust; no sweep needed.)
   - White rects: each white rect whose rough bbox strictly intersects no element *earlier* in document
     order → `delete_up`.
9. `gc_created_clips`, `strip_whitespace`; remove `unlinked_clone` attributes we added.

### Scaler (`scale_plots.py`)
Modes: **Correction** (`tab=correction`, `figuremode` 1 = maintain plot area, 2 = maintain bbox,
`wholeplot3`), **Matching** (`tab=matching`, `hmatchopts`/`vmatchopts` 1 none/2 match/3 match+align,
`matchprop` 1 plot areas/2 bboxes, `deletematch`, `wholeplot2`), **Advanced** (`tab=options`: `tickcorrect`
default true, `tickthreshold` 10 → `thr=0.1`; `marksf` 1 scale_free, 2 aspect_locked, 3 normal, 4 plot_area,
5 clear → sets/removes `inkscape-scientific-scaletype` and exits). **Drop hidden Fixed mode** (accept
`hscale/vscale/wholeplot1`, ignore).

Setup (:230-295): sel minus tspan/namedview/defs/metadata/foreignObject; all images → IMAGE_ERR; `fbbs` =
exact visual bboxes (stroke, clip) of every descendant; `gbbs[el] = geometric_bbox(el, fbb)` = for
path/rect/line/polyline min/max of global end points clamped to visual box, else visual box (:52-65).
Correction: plots = all selected; Matching: `first = sel[0]`, plots = rest, `wholesel` disables tick
correction; every plot must be a `<g>`. After all plots, `deletematch` deletes first.

`find_plot_area(pels, gbbs)` (:69-123) over direct non-comment children with bboxes, reversed: `xrange <
0.001·h` → vertical line, `yrange < 0.001·w` → horizontal; 3..=5 end points with `uniquetol(x,
1e-3·max(xrange,yrange))==2` and same for y → rectangle. Rectangles/`<rect>`s: `hasfill = fill ≠ None && ≠
opaque white`, hasstroke likewise; filled and (no stroke or stroke==fill) → solid (unused); else stroked →
box. `scaletype="plot_area"` → plotarea. Largest vertical extent among (vlines by h, boxes by h, plotareas by
h) → `lvel`; largest horizontal → `lhel`. Either missing (or wholesel) → `noplotarea` + warning.

`scale_plot(i)` (:297-613): `bba` = union of all children (geometric/visual), `bbp` = union of lvel, lhel
(or everything if noplotarea).
- Matching pre-pass (:303-309): group transform `sx = sqrt(a²+b²)`, `sy = det/sx`; if `|sx−1|>1e-5 ||
  |sy−1|>1e-5` run correction on this plot first, then **recompute fbbs/gbbs** (upstream keeps stale boxes).
- Correction (:331-408): `scalex, scaley = sx, sy`; `ref` = bbp.g centre (plot-area mode) or bba.f top-left
  (figure mode); `iextr = T(ref)·S(1/sx,1/sy)·T(−ref)`; `global_transform(group, iextr)`. Transform boxes by
  iextr, rebuild bba/bbp. Figure mode: `scalex = (trW − (bbaW − bbpW))/bbpW` (trW old visual width), same y;
  `tlx = (tr.x1 − refx)/oscalex + refx`, `dxl = bbp.x1 − tlx`, `refx = (tr.x1 + dxl − bbp.x1·scalex)/
  (1−scalex)` (or `tr.x1 + dxl` if scalex==1), same y (:380-408).
- Matching (:413-440): `bbmatch` = first's geometric box (bbox mode) or its plot area (find_plot_area over
  first's children, or [first] if not a group; missing → warning, fall back to box). `scalex = bbmatch.w/bbp.w`
  (plot-area) or `(bbmatch.w + bbp.w − bba.w)/bbp.w` (bbox) when hmatch, else 1; same y.
- `ref` (matching): bbp.g centre (plot-area) or bba.g centre (bbox). `fin = ref`; matching: `finx =
  bbmatch.xc` if align-x, then bbox mode: `finx −= 0.5·((bba.x2−bbp.x2) − (bbp.x1−bba.x1))·(1−scalex)`, same y.
- `gtr = T(fin)·S(scalex,scaley)·T(−ref)`, `iscl = S(1/sx,1/sy)`, `liscl = S(√|sx·sy|)·iscl`, `trul/trbr =
  gtr(bbp.g corners)`.
- Per child (:492-613): `global_transform(el, gtr)`; `stype = scaletype attr` or default `scale_free` for
  text/flowRoot/g, `normal` otherwise. Ticks (tickcorrect and el is v/h line): vertical with `gbb.h <
  thr·bbp.h`: top tick if `gbb.y2 < bbp.y1 + thr·bbp.h`, bottom if `gbb.y1 > bbp.y2 − thr·bbp.h`; horizontal
  analog. Pivot on transformed box: top `(cx, cy > trul.y ? y1 : y2)`, bottom `(cx, cy < trbr.y ? y2 : y1)`,
  left `(cx > trul.x ? x1 : x2, cy)`, right `(cx < trbr.x ? x2 : x1, cy)`; `global_transform(el,
  T(p)·iscl·T(−p))`. Else scale_free/aspect_locked without combined attr: pivot c = centre of gtr(gbb); `tr1 =
  T(c)·(iscl|liscl)·T(−c)`; if `cx < trul.x`: `dx = (gbb.xc − bbp.x1) − (cx − trul.x)`; if `cx > trbr.x`: `dx
  = (gbb.xc − bbp.x2) − (cx − trbr.x)`; same y; `global_transform(el, T(dx,dy)·tr1)`. With combined-by-color
  attr: per range `[si[k], si[k+1])`: `gbb_tr = geometric_bbox(el, gtr(fbb), range)`, `gbb = gtr⁻¹(gbb_tr)`,
  same logic, collect `(range, tr2·tr1)`, one `global_transform(el, identity, ranges)`. `normal`: nothing more.

### Homogenizer (`homogenizer.py:98-375`)
Params: `setfontsize, fontsize=8, fontmodes (2 fixed pt, 3 scale %, 4 scale max to pt, 5 mean, 6 median,
7 min, 8 max), setfontfamily, fontfamily, fixtextdistortion, setstroke, setstrokew=1, strokemodes (2 fixed px,
3 scale %, 5..8 stats), fusetransforms, clearclipmasks, plotaware`. `sel0` = selection, `sel` = descendants;
images → error; plotaware requires every selected item to be a group; `sela` = sel minus namedview/defs/
metadata/foreignObject/font/font-face/missing-glyph.
1. If any text option: `bbs = BB2(text elements)` (plotaware: all descendants), exact current text extents.
2. Font size (Appendix A); `onept = 1.3333/px_per_uu`; modes 5-8 = stats of max char size (pt) per element;
   empty → 12.
3. Distortion fix (:210-225): per text/flowRoot: `C = composed_transform`, det ≠ 0; `s = sign(det)`, `q =
   √|det|`, `m = √(a²+b²)`; `Cnew = [a·q/m, b·q/m, −b·q·s/m, a·q·s/m, e, f]`; `global_transform(el, Cnew·C⁻¹)`.
4. Font family: `inkscape_spec_to_css` (invalid → "Font seems to be invalid—check its spelling."); if any of
   weight/style/stretch set, others reset to normal; delete `-inkscape-font-specification`; `character_fixer`.
5. Recentre (:248-263): `bbs2 = BB2(text, re-parse)`; per text in both: `global_transform(el,
   translate(centre(bb1) − centre(bb2)))`. Plot-aware (:265-319): per selected group `find_plot_area(direct
   children)`, bbp = union lvel/lhel (missing → warning, plain recentre); for text descendants: `dx = (bbp.x1 −
   bb2.x2) − (bbp.x1 − bb1.x2)·bb2.w/bb1.w` if `bb1.xc < bbp.x1`; `dx = (bb1.x1 − bbp.x2)·bb2.w/bb1.w − (bb2.x1
   − bbp.x2)` if `bb1.xc > bbp.x2`; else recentre; same y.
6. Stroke width (:321-361): per element `(sw, sf) = composed_width(stroke-width)`; restrict to elements whose
   specified stroke is not none (upstream writes on all). Modes: 2 → `w_px/px_per_uu`; 3 → `old·w/100`; 5-8
   stats over visual widths. Write `stroke-width = new/sf` + `px`; skip when sf == 0.
7. Fuse transforms: per shape `fuse(el, extra = composed_transform, apply_to_stroke=true)` then `el.transform
   := P⁻¹` (path data in global coordinates).
8. Clear clips/masks: remove both attributes; set inline `none` only when a stylesheet supplies one.

### Text Ghoster (`text_ghoster.py`)
`EXTENT 0.5`, `OPACITY 0.75`, `STDDEV 0.5`. Per selected element: `<g>` **appended at end of parent**, move
el in, `g.transform := el.transform`, remove el.transform. Singular composed transform → skip. `bb = bbox(el,
transform=false)`; `fs = max composed font-size (ut values) over el and descendants that specify font-size`,
fallback `ipx("8pt") = 10.6667`. `border = fs·0.5`. Insert `<filter><feGaussianBlur stdDeviation="border·0.5"/>
</filter>` at index 0 of root `<defs>`; insert `<rect x=bb.x1−b y=bb.y1−b width=bb.w+2b height=bb.h+2b rx=b
style="fill:#ffffff;stroke:none;filter:url(#f);opacity:0.75">` as first child of g. No bbox → wrapped, no rect.

### Combine by color (`combine_by_color.py:36-125`)
`th = lightnessth/100` (0.15). els = selection + descendants in document order, tag not in {namedview, defs,
metadata, foreignObject, g, missing-glyph}, having `d`, `points` or `x1`. For i from last to first: if
`(stroke None || stroke.efflightness ≥ th) && (fill None || fill.efflightness ≥ th)`: gather unmerged j < i
with: stroke widths both None or `|Δ| < 0.001`; strokes both None or same rgb and `|Δalpha| < 0.001`; fills
likewise; dasharrays equal (tol 1e-3); identical raw marker-start/mid/end. If >1: `combine_paths(group,
merge_into = topmost in document order)`.

### Favorite markers (`favorite_markers.py`)
Params: `tab` (markers | addremove), `template`, `smarker/mmarker/emarker`, `size` (%, 100), `addt`,
`template_name`, `remt`, `template_rem`. Template = `[start, mid, end]`, each None or `{marker_attrs (all but
id), paths: [path_attrs (all but id)]}` (`get_marker_props` :290-313: paths from marker's first child if `<g>`
else marker; parse `url(#id)` properly).
- Apply (:446-472): for every path/line/polyline/rect/circle/ellipse in selection+descendants, per type:
  unchecked → delete `marker-<type>` inline; checked → `set_marker_props` (:315-355): `name = "FM"+template+
  type` (no whitespace); `s = size/100`; reuse existing defs marker whose id contains name and whose first
  child is `<g transform="scale(…)">` with `|a−s|<0.01 && |d−s|<0.01`; else create `<marker {attrs}><g
  transform="scale(s)">{paths}</g></marker>` with id `new_id(name)`; set inline `marker-<type>: url(#id)`.
- Add/remove (:387-444): addt → store props of first path-like selected element under template_name; remt →
  delete by name. Message "Templates successfully updated!…".
- **Storage**: `$INKSCAPE_PROFILE_DIR/sciink/favorite_markers.json` (Inkscape exports INKSCAPE_PROFILE_DIR;
  `inkex/utils.py:57-64`), fallback `<binary dir>/favorite_markers.json`. Format `{"templates": {"Arrow":
  [start|null, mid|null, end|null], …}}` with `{"attrs": {...}, "paths": [{...}]}` (prefixed attr names).
  Three built-ins (`favorite_markers.py:24-214`) embedded in the binary. UX: `template` dropdown = `Arrow |
  Triangle | Distance | Custom (name below)` + `template_name` string; add/remove tab: `addt`, `remt` (by
  name), `list` checkbox prints stored names to stderr. No self-modifying .inx, no restart. Lost: custom names
  not in dropdown; no pickle migration.

## B.4 Compatibility attributes (exact)
- `inkscape-scientific-flattenexclude="True"`; any non-empty value = excluded; removed to un-mark.
- `inkscape-scientific-scaletype` ∈ `scale_free | aspect_locked | normal | plot_area`.
- `inkscape-scientific-combined-by-color="s0 s1 … sk"`: ranges `[s_i, s_{i+1})` over commands of `d`; last =
  total count.
- `mpl_comment="0;1"`, `unlinked_clone="True"` (internals), marker ids `FM<Template><start|mid|end><n>`,
  root `<style>` rules `#id{clip-path:url(#x)}`.

## B.5 Rust layout and LOC
```
src/geom/mod.rs      fmt_num, ipx, transform parse/fmt, inverse, sf, Option<Rect> algebra, uniquetol   ~220
src/geom/path.rs     ParsedPath/parse_d/fmt_d/shape_path/end_points/reverse/eq_tol                     ~250
src/ops/style.rs     compose_style, fix_css_clipmask, composed_width, composed_list, strokefill         ~260
src/ops/clip.rs      merge_clipmask, compose_all, ungroup, group, unlink, deswitch                      ~350
src/ops/bbox.rs      BboxOpts, bbox (memo), has_bbox, is_drawn, bb2, is_rectangle                      ~260
src/ops/xform.rs     fuse, global_transform, combine_paths                                             ~330
src/ops/cleanup.rs   delete_up, gc_created_clips, strip_whitespace, doc_scale                          ~120
src/tools/flatten.rs / scale.rs / homogenize.rs / ghost.rs / combine.rs / markers.rs   ~400/450/220/90/120/260
                                                                                              total ≈ 3.5k
```
```rust
pub struct Ctx<'a> { dom: &'a mut Dom, text: &'a mut TextEngine, created_clips: Vec<NodeId>,
                     bbox_memo: HashMap<(NodeId, BboxOpts), Option<Rect>> }
pub fn bbox(ctx, n, opts: BboxOpts) -> Option<Rect>;  pub struct BboxOpts { transform, stroke, rough, parsed, clip: bool }
pub fn strokefill(ctx, n) -> StrokeFill;  pub fn composed_width(ctx, n, prop) -> (f64, f64, f64);
pub fn is_rectangle(ctx, n, including_transform: bool) -> bool;
pub fn compose_all(ctx, el, clip: Option<NodeId>, mask: Option<NodeId>, t: Affine, style: Option<&StyleMap>, remove_text_clip: bool) -> bool;
pub fn ungroup(ctx, g, remove_text_clip: bool);  pub fn unlink(ctx, use_: NodeId) -> Option<NodeId>;
pub fn fuse(ctx, n, extra: Affine, ranges: Option<&[(Range<usize>, Affine)]>, apply_to_stroke: bool);
pub fn global_transform(ctx, n, t: Affine, ranges: Option<Vec<(Range<usize>, Affine)>>, preserve_stroke: bool);
pub fn combine_paths(ctx, els: &[NodeId], merge_into: usize);
pub fn run_<tool>(ctx, sel: &[NodeId], p: &Params) -> Result<(), String>;   // Err → stderr message
```
Bbox memo: clear after any geometric mutation (simplest: per tool phase).

## B.6 Risks & experiments
- **R1** clip-path precedence (attribute vs inline vs CSS class) in Inkscape 1.2–1.5: 3 rects, open in each.
- **R2** `<use>` semantics (x/y then clip then transform, symbol→group): unlink Acid_tests, render with
  `inkscape --export-type=png` (test-time), pixel-diff vs original.
- **R3** clip explosion: Acid_tests has 1113 `<g clip-path>`; measure size/time after ungroup; dedupe created
  clipPaths by (transform, d) hash if bad.
- **R4** style-cache churn: `compose_all` invalidates subtrees ~20k times; verify per-node memoization.
- **R5** selection order from Inkscape (Scaler's "first selected"): log argv from a stub binary.
- **R6** arc→cubic index mapping: combine two circles with Python, scale with Rust; rendered diff.
- **R7** white-rect step must be disabled if the text engine is unavailable.
- **R8** float-exact vs tolerant comparisons: compare Python/Rust dup-removal counts on Acid_tests.
- **R9** stale bboxes in matching pre-correction / stroke-width side effects: deliberate deviations; verify on
  `Other_tests.svg` `g5224` (correction) and `rect5248`+`g4982` (matching) within test tolerance (transforms
  3 decimals, others 0 decimals — `tester_mods.py:432-483`).

## B.7 Build order
geom → ops/style + ops/clip → ops/bbox + is_rectangle + strokefill → ops/cleanup → **Flattener non-text
ships** (text steps no-op behind the engine) → ops/xform (fuse, global_transform) → Text Ghoster, Combine by
color (+combine_paths) → Scaler → Homogenizer (after text engine) → Favorite Markers any time.
