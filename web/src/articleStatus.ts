export type CheckResult = { id: string; verdict: string };

/** Run completion is a Goal status. It is not the final-article verdict. */
export function runStatusLabel(status: string, checkFailed: boolean): string {
  const labels: Record<string, string> = {
    running: '研究进行中',
    waiting: '等待澄清或补充',
    completed: '研究已完成',
    not_met: '文章未达标',
    failed: checkFailed ? '质量检查未完成' : '研究未完成',
    cancelling: '正在确认停止',
    cancelled: '已停止',
    stopped: '研究已暂停',
    blocked: '研究已阻塞',
  };
  return labels[status] || '状态待核对';
}

/**
 * 「最终文章 · 已达标」 requires the registered claim-paragraph check to pass.
 * A model semantic pass and a quote that merely occurs in the paragraph do not.
 */
export function finalArticleLabel(input: {
  stage: 'draft' | 'revise';
  goalOutcome: string | null;
  results: readonly CheckResult[];
}): string {
  if (input.stage === 'draft') return '初稿 · 不代表最终达标';
  if (input.goalOutcome === 'not_met') return '最终文章 · 未达标';
  const claimPassed = input.results.some(
    (result) => result.id === 'citation_integrity' && result.verdict === 'pass',
  );
  if (input.stage === 'revise' && input.goalOutcome === 'achieved' && claimPassed) {
    return '最终文章 · 已达标';
  }
  return '最终文章 · 评价待核对';
}
