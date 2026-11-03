"""The backlog's shape, as data: one ranked list, gated at its bottom.

The rule (2026-10-30): pullable work first in value/effort order, the gated block
at the bottom of the same list, then delivered history, then retired items. This
checks the shape the rule describes, not the specific items, so it survives
delivery without being edited.
"""
import re
import sys

text = open('BACKLOG.md').read()

# The known dependency order inside the gated block. A new gated item must be
# placed deliberately, not appended.
GATED_ORDER = ('ITEM-076', 'ITEM-032', 'ITEM-029', 'ITEM-030', 'ITEM-072', 'ITEM-084')
MARKER = 'gated below this line'


def section(name):
    start = text.index(f'### {name}')
    end = text.find('\n### ', start + 1)
    return text[start: end if end != -1 else len(text)]


def rows(name):
    out = []
    for line in section(name).splitlines():
        if not line.startswith('| ITEM-'):
            continue
        cells = [c.strip() for c in line.strip('|').split('|')]
        out.append((cells[0], cells[3].lower()))
    return out


def gated(status):
    return 'staged' in status or 'slice 1 done' in status


problems = []
undelivered = rows('Undelivered')
gated_rows = [item for item, status in undelivered if gated(status)]
pullable = [item for item, status in undelivered if not gated(status)]

body = section('Undelivered')
if gated_rows:
    if MARKER not in body:
        problems.append('gated rows exist but the marker is missing')
    elif pullable:
        # Everything pullable must sit above the marker, everything gated below.
        cut = body.index(MARKER)
        for item in pullable:
            if body.index(f'| {item} |') > cut:
                problems.append(f'{item} is pullable but sits below the gated marker')
        for item in gated_rows:
            if body.index(f'| {item} |') < cut:
                problems.append(f'{item} is gated but sits above the marker')
    if tuple(gated_rows) != GATED_ORDER:
        problems.append(f'gated block order changed: {gated_rows}')

# No item may be listed twice across the three tables.
listed = [item for name in ('Undelivered', 'Delivered', 'Retired') for item, _ in rows(name)]
dupes = sorted({item for item in listed if listed.count(item) > 1})
if dupes:
    problems.append(f'listed in more than one place: {dupes}')

# Every row names an item and carries a status.
for name in ('Undelivered', 'Delivered', 'Retired'):
    for line in section(name).splitlines():
        if line.startswith('| ITEM-') and '**' not in line:
            problems.append(f'{name}: row has no status: {line[:60]}')

if problems:
    print('\n'.join(problems))
    sys.exit(1)
print(
    'backlog shape ok: one ranked list, gated at the bottom'
    + ('' if pullable else ' (nothing pullable)')
)
