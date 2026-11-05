"""The guides' shape, as data: linkable, dated, and short-sentenced.

A guide we publish has to be usable without a support channel (ITEM-113), so the
rules are mechanical and checked, not remembered:

- every relative link resolves to a file in this repository;
- every published page carries a `Last verified` footer with a date and a commit;
- every troubleshooting page uses the same five sections, in the same order;
- the mean sentence stays under 25 words, and no sentence runs past 40;
- the two sales working documents stay internal: no published page links to them.

Run from the repository root: `python3 scripts/check-guides.py`.
"""
import re
import sys
from pathlib import Path

GUIDES = Path('guides')
# Sales and outreach material, not product documentation. Kept in the repository,
# kept out of the site, and never linked from a page a customer reads.
INTERNAL = {'inferon-cost-audit.md', 'inferon-step2-materials.md'}
TROUBLESHOOTING = GUIDES / 'troubleshooting'
SECTIONS = (
    'Symptom',
    'What it means',
    'Fix',
    'How to confirm it worked',
    'When to stop and ask',
)
FOOTER = re.compile(
    r'^_Last verified (\d{4}-\d{2}-\d{2}) (?:against|by) .+ \(master ([0-9a-f]{7,40})\)\._$'
)
LINK = re.compile(r'\[[^\]]*\]\(([^)\s]+)\)')
FENCE = re.compile(r'^```')
SENTENCE_SPLIT = re.compile(r'(?<=[.!?])\s+')
MAX_MEAN = 25
MAX_ONE = 40

problems = []


def pages():
    """Published candidates: Markdown files, never a dotfile.

    `._name.md` is a macOS resource fork, not a page — a tar from an SMB volume can
    create them, so they are skipped rather than parsed.
    """
    return sorted(p for p in GUIDES.rglob('*.md') if not p.name.startswith('.'))


def published(path):
    return path.name not in INTERNAL


def prose(text):
    """The lines a reader reads: no fenced code, no tables, no headings."""
    out, fenced = [], False
    for line in text.splitlines():
        if FENCE.match(line):
            fenced = not fenced
            continue
        if fenced or line.startswith('|') or line.startswith('#'):
            continue
        out.append(line)
    return '\n'.join(out)


def plain(line):
    """Drop link targets and inline code, keep the words a reader sees."""
    line = LINK.sub(lambda m: m.group(0).split('](')[0].lstrip('['), line)
    return re.sub(r'`[^`]*`', 'code', line)


def sentences(text):
    for line in prose(text).splitlines():
        for sentence in SENTENCE_SPLIT.split(plain(line).strip()):
            words = [w for w in sentence.split() if w.strip('.,;:!?—–-()')]
            if words:
                yield sentence.strip(), len(words)


def check_links(path, text):
    for target in LINK.findall(text):
        if target.startswith(('http://', 'https://', 'mailto:', '#')):
            continue
        clean = target.split('#')[0].split('?')[0]
        if not clean:
            continue
        resolved = (path.parent / clean).resolve()
        if not resolved.exists():
            problems.append(f'{path}: link target does not exist: {target}')
        elif GUIDES.resolve() not in resolved.parents and resolved.parent != GUIDES.resolve():
            problems.append(f'{path}: links outside guides/ — a reader cannot open it: {target}')
        elif not published(resolved):
            problems.append(f'{path}: links to an internal document: {target}')


def check_footer(path, text):
    last = [line for line in text.splitlines() if line.startswith('_Last verified')]
    if not last:
        problems.append(f'{path}: no `Last verified` footer')
    elif not FOOTER.match(last[-1].strip()):
        problems.append(
            f'{path}: footer is not `_Last verified <YYYY-MM-DD> against <where> (master <sha>)._`'
        )


def check_sections(path, text):
    if TROUBLESHOOTING not in path.parents:
        return
    headings = [m.group(1) for m in re.finditer(r'^## (.+)$', text, re.MULTILINE)]
    missing = [s for s in SECTIONS if s not in headings]
    if missing:
        problems.append(f'{path}: missing sections: {", ".join(missing)}')
        return
    order = [h for h in headings if h in SECTIONS]
    if order != list(SECTIONS):
        problems.append(f'{path}: sections are out of order: {" → ".join(order)}')


def check_reading(path, text):
    lengths = [n for _, n in sentences(text)]
    if not lengths:
        return
    mean = sum(lengths) / len(lengths)
    if mean > MAX_MEAN:
        problems.append(f'{path}: mean sentence is {mean:.1f} words (max {MAX_MEAN})')
    for sentence, n in sentences(text):
        if n > MAX_ONE:
            problems.append(f'{path}: {n}-word sentence (max {MAX_ONE}): {sentence[:70]}…')


for page in pages():
    body = page.read_text()
    if not published(page):
        # Internal documents may point at each other and need no footer.
        continue
    check_links(page, body)
    check_footer(page, body)
    check_sections(page, body)
    check_reading(page, body)

if problems:
    print('\n'.join(problems))
    sys.exit(1)

count = len([p for p in pages() if published(p)])
print(f'guides ok: {count} published pages, links resolve, footers dated, sections even')
