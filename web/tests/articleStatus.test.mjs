import assert from 'node:assert/strict';
import test from 'node:test';
import { finalArticleLabel, runStatusLabel } from '../../work/verify/web-tests/articleStatus.js';

const paragraph = 'A saved official statement with a real mechanism and limitation.';
const quote = 'official statement';
const semantic = ['Q1', 'Q2', 'Q3', 'Q4', 'Q5', 'Q6'].map((id) => ({ id, verdict: 'pass' }));

test('a quote inside a different statement does not label the final article as 已达标', () => {
  assert.equal(paragraph.includes(quote), true);
  const label = finalArticleLabel({
    stage: 'revise',
    goalOutcome: 'achieved',
    results: semantic,
  });
  assert.equal(label.includes('最终文章 · 已达标'), false);
  assert.equal(runStatusLabel('completed', false), '研究已完成');
  assert.equal(runStatusLabel('completed', false).includes('最终文章 · 已达标'), false);
});

test('已达标 appears only when the claim-paragraph check passed', () => {
  const passed = finalArticleLabel({
    stage: 'revise',
    goalOutcome: 'achieved',
    results: [...semantic, { id: 'citation_integrity', verdict: 'pass' }],
  });
  assert.equal(passed, '最终文章 · 已达标');
  const withoutClaim = finalArticleLabel({
    stage: 'revise',
    goalOutcome: 'achieved',
    results: [...semantic, { id: 'citation_integrity', verdict: 'fail' }],
  });
  assert.equal(withoutClaim.includes('最终文章 · 已达标'), false);
});
