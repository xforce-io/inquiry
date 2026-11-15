import { projectRun, type RawEvent, type RunInput } from './run-view';

function assert(condition: unknown, message: string) {
  if (!condition) throw new Error(message);
}

function input(partial: Partial<RunInput> = {}): RunInput {
  return {
    title: '如何选择浏览器方案',
    status: 'running',
    stateId: 'discover',
    visit: 1,
    outcome: null,
    enabled: true,
    issues: [],
    fetchNote: '',
    dropped: 0,
    events: [],
    ...partial,
  };
}

function event(partial: Partial<RawEvent> & Pick<RawEvent, 'id' | 'kind'>): RawEvent {
  return {
    state_id: 'discover',
    visit: 1,
    timestamp_ms: 5_000,
    span_id: partial.id,
    data: {},
    ...partial,
  };
}

const started = projectRun(input());
assert(started.messages.length === 1 && started.messages[0].id === 'start', '任务一开始就有你交给研究的问题');
assert(started.messages[0].from === 'you' && started.messages[0].to === 'research', '开场是你到研究');
assert(started.tools.length === 0, '没有 Trace 时不发明工具');
assert(started.note.includes('研究已经开始'), '运行中且尚无动作时要说明会随后出现');
assert(started.lead.status === 'active' && started.lead.activity === '正在广泛检索', '编制上的研究显示当前阶段');
assert(started.working === 1 && started.elapsedLabel === '0:00' && started.title === '如何选择浏览器方案', '页头用问题、进行中人数和已进行时间');

const disabled = projectRun(input({ enabled: false, issues: ['disabled'], status: 'running', events: [] }));
assert(disabled.tools.length === 0 && disabled.note.includes('没有运行记录'), '未采集时只保留交给研究的问题');

const run = projectRun(
  input({
    events: [
      event({ id: 'p1', kind: 'phase.started', timestamp_ms: 1_000, state_id: 'discover' }),
      event({ id: 'r1', kind: 'tool.requested', timestamp_ms: 2_000, span_id: 's-req', data: { callId: 'c1', name: 'inquiry.search' } }),
      event({ id: 's1', kind: 'tool.started', timestamp_ms: 3_000, span_id: 's-tool', data: { call_id: 'c1', tool: 'inquiry.search' } }),
      event({ id: 'f1', kind: 'tool.finished', timestamp_ms: 4_000, span_id: 's-tool', data: { outcome: 'succeeded' } }),
      event({ id: 'e1', kind: 'executor.finished', timestamp_ms: 8_000, data: { outcome: 'completed' } }),
    ],
  }),
);
assert(run.messages.filter((message) => message.label === '检索公开网页').length === 1, '同一次检索的请求和开始只画一条');
assert(run.participants.map((item) => item.name).join(',') === '你,研究,检索', '参与者按出现顺序');
const reply = run.messages.find((message) => message.kind === 'reply' && message.from === 'tool:inquiry.search');
assert(reply?.label === '已返回' && reply.replyTo === 'call:s1', '完成按同一 span 回到研究');
assert(reply && run.messages.find((message) => message.id === 'call:s1')!.at === 2_000, '时间从第一条 Trace 起算');
assert(run.messages.some((message) => message.label === '广泛检索已提交' && message.to === 'you'), '阶段提交回到你');
assert(run.tools[0].status === 'done' && run.tools[0].last === '检索公开网页', '编制保留该工具的最近动作');
assert(run.elapsedLabel === '0:07' && run.working === 1 && run.lead.last === '广泛检索已提交', '已进行时间取最晚动作，交接留给研究');

const failed = projectRun(
  input({
    status: 'failed',
    events: [
      event({ id: 's2', kind: 'tool.started', span_id: 'span-2', data: { tool: 'inquiry.fetch' } }),
      event({ id: 'f2', kind: 'tool.finished', span_id: 'span-2', data: { outcome: 'failed' } }),
    ],
  }),
);
assert(failed.messages.some((message) => message.status === 'failed' && message.label === '未完成'), '失败的返回画成未完成');
assert(failed.tools[0].status === 'failed', '编制上该工具是未完成');
assert(failed.working === 0, '失败不算进行中');
assert(failed.messages.some((message) => message.id === 'end' && message.label === '研究未完成'), '工具失败不等于阶段回执，快照仍说明研究未完成');

const waiting = projectRun(input({ status: 'waiting', stateId: 'await_sources', enabled: true, events: [] }));
assert(waiting.messages.some((message) => message.id === 'wait' && message.status === 'waiting' && message.to === 'you'), '等待资料时明确向你要网址');

const many = projectRun(
  input({
    events: Array.from({ length: 60 }, (_, index) =>
      event({ id: `n${index}`, kind: 'phase.started', timestamp_ms: 1_000 + index, state_id: 'analyze', visit: 2 }),
    ),
  }),
);
assert(many.messages[0].id === 'start' && many.messages.length === 48 && many.hidden === 13, '图上保留开场和最近的动作');
assert(many.messages.some((message) => message.label === '第2轮进入分析论证'), '自述使用事件自己的阶段而不是当前阶段');

const quiet = projectRun(input({ status: 'completed', outcome: 'report_ready', enabled: true, events: [] }));
assert(quiet.lead.activity === '报告已形成' && quiet.messages.some((message) => message.label === '报告已形成'), '没有 Trace 的完成态用快照说明结果');

console.log('run-view ok');
