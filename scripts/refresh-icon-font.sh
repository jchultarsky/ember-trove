#!/usr/bin/env bash
# Regenerate the self-hosted, subsetted Material Symbols font.
#
# The icon font at ui/public/fonts/material-symbols-outlined.woff2 contains
# ONLY the ligature glyphs for icon names the UI actually uses (~91 KB vs
# ~4 MB for the full set). A newly added icon name will render as raw text
# ("wb_sunny") until it is added to the subset — run this script after
# introducing a new icon and commit the refreshed .woff2 (and the ?v= stamps
# it writes into ui/index.html and ui/input.css).
#
# Google's css2 API does the subsetting server-side via `icon_names`; we
# scan ui/src for icon-name literals (icon= props, material-symbols spans,
# and match arms in icon-rendering files), then download the subset.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

NAMES=$(python3 - <<'EOF'
import re, glob

names = set()
for f in glob.glob('ui/src/**/*.rs', recursive=True):
    t = open(f).read()
    # icon="name" / icon: "name" props and fields
    for m in re.finditer(r'icon\s*[:=]\s*"([a-z][a-z0-9_]+)"', t):
        names.add(m.group(1))
    # the glyph inside a material-symbols-outlined span: every literal
    # between the end of its opening tag and </span>, so `aria-hidden="true"`
    # or a style attribute is never mistaken for the name, and both arms of an
    # `if … { "a" } else { "b" }` count
    for m in re.finditer(r'material-symbols-outlined', t):
        gt = t.find('>', m.end())
        end = t.find('</span>', gt)
        if gt != -1 and end != -1 and end - gt < 400:
            for lit in re.findall(r'"([a-z][a-z0-9_]{2,40})"', t[gt:end]):
                names.add(lit)
    # match arms / ternaries returning icon literals in icon-rendering files
    if 'material-symbols' in t:
        for p in (r'=>\s*\(?[^)\n]*?"([a-z][a-z0-9_]{2,30})"',
                  r'else\s*\{\s*"([a-z][a-z0-9_]{2,30})"'):
            for m in re.finditer(p, t):
                names.add(m.group(1))

# The scan over-collects (any lowercase literal in an arm can slip in).
# Unknown names are rejected by the fonts API with HTTP 400, which fails
# this script loudly — prune the offender below if that happens.
print(','.join(sorted(names)))
EOF
)

echo "Requesting subset for $(echo "$NAMES" | tr ',' '\n' | wc -l | tr -d ' ') names…"
UA="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36"
CSS=$(curl -fsS -A "$UA" "https://fonts.googleapis.com/css2?family=Material+Symbols+Outlined:opsz,wght,FILL,GRAD@20..48,100..700,0..1,-50..200&icon_names=${NAMES}&display=block")

URL=$(echo "$CSS" | python3 -c "import re,sys; print(re.search(r'url\((\S+?)\) format', sys.stdin.read()).group(1))")
curl -fsS -o ui/public/fonts/material-symbols-outlined.woff2 "$URL"

python3 - <<'EOF'
import hashlib, re
d = open('ui/public/fonts/material-symbols-outlined.woff2','rb').read()
assert d[:4] == b'wOF2', 'not a woff2 file'
print(f'ui/public/fonts/material-symbols-outlined.woff2 refreshed ({len(d)//1024} KB)')
# Cache-bust: the filename isn't hashed and /fonts/ is cached for 7 days,
# so stamp a content hash into every reference. index.html is never cached
# and the compiled CSS is content-hashed, so clients fetch the new glyphs at
# once instead of showing raw names ("mail") for up to a week (#147).
v = hashlib.sha256(d).hexdigest()[:10]
for ref in ('ui/index.html', 'ui/input.css'):
    t = open(ref).read()
    t2 = re.sub(r'material-symbols-outlined\.woff2(\?v=[0-9a-f]+)?',
                f'material-symbols-outlined.woff2?v={v}', t)
    open(ref, 'w').write(t2)
print(f'font references stamped ?v={v}')
EOF
