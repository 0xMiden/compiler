#!/usr/bin/env python3
"""Citation check for the fuzza fact files.

Every backticked `module::test` whose module is a differential test module
must exist as `fn <test>()`; every backticked source path must exist; every
`#[ignore]`d differential test must be named in CORPUS-MAP.md; em dashes are
flagged. Run from anywhere: `python3 tools/fuzza-agent/check_facts.py`.
Exit status 1 when any problem is found.
"""
import glob
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '..', '..'))
TESTS = os.path.join(ROOT, 'tests/integration/src/end_to_end/differential/tests/')
FILES = ['KNOWLEDGE.md', 'PIPELINE-FACTS.md', 'CORPUS-MAP.md']
MODULES = {os.path.basename(p)[:-3] for p in glob.glob(TESTS + '*.rs')} - {'mod'}

fns = {}
ignored = set()
for m in MODULES:
    src = open(TESTS + m + '.rs').read()
    fns[m] = set(re.findall(r'^fn (\w+)\(\)', src, re.M))
    lines = src.split('\n')
    for i, l in enumerate(lines):
        if l.strip().startswith('#[ignore'):
            j = i
            while not lines[j].startswith('fn '):
                j += 1
            ignored.add(m + '::' + re.match(r'fn (\w+)', lines[j]).group(1))

bad = 0
cited = set()
for f in FILES:
    text = open(os.path.join(HERE, f)).read()
    print(f'{f}: {text.count(chr(10))} lines, {len(text.split())} words')
    if '—' in text:
        print(f'  EM DASH present in {f}')
        bad += 1
    # Backticked spans (they may wrap across lines inside a bullet).
    for span in re.findall(r'`([^`]+)`', text, re.S):
        span = ' '.join(span.split())
        for m, t in re.findall(r'\b(\w+)::(\w+)\b', span):
            if m not in MODULES:
                continue
            cited.add(m + '::' + t)
            if t not in fns[m]:
                print(f'  MISSING TEST {m}::{t} in {f}')
                bad += 1
        # Source paths: tokens with a slash and a Rust or MASM extension.
        for p in re.findall(r'[\w./-]+/[\w.-]+\.(?:rs|masm)', span):
            p2 = 'tests/integration/src/end_to_end/differential/' + p if p.startswith('tests/') else p
            cands = [os.path.join(ROOT, p), os.path.join(ROOT, p2)]
            cands += glob.glob(os.path.join(ROOT, '**', p), recursive=True)[:1]
            if not any(os.path.exists(c) for c in cands):
                print(f'  MISSING PATH {p} in {f}')
                bad += 1

corpus = open(os.path.join(HERE, 'CORPUS-MAP.md')).read()
for t in sorted(ignored):
    m, n = t.split('::')
    # `_repro` / `_edges` twins may be cited as "(+ `_repro`)".
    base = re.sub(r'_(repro|edges)$', '', n)
    if f'{m}::{n}' not in corpus and f'{m}::{base}' not in corpus:
        print(f'  IGNORED TEST NOT IN CORPUS-MAP: {t}')
        bad += 1

print(f'cited tests: {len(cited)}, ignored tests: {len(ignored)}, problems: {bad}')
sys.exit(1 if bad else 0)
