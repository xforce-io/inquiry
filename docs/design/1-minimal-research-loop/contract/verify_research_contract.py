"""Inquiry 领域设计见证；不调度任务、不导入 TaskLoop、不启动真实工具。"""
import copy
import hashlib
import importlib.metadata
import json
from pathlib import Path
import sys
import unittest
from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent

class DomainError(Exception):
    pass

def need(condition, message):
    if not condition:
        raise DomainError(message)

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def validate(name, value):
    schema = json.loads((ROOT / (name + '.schema.json')).read_text())
    Draft202012Validator.check_schema(schema)
    need(not list(Draft202012Validator(schema).iter_errors(value)), name + '_invalid')

def capture(candidates, materials, trusted_facts, blobs):
    """trusted_facts 仅来自框架受信接口；这里不实现框架的身份/归属校验。"""
    validate('candidates', candidates); validate('materials', materials)
    expected = {i['source_id']: i for i in candidates['items']}
    need(len(expected) == len(candidates['items']), 'duplicate_source')
    actual = {i['source_id']: i for i in materials['items']}
    need(len(actual) == len(materials['items']) and set(expected) == set(actual), 'coverage')
    usable = 0
    for sid, item in actual.items():
        fact = trusted_facts.get(item['receipt_id'])
        need(fact is not None, 'trusted_fact_missing'); validate('acquisition-fact', fact)
        need(fact['source_id'] == sid and expected[sid]['url'] == fact['requested_url'], 'source_mismatch')
        for key in ['requested_url', 'final_url', 'http_status', 'fetch_status', 'reason']:
            need(item[key] == fact[key], 'fact_mismatch')
        if fact['fetch_status'] == 'captured':
            raw = blobs.get(item['raw_slot']); need(raw is not None and sha(raw) == fact['body_sha256'], 'raw_mismatch')
            if fact['usable']:
                text = blobs.get(item['snapshot_slot'])
                need(text is not None and sha(text) == fact['normalized_sha256'], 'normalized_mismatch')
                need(bool(fact['transform_version']) and bool(text.strip()), 'empty_body')
                usable += 1
            else:
                need(item['snapshot_slot'] is None, 'unusable_snapshot')
        else:
            need(not fact['usable'] and item['snapshot_slot'] is None and item['raw_slot'] is None, 'false_capture')
    return 'ready' if usable else 'need_input'

def citation_key(c):
    return (c['artifact_id'], c['sha256'], c['paragraph'], c['quote'])

def locate(citation, accepted_text):
    raw = accepted_text.get(citation['artifact_id'])
    need(raw is not None, 'unaccepted_text')
    need(sha(raw) == citation['sha256'], 'version_mismatch')
    paragraphs = raw.decode('utf-8').split('\n\n')
    index = citation['paragraph']
    need(index < len(paragraphs) and citation['quote'] in paragraphs[index], 'location_mismatch')

def analysis(value, accepted_text):
    validate('analysis', value)
    evidence = {e['id']: e for e in value['evidence']}
    need(len(evidence) == len(value['evidence']), 'duplicate_evidence')
    for e in evidence.values(): locate(e, accepted_text)
    for j in value['judgments']:
        need(set(j['support'] + j['counter']) <= set(evidence), 'dangling_evidence')
    if value['decision'] == 'ready':
        need(evidence and value['judgments'], 'ready_without_evidence')
        need(all(j['support'] or j['counter'] for j in value['judgments']), 'unsupported_judgment')
    return value['decision']

def report(value, accepted_analysis, accepted_text):
    validate('report', value); analysis(accepted_analysis, accepted_text)
    allowed = {citation_key(e) for e in accepted_analysis['evidence']}
    for c in value['citations']:
        need(citation_key(c) in allowed, 'not_in_accepted_analysis'); locate(c, accepted_text)
    if value['answer_status'] == 'answered':
        need(value['citations'], 'citation_missing')
        return 'ready'
    return 'need_input'

