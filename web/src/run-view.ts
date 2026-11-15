// 运行时序 / 运行编制。只投影公开快照和 Trace，不发明未出现的工具。

export type RunMode = 'sequence' | 'crew';

export type RawEvent = {
  id: string;
  kind: string;
  state_id: string;
  visit: number;
  timestamp_ms: number;
  span_id: string;
  data: Record<string, unknown>;
};

export type RunInput = {
  title: string;
  status: string;
  stateId: string;
  visit: number;
  outcome: string | null;
  enabled: boolean | null;
  issues: string[];
  fetchNote: string;
  dropped: number;
  events: RawEvent[];
};

export type RunMessage = {
  id: string;
  from: string;
  to: string;
  fromName: string;
  toName: string;
  label: string;
  detail: string;
  kind: 'call' | 'reply';
  status?: 'failed' | 'waiting';
  replyTo?: string;
  at: number | null;
};

export type CrewCard = {
  id: string;
  name: string;
  kind: 'agent' | 'tool';
  status: 'active' | 'idle' | 'done' | 'failed' | 'waiting';
  activity: string;
  last?: string;
};

export type RunModel = {
  participants: { id: string; name: string; kind: 'person' | 'agent' | 'tool'; status: CrewCard['status'] }[];
  messages: RunMessage[];
  hidden: number;
  lead: CrewCard;
  tools: CrewCard[];
  note: string;
  title: string;
  elapsedLabel: string;
  working: number;
};

const STAGES: Record<string, string> = {
  scope: '研究计划',
  discover: '广泛检索',
  capture: '获取留存',
  catalogue: '整理资料',
  analyze: '分析论证',
  report: '形成报告',
  await_sources: '补充资料',
  await_search: '搜索重试',
};

const TOOLS: Record<string, { name: string; call: string }> = {
  'inquiry.search': { name: '检索', call: '检索公开网页' },
  'inquiry.fetch': { name: '获取', call: '获取网页' },
  'inquiry.read_source': { name: '阅读', call: '阅读已保存资料' },
  'artifact.read': { name: '读取', call: '读取已有成果' },
  'artifact.write': { name: '提交', call: '提交本阶段成果' },
  '<undeclared>': { name: '未声明工具', call: '调用未声明工具' },
};

const STATUS_WORD: Record<CrewCard['status'], string> = {
  active: '进行中',
  idle: '空闲',
  done: '已结束',
  failed: '未完成',
  waiting: '等待',
};

type OpenCall = { messageId: string; tool: string };

function stageName(id: string) {
  return STAGES[id] || '研究';
}

function clip(text: string, max: number) {
  const clean = text.replace(/\s+/g, ' ').trim();
  return clean.length > max ? clean.slice(0, max) + '…' : clean;
}

function field(data: Record<string, unknown>, ...keys: string[]) {
  for (const key of keys) {
    const value = data[key];
    if (typeof value === 'string' && value.trim()) return value.trim();
  }
  return '';
}

function toolOf(event: RawEvent) {
  return field(event.data, 'tool', 'name');
}

function callOf(event: RawEvent) {
  return field(event.data, 'call_id', 'callId');
}

function toolMeta(id: string) {
  return TOOLS[id] || { name: id || '工具', call: '调用工具' };
}

function leadStatus(status: string): CrewCard['status'] {
  if (status === 'running') return 'active';
  if (status === 'waiting' || status === 'cancelling' || status === 'created') return 'waiting';
  if (status === 'failed' || status === 'blocked') return 'failed';
  if (status === 'completed' || status === 'cancelled') return 'done';
  return 'idle';
}

