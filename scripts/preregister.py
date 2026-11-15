#!/usr/bin/env python3
"""Seal the fixed protocol before any actual report. Never overwrite a run."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import random
import secrets

ROOT = Path(__file__).resolve().parents[1]
protocol = ROOT / 'docs/design/2-research-web-mvp.md'
raw = protocol.read_text()
marker = '### 11.6 共同报告提示词（运行前仅填时间）'
prompt = raw.split(marker, 1)[1].split('```text\n', 1)[1].split('\n```', 1)[0]
assert prompt.count('CUTOFF_UTC') == 1
now = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
run = now.replace('-', '').replace(':', '')
public = ROOT / 'docs/experiments' / run
private = ROOT / 'work/experiments' / run
public.mkdir(parents=True, exist_ok=False)
private.mkdir(parents=True, mode=0o700, exist_ok=False)
prompt = prompt.replace('CUTOFF_UTC', now) + '\n'
seed = secrets.randbits(256)
identities = ['inquiry', 'baseline-1', 'baseline-2', 'baseline-3']
random.Random(seed).shuffle(identities)
mapping = {'seed': str(seed), 'labels': dict(zip(['R1', 'R2', 'R3', 'R4'], identities))}
encoded = json.dumps(mapping, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()
(private / 'mapping.json').write_bytes(encoded)
os.chmod(private / 'mapping.json', 0o600)
(public / 'common-prompt.txt').write_text(prompt)
(private / 'protocol.md').write_bytes(protocol.read_bytes())
manifest = {
    'version': 1, 'registered_at_utc': now, 'cutoff_utc': now,
    'protocol_version': 'v0.1.2',
    'protocol_sha256': hashlib.sha256(protocol.read_bytes()).hexdigest(),
    'common_prompt_sha256': hashlib.sha256(prompt.encode()).hexdigest(),
    'mapping_commitment_sha256': hashlib.sha256(encoded).hexdigest(),
    'research': {'model': 'gpt-6-astra', 'effort': 'high', 'method': '0.3.0', 'first_report_before_baselines': True},
    'baselines': {'model': 'gpt-6-astra', 'effort': 'max', 'count': 3, 'tools': False},
    'judges': [{'model': 'gpt-6-astra', 'effort': 'max', 'order': ['R1','R3','R2','R4']},
               {'model': 'gpt-6.1-sol', 'effort': 'high', 'order': ['R4','R2','R3','R1']}],
    'thresholds': {'inquiry_minimum': 85, 'gap_over_best_baseline_minimum': 15, 'effective_citations': 12, 'primary_documents': 8},
    'initial_state': 'No actual Inquiry, baseline, or judge report has been generated.'
}
(public / 'preregistration.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2)+'\n')
(private / 'registration-path.txt').write_text(str(public)+'\n')
print(json.dumps({'run': run, 'public': str(public), 'private': str(private), 'mapping_commitment': manifest['mapping_commitment_sha256']}, ensure_ascii=False))