class ResearchTests(unittest.TestCase):
    def setUp(self):
        self.raw = b'First paragraph.\n\nEvidence supports the claim.'
        self.texts = {'saved-1': self.raw}
        self.ev = {'id': 'e1', 'artifact_id': 'saved-1', 'sha256': sha(self.raw), 'paragraph': 1, 'quote': 'Evidence supports'}
        self.a = {'evidence': [self.ev], 'judgments': [{'text': 'claim', 'support': ['e1'], 'counter': [], 'limitations': ['fixture only']}], 'gaps': [], 'decision': 'ready'}
        self.r = {'answer_status': 'answered', 'conclusion': 'claim', 'citations': [{k:v for k,v in self.ev.items() if k != 'id'}], 'limitations': ['fixture only']}
        self.cs = {'items': [{'source_id': 's1', 'url': 'https://example.org/a', 'discovery_receipt_id': 'search-1', 'subquestion': 'Q'}], 'stop_reason': 'budget'}
        self.fact = {'source_id': 's1', 'requested_url': 'https://example.org/a', 'final_url': 'https://example.org/a', 'captured_at': '2026-11-02T00:00:00Z', 'http_status': 200, 'fetch_status': 'captured', 'reason': 'ok', 'body_sha256': sha(self.raw), 'normalized_sha256': sha(self.raw), 'transform_version': 'text@1', 'usable': True}
        self.ms = {'items': [{'source_id': 's1', 'requested_url': self.fact['requested_url'], 'final_url': self.fact['final_url'], 'http_status': 200, 'fetch_status': 'captured', 'reason': 'ok', 'receipt_id': 'fact-1', 'snapshot_slot': 'text', 'raw_slot': 'raw'}], 'stop_reason': 'done'}
        self.blobs = {'raw': self.raw, 'text': self.raw}
    def test_all_business_schemas(self):
        for p in ROOT.glob('*.schema.json'): Draft202012Validator.check_schema(json.loads(p.read_text()))
    def test_initial_inputs_and_no_upload(self):
        value = {'question': 'Q', 'seed_urls': [], 'scope': 'public'}; validate('research-input', value)
        value['uploads'] = ['file']; self.assertRaises(DomainError, validate, 'research-input', value)
    def test_sources_signal_is_only_domain_payload(self):
        value = {'urls': ['https://example.org/more'], 'reason': 'more'}; validate('sources-added', value)
        value['target_state'] = 'done'; self.assertRaises(DomainError, validate, 'sources-added', value)
    def test_capture_verified_content(self):
        self.assertEqual(capture(self.cs, self.ms, {'fact-1': self.fact}, self.blobs), 'ready')
    def test_capture_requires_trusted_fact(self):
        self.assertRaises(DomainError, capture, self.cs, self.ms, {}, self.blobs)
    def test_capture_rejects_modified_bytes(self):
        self.blobs['text'] = b'changed'; self.assertRaises(DomainError, capture, self.cs, self.ms, {'fact-1': self.fact}, self.blobs)
    def test_all_sources_unavailable_needs_input(self):
        self.fact.update(fetch_status='unavailable', usable=False, http_status=404, reason='not_found', body_sha256=None, normalized_sha256=None, transform_version=None)
        self.ms['items'][0].update(fetch_status='unavailable', http_status=404, reason='not_found', snapshot_slot=None, raw_slot=None)
        self.assertEqual(capture(self.cs, self.ms, {'fact-1': self.fact}, {}), 'need_input')
    def test_http_success_without_usable_text_is_not_ready(self):
        self.fact.update(usable=False, normalized_sha256=None); self.ms['items'][0]['snapshot_slot'] = None
        self.assertEqual(capture(self.cs, self.ms, {'fact-1': self.fact}, self.blobs), 'need_input')
    def test_capture_rejects_missing_source_disposition(self):
        self.ms['items'] = []; self.assertRaises(DomainError, capture, self.cs, self.ms, {}, {})
    def test_analysis_evidence_and_judgment(self):
        self.assertEqual(analysis(self.a, self.texts), 'ready')
    def test_analysis_rejects_dangling_support(self):
        self.a['judgments'][0]['support'] = ['unknown']; self.assertRaises(DomainError, analysis, self.a, self.texts)
    def test_analysis_rejects_duplicate_evidence(self):
        self.a['evidence'].append(copy.deepcopy(self.ev)); self.assertRaises(DomainError, analysis, self.a, self.texts)
    def test_analysis_rejects_wrong_quote(self):
        self.ev['quote'] = 'not in source'; self.assertRaises(DomainError, analysis, self.a, self.texts)
    def test_analysis_rejects_wrong_version(self):
        self.ev['sha256'] = 'f' * 64; self.assertRaises(DomainError, analysis, self.a, self.texts)
    def test_ready_analysis_requires_evidence_and_judgment(self):
        self.a['judgments'] = []; self.assertRaises(DomainError, analysis, self.a, self.texts)
    def test_ready_analysis_rejects_unsupported_judgment(self):
        self.a['judgments'][0]['support'] = []; self.assertRaises(DomainError, analysis, self.a, self.texts)
    def test_report_uses_accepted_evidence(self):
        self.assertEqual(report(self.r, self.a, self.texts), 'ready')
    def test_report_rejects_unanalysed_citation(self):
        self.r['citations'][0]['quote'] = 'supports'; self.assertRaises(DomainError, report, self.r, self.a, self.texts)
    def test_answered_requires_citation(self):
        self.r['citations'] = []; self.assertRaises(DomainError, report, self.r, self.a, self.texts)
    def test_insufficient_evidence_is_not_answered(self):
        self.r.update(answer_status='insufficient_evidence', citations=[])
        self.assertEqual(report(self.r, self.a, self.texts), 'need_input')
    def test_acquisition_payload_does_not_own_framework_run(self):
        self.fact['run_id'] = 'internal'; self.assertRaises(DomainError, validate, 'acquisition-fact', self.fact)

if __name__ == '__main__':
    result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(ResearchTests))
    files = sorted(ROOT.glob('*.schema.json')) + [ROOT/'research.bundle.json', ROOT/'research-input.example.json', Path(__file__), ROOT/'requirements.txt', ROOT/'skills/inquiry-deep-research/SKILL.md']
    summary = {'scope': 'Inquiry 业务 Schema/获取事实/证据与引用关系的设计见证，不证明真实 Rust、TaskLoop 接入、网页解析或研究语义质量', 'tests': result.testsRun, 'failures': len(result.failures), 'errors': len(result.errors), 'passed': result.wasSuccessful(), 'python': sys.version.split()[0], 'jsonschema': importlib.metadata.version('jsonschema'), 'source_sha256': {p.relative_to(ROOT).as_posix(): sha(p.read_bytes()) for p in files}, 'cases': sorted(n for n in dir(ResearchTests) if n.startswith('test_'))}
    (ROOT/'research-verification.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n')
    sys.exit(0 if result.wasSuccessful() else 1)
