"""The backlog's shape, as data: one ranked list, gated at its bottom."""
import re, sys
text = open('BACKLOG.md').read()

def section(name):
    start = text.index(f'### {name}')
    nxt = text.find('\n### ', start + 1)
    return text[start: nxt if nxt != -1 else len(text)]

def rows(name):
    return [l.split('|')[1].strip() for l in section(name).splitlines() if l.startswith('| ITEM-')]

problems = []
undelivered = rows('Undelivered')
if undelivered[:1] != ['ITEM-102']:
    problems.append(f'the pullable item is not at the top: {undelivered[:1]}')
gated = ('ITEM-076', 'ITEM-032', 'ITEM-029', 'ITEM-030', 'ITEM-072', 'ITEM-084')
if tuple(undelivered[1:]) != gated:
    problems.append(f'gated block order changed: {undelivered[1:]}')

# The gated marker must sit between the pullable rows and the gated block.
body = section('Undelivered')
if body.index('gated below this line') > body.index('| ITEM-076'):
    problems.append('the gated marker is below the gated block')

# No item may be listed twice: the three sections must be disjoint.
listed = rows('Undelivered') + rows('Delivered') + rows('Retired')
dupes = sorted({i for i in listed if listed.count(i) > 1})
if dupes:
    problems.append(f'listed in more than one place: {dupes}')
if len(set(listed)) != len(listed):
    problems.append('duplicate ids in the tables')

# Every row names an item and carries a status.
for name in ('Undelivered', 'Delivered', 'Retired'):
    for row in section(name).splitlines():
        if row.startswith('| ITEM-') and '**' not in row:
            problems.append(f'{name}: row has no status: {row[:60]}')

print('\n'.join(problems) if problems else 'backlog shape ok: one ranked list, gated at the bottom')
sys.exit(1 if problems else 0)