function leadActivity(input: RunInput) {
  const stage = stageName(input.stateId);
  const round = input.visit > 1 ? ` · 第${input.visit}轮` : '';
  switch (input.status) {
    case 'running':
      return `正在${stage}${round}`;
    case 'created':
      return '等待开始';
    case 'waiting':
      return input.stateId === 'await_search' ? '搜索服务暂不可用，稍后重试' : '等待补充公开网址';
    case 'cancelling':
      return '正在确认停止';
    case 'cancelled':
      return '停止已确认';
    case 'failed':
    case 'blocked':
      return '研究未完成';
    case 'completed':
      if (input.outcome === 'report_ready') return '报告已形成';
      if (input.outcome === 'search_unavailable') return '搜索服务仍不可用，研究已结束';
      return '已保留阶段成果';
    default:
      return stage;
  }
}

function roundPrefix(name: string, visit: number) {
  return visit > 1 ? `第${visit}轮${name}` : name;
}

function names(id: string, participants: RunModel['participants']) {
  return participants.find((item) => item.id === id)?.name || id;
}

function formatClock(ms: number) {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  if (hours > 0) return `${hours}:${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
  return `${minutes}:${String(seconds).padStart(2, '0')}`;
}

function finishLabel(event: RawEvent) {
  const outcome = field(event.data, 'outcome');
  const code = field(event.data, 'code');
  if (event.kind === 'tool.rejected' || code === 'rejected') return { label: '未受理', failed: true };
  if (event.kind === 'tool.interrupted') return { label: '已中断', failed: true };
  if (outcome === 'failed' || code) return { label: '未完成', failed: true };
  if (event.kind === 'tool.delivery') return { label: '已送达', failed: false };
  return { label: '已返回', failed: false };
}

// tool.requested 与随后的 tool.started 是同一次调用的两笔记账，图上只留开始。
function droppedRequests(events: RawEvent[]) {
  const drop = new Set<number>();
  for (let index = 0; index < events.length; index++) {
    if (events[index].kind !== 'tool.requested') continue;
    const call = callOf(events[index]);
    const tool = toolOf(events[index]);
    for (let next = index + 1; next < events.length; next++) {
      const kind = events[next].kind;
      if (kind === 'tool.finished' || kind === 'tool.rejected' || kind === 'tool.interrupted') break;
      if (kind === 'tool.started' && tool && ((call && callOf(events[next]) === call) || toolOf(events[next]) === tool)) {
        drop.add(index);
        break;
      }
    }
  }
  return drop;
}

export function projectRun(input: RunInput): RunModel {
  const participants: RunModel['participants'] = [
    { id: 'you', name: '你', kind: 'person', status: input.status === 'waiting' ? 'waiting' : 'idle' },
    { id: 'research', name: '研究', kind: 'agent', status: leadStatus(input.status) },
  ];
  const toolIds: string[] = [];
  const ensureTool = (raw: string) => {
    const id = `tool:${raw}`;
    if (!participants.some((item) => item.id === id)) {
      participants.push({ id, name: toolMeta(raw).name, kind: 'tool', status: 'idle' });
      toolIds.push(id);
    }
    return id;
  };

  const messages: RunMessage[] = [];
  const push = (message: Omit<RunMessage, 'fromName' | 'toName'>) => {
    messages.push({
      ...message,
      fromName: names(message.from, participants),
      toName: names(message.to, participants),
    });
  };

  push({
    id: 'start',
    from: 'you',
    to: 'research',
    label: clip(input.title || '研究问题', 42),
    detail: clip(input.title || '研究问题', 180),
    kind: 'call',
    at: null,
  });

  const drop = droppedRequests(input.events);
  const started = new Set<string>();
  const opens: OpenCall[] = [];
  const keys = new Map<string, OpenCall>();
  const remember = (call: OpenCall, callId: string, spanId: string) => {
    opens.push(call);
    if (callId) keys.set(`call:${callId}`, call);
    if (spanId) keys.set(`span:${spanId}`, call);
  };
  const take = (callId: string, spanId: string, fallback: boolean) => {
    const found = keys.get(`call:${callId}`) || keys.get(`span:${spanId}`) || (fallback ? opens[opens.length - 1] : undefined);
    if (!found) return undefined;
    const index = opens.indexOf(found);
    if (index >= 0) opens.splice(index, 1);
    for (const [key, value] of keys) if (value === found) keys.delete(key);
    return found;
  };

  const timed = input.events.map((event) => event.timestamp_ms).filter((value) => Number.isFinite(value) && value > 0);
  const origin = timed.length ? Math.min(...timed) : 0;
  const atOf = (event: RawEvent) => (Number.isFinite(event.timestamp_ms) && event.timestamp_ms > 0 ? event.timestamp_ms - origin : null);

  input.events.forEach((event, index) => {
    if (drop.has(index)) return;
    const visit = Number.isFinite(event.visit) && event.visit > 0 ? event.visit : 1;
    if (event.kind === 'phase.started') {
      const name = stageName(event.state_id);
      push({
        id: `phase:${event.id}`,
        from: 'research',
        to: 'research',
        label: visit > 1 ? `第${visit}轮进入${name}` : `进入${name}`,
        detail: visit > 1 ? `第${visit}轮进入${name}` : `进入${name}`,
        kind: 'call',
        at: atOf(event),
      });
      return;
    }
    if (event.kind === 'tool.started' || event.kind === 'tool.requested') {
      const raw = toolOf(event);
      if (!raw) return;
      const callId = callOf(event);
      const key = callId ? `call:${callId}` : event.span_id ? `span:${event.span_id}` : `id:${event.id}`;
      if (started.has(key)) return;
      started.add(key);
      const tool = ensureTool(raw);
      const meta = toolMeta(raw);
      const messageId = `call:${event.id}`;
      remember({ messageId, tool }, callId, event.span_id);
      push({
        id: messageId,
        from: 'research',
        to: tool,
        label: meta.call,
        detail: meta.call,
        kind: 'call',
        at: atOf(event),
      });
      return;
    }
    if (['tool.finished', 'tool.rejected', 'tool.host.responded', 'tool.delivery', 'tool.interrupted'].includes(event.kind)) {
      const found = take(callOf(event), event.span_id, event.kind === 'tool.finished');
      if (!found) return;
      if (messages.some((message) => message.replyTo === found.messageId)) return;
      const result = finishLabel(event);
      push({
        id: `reply:${event.id}`,
        from: found.tool,
        to: 'research',
        label: result.label,
        detail: result.label,
        kind: 'reply',
        status: result.failed ? 'failed' : undefined,
        replyTo: found.messageId,
        at: atOf(event),
      });
      return;
    }
    if (event.kind === 'executor.finished') {
      const name = roundPrefix(stageName(event.state_id), visit);
      const outcome = field(event.data, 'outcome');
      const failed = outcome === 'failed';
      const label =
        outcome === 'completed' ? `${name}已提交` : outcome === 'cancelled' ? `${name}已停止` : outcome === 'stop_unknown' ? `${name}的停止尚未确认` : failed ? `${name}未完成` : `${name}已结束`;
      push({
        id: `exec:${event.id}`,
        from: 'research',
        to: 'you',
        label,
        detail: label,
        kind: 'reply',
        status: failed ? 'failed' : undefined,
        at: atOf(event),
      });
    }
  });

  if (input.status === 'waiting') {
    const waitingSearch = input.stateId === 'await_search';
    push({
      id: 'wait',
      from: 'research',
      to: 'you',
      label: waitingSearch ? '搜索服务暂不可用' : '请补充公开网址',
      detail: waitingSearch ? '搜索服务暂不可用，稍后重试' : '请补充公开网址后继续',
      kind: 'reply',
      status: 'waiting',
      at: null,
    });
  } else if (['failed', 'blocked', 'cancelled', 'completed'].includes(input.status) && !messages.some((message) => message.id.startsWith('exec:'))) {
    const label =
      input.status === 'cancelled'
        ? '停止已确认'
        : input.status === 'completed'
          ? leadActivity(input)
          : '研究未完成';
    push({
      id: 'end',
      from: 'research',
      to: 'you',
      label,
      detail: label,
      kind: 'reply',
      status: input.status === 'failed' || input.status === 'blocked' ? 'failed' : undefined,
      at: null,
    });
  }

  for (const message of messages) {
    message.fromName = names(message.from, participants);
    message.toName = names(message.to, participants);
  }

  const tools: CrewCard[] = toolIds.map((id) => {
    const related = messages.filter((message) => message.from === id || message.to === id);
    const open = related.some((message) => message.kind === 'call' && !messages.some((reply) => reply.replyTo === message.id));
    const last = related[related.length - 1];
    const status: CrewCard['status'] =
      open && (input.status === 'running' || input.status === 'cancelling') ? 'active' : last?.status === 'failed' ? 'failed' : last ? 'done' : 'idle';
    const participant = participants.find((item) => item.id === id);
    if (participant) participant.status = status;
    const call = [...related].reverse().find((message) => message.kind === 'call');
    return {
      id,
      name: participant?.name || id,
      kind: 'tool',
      status,
      activity: status === 'active' ? '正在调用' : status === 'failed' ? '上次未完成' : '上次已返回',
      last: call?.label,
    };
  });

  const handoff = [...messages].reverse().find((message) => message.from === 'research' && message.to !== 'research');
  let hidden = input.dropped;
  let visible = messages;
  if (messages.length > 48) {
    const start = messages[0]?.id === 'start' ? messages[0] : null;
    const tail = messages.slice(start ? -47 : -48);
    hidden += messages.length - tail.length - (start ? 1 : 0);
    visible = start ? [start, ...tail] : tail;
  }

  const notes: string[] = [];
  if (input.fetchNote) notes.push(input.fetchNote);
  else if (input.enabled === false) notes.push('这次研究没有运行记录。这里只保留你交给研究的问题。');
  else if (input.enabled === null) notes.push('正在读取运行记录。');
  else if (!input.events.length && (input.status === 'running' || input.status === 'created')) notes.push('研究已经开始。阶段和工具会在记下来之后出现在图上。');
  else if (input.issues.some((issue) => issue === 'not_recorded_or_expired' || issue.startsWith('retention_expired'))) notes.push('运行记录还没有，或已经过期。');
  else if (input.issues.some((issue) => issue !== 'disabled')) notes.push('运行记录不完整，这里只画出已经记到的动作。');
  if (hidden) notes.push(`较早的 ${hidden} 条动作没有画在这张图上。`);

  const elapsed = visible.reduce((max, message) => (message.at === null ? max : Math.max(max, message.at)), 0);
  const lead: CrewCard = {
    id: 'research',
    name: '研究',
    kind: 'agent',
    status: leadStatus(input.status),
    activity: leadActivity(input),
    last: handoff?.label,
  };
  return {
    participants,
    messages: visible,
    hidden,
    lead,
    tools,
    note: notes.join(' '),
    title: input.title,
    elapsedLabel: formatClock(elapsed),
    working: [lead, ...tools].filter((card) => card.status === 'active').length,
  };
}

function clock(message: RunMessage) {
  if (message.at === null) return message.id === 'start' ? '0:00' : message.id === 'wait' || message.id === 'end' ? '现在' : '·';
  return formatClock(message.at);
}

let selectedId = '';

export function resetRunView() {
  selectedId = '';
}

function node(tag: string, className = '', text?: string) {
  const element = document.createElement(tag);
  if (className) element.className = className;
  if (text !== undefined) element.textContent = text;
  return element;
}

function paintSelection(host: HTMLElement, model: RunModel) {
  for (const button of host.querySelectorAll<HTMLButtonElement>('.seq-hit, .crew-card')) button.setAttribute('aria-pressed', button.dataset.id === selectedId ? 'true' : 'false');
  const detail = host.querySelector<HTMLElement>('.run-detail');
  if (!detail) return;
  const message = model.messages.find((item) => item.id === selectedId);
  const card = model.lead.id === selectedId ? model.lead : model.tools.find((item) => item.id === selectedId);
  if (message) detail.textContent = `${message.fromName}到${message.toName}：${message.detail}`;
  else if (card) detail.textContent = `${card.name}，${STATUS_WORD[card.status]}。${card.activity}${card.last ? `。最近：${card.last}` : ''}`;
  else detail.textContent = '';
}

function select(host: HTMLElement, model: RunModel, id: string) {
  selectedId = selectedId === id ? '' : id;
  paintSelection(host, model);
}

function toneClass(participant: RunModel['participants'][number], participants: RunModel['participants']) {
  if (participant.kind === 'person') return 'tone-person';
  if (participant.kind === 'agent') return 'tone-lead';
  const index = participants.filter((item) => item.kind === 'tool').findIndex((item) => item.id === participant.id);
  return `tone-${Math.max(0, index) % 4}`;
}

function folderMark() {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 16 16');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('class', 'run-folder');
  const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  path.setAttribute('d', 'M1.8 4.2c0-.8.7-1.5 1.5-1.5H6l1.1 1.4h5.6c.8 0 1.5.7 1.5 1.5v6.2c0 .8-.7 1.5-1.5 1.5H3.3c-.8 0-1.5-.7-1.5-1.5V4.2z');
  path.setAttribute('fill', '#e4ebfb');
  path.setAttribute('stroke', '#7ea0e8');
  path.setAttribute('stroke-width', '1.2');
  path.setAttribute('stroke-linejoin', 'round');
  svg.append(path);
  return svg;
}

function renderHead(model: RunModel, mode: RunMode, onMode?: (mode: RunMode) => void) {
  const head = node('div', 'run-head');
  const title = node('h2', 'run-title', model.title || '研究');
  if (model.title) title.title = model.title;
  if (mode === 'crew') {
    const wrap = node('div', 'run-title-wrap');
    wrap.append(folderMark(), title);
    head.append(wrap);
  } else head.append(title);
  const meta = node('p', 'run-meta');
  if (model.working > 0) meta.append(node('span', 'run-working on', `${model.working} 进行中`));
  meta.append(node('span', '', `${model.messages.length} 条`), node('span', '', model.elapsedLabel));
  head.append(meta);
  const group = node('div', 'run-switch');
  group.setAttribute('role', 'radiogroup');
  group.setAttribute('aria-label', '运行视图');
  for (const [id, label] of [['sequence', '时序'], ['crew', '编制']] as const) {
    const choice = node('button', mode === id ? 'on' : '', label) as HTMLButtonElement;
    choice.type = 'button';
    choice.dataset.mode = id;
    choice.setAttribute('role', 'radio');
    choice.setAttribute('aria-checked', mode === id ? 'true' : 'false');
    choice.addEventListener('click', () => {
      if (id !== mode) onMode?.(id);
    });
    group.append(choice);
  }
  group.addEventListener('keydown', (event) => {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    event.preventDefault();
    event.stopPropagation();
    onMode?.(mode === 'sequence' ? 'crew' : 'sequence');
  });
  head.append(group);
  return head;
}

export function renderRun(host: HTMLElement, model: RunModel, mode: RunMode, animate: boolean, onMode?: (mode: RunMode) => void) {
  const previous = host.querySelector('.seq-scroll');
  const stick = !previous || previous.scrollHeight - previous.scrollTop - previous.clientHeight < 80;
  const left = previous?.scrollLeft ?? 0;
  const focusMode = (document.activeElement as HTMLElement | null)?.dataset?.mode;
  host.replaceChildren();
  const known = model.messages.some((message) => message.id === selectedId) || model.tools.some((tool) => tool.id === selectedId) || selectedId === model.lead.id;
  if (!known) selectedId = '';
  host.append(renderHead(model, mode, onMode));
  host.append(mode === 'crew' ? renderCrew(host, model) : renderSequence(host, model, animate));
  host.append(node('p', 'run-detail', ''));
  paintSelection(host, model);
  host.onkeydown = (event) => {
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
    const buttons = [...host.querySelectorAll<HTMLButtonElement>('.seq-hit, .crew-card')];
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (index < 0) return;
    event.preventDefault();
    const next = event.key === 'Home' ? buttons[0] : event.key === 'End' ? buttons[buttons.length - 1] : buttons[index + (event.key === 'ArrowDown' ? 1 : -1)];
    next?.focus();
  };
  if (focusMode === 'sequence' || focusMode === 'crew') host.querySelector<HTMLButtonElement>(`.run-switch button[data-mode="${mode}"]`)?.focus();
  const scroller = host.querySelector<HTMLElement>('.seq-scroll');
  if (scroller) {
    scroller.scrollLeft = left;
    if (stick) scroller.scrollTop = scroller.scrollHeight;
  }
}

function activationBars(model: RunModel) {
  const rows = model.messages;
  const count = rows.length;
  const bars: { index: number; start: number; end: number; open: boolean; tone: string }[] = [];
  const researchAt = model.participants.findIndex((item) => item.id === 'research');
  if (researchAt >= 0 && count) {
    bars.push({ index: researchAt, start: 0, end: count - 1, open: model.lead.status === 'active', tone: 'tone-lead' });
  }
  model.participants.forEach((participant, index) => {
    if (participant.kind !== 'tool') return;
    const spans: [number, number, boolean][] = [];
    rows.forEach((message, row) => {
      if (message.kind !== 'call' || message.to !== participant.id) return;
      const replyAt = rows.findIndex((item) => item.replyTo === message.id);
      const open = replyAt < 0 && participant.status === 'active';
      spans.push([row, Math.max(row, replyAt >= 0 ? replyAt : count - 1), open]);
    });
    spans.sort((a, b) => a[0] - b[0]);
    const merged: [number, number, boolean][] = [];
    for (const span of spans) {
      const previous = merged[merged.length - 1];
      if (previous && span[0] <= previous[1]) {
        previous[1] = Math.max(previous[1], span[1]);
        previous[2] = previous[2] || span[2];
      } else merged.push([span[0], span[1], span[2]]);
    }
    const tone = toneClass(participant, model.participants);
    for (const [start, end, open] of merged) bars.push({ index, start, end, open, tone });
  });
  return bars;
}

function renderSequence(shell: HTMLElement, model: RunModel, animate: boolean) {
  const sheet = node('div', 'seq-sheet');
  sheet.style.setProperty('--n', String(model.participants.length));
  const chips = node('div', 'seq-chips');
  chips.append(node('span', 'seq-gutter'));
  for (const participant of model.participants) {
    const chip = node('span', `chip ${toneClass(participant, model.participants)}`);
    chip.append(node('i', 'mark'), node('b', '', participant.name));
    chip.title = `${participant.name}，${STATUS_WORD[participant.status]}`;
    chips.append(chip);
  }
  const stage = node('div', 'seq-stage');
  stage.style.setProperty('--rows', String(model.messages.length));
  const rails = node('div', 'seq-rails');
  rails.setAttribute('aria-hidden', 'true');
  model.participants.forEach((_, index) => {
    const life = node('span', 'life');
    life.style.setProperty('--i', String(index));
    rails.append(life);
  });
  for (const bar of activationBars(model)) {
    const mark = node('span', `seq-bar ${bar.tone}${bar.open ? ' open' : ''}`);
    mark.style.setProperty('--i', String(bar.index));
    mark.style.setProperty('--a', String(bar.start));
    mark.style.setProperty('--b', String(bar.end));
    rails.append(mark);
  }
  const list = node('ol', 'seq-log');
  list.setAttribute('aria-label', '运行时序');
  const lastId = model.messages[model.messages.length - 1]?.id;
  for (const message of model.messages) {
    const from = model.participants.findIndex((item) => item.id === message.from);
    const to = model.participants.findIndex((item) => item.id === message.to);
    if (from < 0 || to < 0) continue;
    const item = node('li');
    const button = node('button', 'seq-hit') as HTMLButtonElement;
    button.type = 'button';
    button.dataset.id = message.id;
    button.setAttribute('aria-pressed', 'false');
    const time = clock(message);
    const mark = message.status === 'failed' ? '，未完成' : message.status === 'waiting' ? '，等待' : '';
    button.setAttribute('aria-label', `${message.fromName}到${message.toName}：${message.detail}，${time}${mark}`);
    if (animate && message.id === lastId) button.classList.add('latest');
    button.append(node('span', 'seq-time', time));
    const lo = Math.min(from, to);
    const hi = Math.max(from, to);
    const sender = model.participants[from];
    const track = node('span', `seq-track ${toneClass(sender, model.participants)}`);
    track.style.gridColumn = `${lo + 2} / ${hi + 3}`;
    track.style.setProperty('--span', String(hi - lo + 1));
    if (from === to) {
      track.classList.add('loop');
      if (from === model.participants.length - 1) track.classList.add('flip');
    } else if (message.kind === 'reply') track.classList.add('reply');
    if (message.status === 'failed') track.classList.add('failed');
    if (message.status === 'waiting') track.classList.add('waiting');
    const label = node('span', 'seq-label', message.label);
    label.title = message.label;
    track.append(label);
    button.append(track);
    button.addEventListener('click', () => select(shell, model, message.id));
    item.append(button);
    list.append(item);
  }
  stage.append(rails, list);
  sheet.append(chips, stage);
  const scroll = node('div', 'seq-scroll');
  scroll.append(sheet);
  return scroll;
}

function renderCrew(shell: HTMLElement, model: RunModel) {
  const sheet = node('div', 'crew-sheet');
  const tree = node('ul', 'crew');
  tree.setAttribute('aria-label', '运行编制');
  const leadItem = node('li', 'crew-lead-wrap');
  leadItem.append(crewButton(shell, model, model.lead));
  tree.append(leadItem);
  if (model.tools.length) {
    const children = node('ul', `crew-children${model.tools.length === 1 ? ' solo' : ''}`);
    children.style.setProperty('--kids', String(model.tools.length));
    for (const tool of model.tools) {
      const item = node('li');
      item.append(crewButton(shell, model, tool));
      children.append(item);
    }
    const branch = node('li', 'crew-branch');
    branch.append(children);
    tree.append(branch);
  }
  sheet.append(tree);
  return sheet;
}

function crewButton(shell: HTMLElement, model: RunModel, card: CrewCard) {
  const tone = card.kind === 'tool' ? toneClass({ id: card.id, name: card.name, kind: 'tool', status: card.status }, model.participants) : 'tone-lead';
  const button = node('button', `crew-card ${tone} ${card.status}`) as HTMLButtonElement;
  button.type = 'button';
  button.dataset.id = card.id;
  button.setAttribute('aria-pressed', 'false');
  const kind = card.kind === 'tool' ? '工具，' : '';
  button.setAttribute('aria-label', `${card.name}，${kind}${STATUS_WORD[card.status]}，${card.activity}${card.last ? `，最近${card.last}` : ''}`);
  const top = node('span', 'crew-top');
  const name = node('span', 'crew-id');
  name.append(node('strong', '', card.name));
  if (card.kind === 'tool') name.append(node('small', '', '工具'));
  const pill = node('span', `pill ${card.status}`);
  pill.append(node('i'), node('span', '', STATUS_WORD[card.status]));
  top.append(node('i', 'mark'), name, pill);
  button.append(top, node('span', 'crew-activity', card.activity));
  if (card.last && card.last !== card.activity) {
    const last = node('span', 'crew-last');
    last.append(node('span', 'crew-return', '↩'), node('span', '', card.last));
    button.append(last);
  }
  button.addEventListener('click', () => select(shell, model, card.id));
  return button;
}
