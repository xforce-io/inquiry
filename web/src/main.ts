import './style.css';
import { buildCitationAliases, resolveCitationIndex, type Citation, type Evidence } from './citationRefs';

type Ref={id:string;sha256:string;slot:string;state_id:string;visit:number};
type Snapshot={execution_id:string;inputs:{question:string;scope:string;seed_urls:string[]};status:string;current_state:string;visit:number;revision:number;business_outcome:string|null;progress:{phase?:string};accepted_artifacts:Ref[];error:{code:string;message:string;path?:string}|null};
type Fact={source_id:string;requested_url:string;final_url:string|null;captured_at:string;fetch_status:string;http_status:number|null;reason:string;title:string;body_sha256:string|null;usable:boolean;snapshot_slot:string|null;truncated:boolean};
type Material={source_id:string;fact:Fact};
type Report={answer_status:string;conclusion:string;citations:Citation[];limitations:string[]};
type Output={artifact:Ref;value:unknown;stage?:string};
type Evaluation={phase:string;verdict:string;subject:{targets?:Record<string,{artifact_id:string;sha256:string}>};evaluation:{verdict:string;results:{id:string;verdict:string;message:string}[]}};
type Goal={status:string;outcome:string|null;spec:{parent_goal_id?:string};nodes:Record<string,{status:string}>;evaluations?:{check_id:string;status:string;error?:{message:string}|null}[];waiting:{wait_id:string;nonce:string;reason:string;consumed:boolean}[]};
type View={snapshot:Snapshot;outputs:Output[];engine?:string;read_only?:boolean;goal?:Goal;evaluations?:Evaluation[]};
type Saved={artifact:Ref;value:{fact:Fact;paragraphs:string[]}|null;text:string|null};
const app=document.querySelector<HTMLDivElement>('#app')!;
const drawer=document.querySelector<HTMLDivElement>('#drawer-root')!;
const stages=[['scope','研究计划'],['discover','广泛检索'],['capture','获取留存'],['catalogue','整理资料'],['analyze','分析论证'],['report','形成报告']];
const terminal=new Set(['completed','failed','cancelled','not_met','stopped']);
let generation=0;let timer:ReturnType<typeof setTimeout>|undefined;let current:View|null=null;let drawerVersion=0;let focusBeforeDrawer:HTMLElement|null=null;
let draft={question:'',urls:'',scope:'',key:crypto.randomUUID()};
const sourceDraft=new Map<string,{text:string;key:string}>();const cancelKeys=new Map<string,{key:string;revision:number}>();
const resumeKeys=new Map<string,{key:string;revision:number}>();
const selectedVersions=new Map<string,string>();
const clarificationDrafts=new Map<string,{text:string;urls:string;key:string}>();
let parentGoalId:string|undefined;
const element=<K extends keyof HTMLElementTagNameMap>(tag:K,cls='',text?:string)=>{const e=document.createElement(tag);e.className=cls;if(text!==undefined)e.textContent=text;return e;};
function button(text:string,cls:string,action:()=>void){const b=element('button',cls,text);b.type='button';b.addEventListener('click',action);return b;}
function link(text:string,href:string,cls=''){const a=element('a',cls,text);a.href=href;if(href.startsWith('/'))a.addEventListener('click',e=>{if(!e.ctrlKey&&!e.metaKey){e.preventDefault();navigate(href);}});else{a.target='_blank';a.rel='noopener noreferrer';}return a;}
function navigate(path:string){history.pushState({},'',path);route();window.scrollTo(0,0);}
function shell(){app.innerHTML='<header class="topbar"><a class="brand" href="/"><span class="brand-mark">i</span><span>Inquiry</span></a><span class="top-note">有依据地，研究一个问题。</span></header><main id="main"></main><footer>基于公开资料的研究 · 保留来源、分歧与未知</footer>';app.querySelector('a')!.addEventListener('click',e=>{e.preventDefault();navigate('/');});return app.querySelector<HTMLElement>('#main')!;}
function notice(container:HTMLElement,text:string,kind='error'){const p=element('div','notice '+kind,text);p.setAttribute('role',kind==='error'?'alert':'status');container.append(p);return p;}
async function api<T>(path:string,body?:unknown):Promise<T>{
 const options:RequestInit={credentials:'same-origin' as RequestCredentials,method:body===undefined?'GET':'POST',headers:body===undefined?{}:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)};
 let response=await fetch(path,{...options,signal:AbortSignal.timeout(15000)});
 if(response.status===401){await fetch('/',{credentials:'same-origin',cache:'no-store',signal:AbortSignal.timeout(10000)});response=await fetch(path,{...options,signal:AbortSignal.timeout(15000)});}
 let data:unknown;try{data=await response.json();}catch{throw new Error('服务暂时未返回可读取的结果，请重试。');}
 if(!response.ok){const err=data as {message?:string;code?:string};throw Object.assign(new Error(err.message||`请求未完成（${response.status}）`),{code:err.code,status:response.status});}return data as T;
}
function errorText(error:unknown){
 if(error instanceof DOMException&&['TimeoutError','AbortError'].includes(error.name))return '等待响应超时，请核对当前状态后重试。';
 if(error instanceof TypeError)return '暂时无法连接服务，请检查网络或服务后重试。';
 if(error instanceof Error){const code=(error as Error&{code?:string}).code;const messages:Record<string,string>={revision_conflict:'研究状态已更新，请刷新核对后再试。',request_conflict:'这次提交与先前请求不一致，请重新确认后再试。',not_found:'没有找到这项研究或资料，请核对地址。',capability_denied:'当前操作不被允许，请刷新核对状态。'};if(code&&messages[code])return messages[code];if(/[\u3400-\u9fff]/.test(error.message))return error.message;}
 return '请求未完成，请核对输入和当前状态后重试。';
}
function rootPage(){const main=shell();main.className='create-page';main.innerHTML='<section class="intro"><p class="eyebrow">从问题，到有依据的结论</p><h1>把问题<br><em>研究清楚。</em></h1><p class="intro-copy">先广泛收集，再核对、留存与分析。<br>每一份报告，都能回到它所依据的资料。</p></section><section class="compose-card"><form id="create-form"><label for="question">你想研究什么？</label><textarea id="question" rows="6" maxlength="12000" required placeholder="描述一个需要做决定、比较方案，或深入理解的问题。"></textarea><details id="options"><summary>补充网址与研究范围 <span>可选</span></summary><label for="seed-urls">已有线索 <small>每行一个公开网页，最多20个</small></label><textarea id="seed-urls" rows="3" placeholder="https://…"></textarea><label for="scope">背景、范围与完成标准</label><textarea id="scope" rows="3" maxlength="4000" placeholder="例如：团队背景、关注的时间范围、希望比较哪些选择……"></textarea></details><div class="resource-note"><span class="dot"></span>公开网页 · 两轮资料处理 · 一次修订 · 最长30分钟处理</div><div id="create-error"></div><div class="form-bottom"><p>需要澄清时可以补充，<br>未达标仍保留文章与缺口。</p><button class="primary" type="submit">开始研究 <span aria-hidden="true">↗</span></button></div></form></section><section class="method-preview"><span><b>01</b> 发现与留存</span><span><b>02</b> 整理与分析</span><span><b>03</b> 报告与溯源</span></section>';
 const question=main.querySelector<HTMLTextAreaElement>('#question')!,urls=main.querySelector<HTMLTextAreaElement>('#seed-urls')!,scope=main.querySelector<HTMLTextAreaElement>('#scope')!;question.value=draft.question;urls.value=draft.urls;scope.value=draft.scope;if(draft.urls||draft.scope)main.querySelector<HTMLDetailsElement>('#options')!.open=true;
 for(const input of [question,urls,scope])input.addEventListener('input',()=>{draft={question:question.value,urls:urls.value,scope:scope.value,key:crypto.randomUUID()};});
 if(parentGoalId){const context=notice(main.querySelector<HTMLElement>('.compose-card')!,'正在基于先前研究创建一项新研究。原研究与成果保持可读。','info');context.append(link('查看原研究','/research/'+encodeURIComponent(parentGoalId)));}
 const form=main.querySelector<HTMLFormElement>('#create-form')!;form.addEventListener('submit',async e=>{e.preventDefault();const gen=generation;const submit=form.querySelector<HTMLButtonElement>('button')!;const errors=main.querySelector<HTMLElement>('#create-error')!;errors.replaceChildren();submit.disabled=true;for(const input of [question,urls,scope])input.disabled=true;submit.textContent='正在创建…';try{const result=await api<{id:string}>('/api/research',{question:draft.question,scope:draft.scope,parent_goal_id:parentGoalId,seed_urls:draft.urls.split('\n').map(s=>s.trim()).filter(Boolean),request_key:draft.key});if(gen===generation){draft={question:'',urls:'',scope:'',key:crypto.randomUUID()};parentGoalId=undefined;navigate('/research/'+encodeURIComponent(result.id));}}catch(err){if(gen===generation)notice(errors,errorText(err)+' 内容已保留，可再次提交。');}finally{if(gen===generation){submit.disabled=false;for(const input of [question,urls,scope])input.disabled=false;submit.textContent='开始研究 ↗';}}});
}
function latest<T>(data:View,slot:string):T|null {const found=data.outputs.filter(o=>o.artifact.slot===slot).reduce<View['outputs'][number]|undefined>((latest,item)=>!latest||item.artifact.visit>latest.artifact.visit?item:latest,undefined);return found?found.value as T:null;}
function titleOf(question:string){const explicit=question.match(/(?:^|\n)问题[：:]\s*([^\n]+)/);const text=explicit?.[1]||question.split('\n').find(s=>s.trim())||'研究';return text.length>180?text.slice(0,180)+'…':text;}
function stateText(s:Snapshot){if(s.status==='waiting'&&s.current_state==='await_search')return '搜索服务暂不可用';if(s.status==='completed'&&s.business_outcome==='search_unavailable')return '搜索服务暂不可用 · 阶段成果';if(s.status==='completed')return s.business_outcome==='report_ready'?'报告就绪':'资料不足 · 阶段成果';return ({running:'研究进行中',created:'等待开始',waiting:'等待补充资料',blocked:'研究已阻塞',failed:'研究未完成',cancelling:'正在确认停止',cancelled:'已停止'} as Record<string,string>)[s.status]||'状态待核对';}
function researchError(error:NonNullable<Snapshot['error']>){
 const slot=error.path?.match(/^\/outputs\/([^/]+)/)?.[1];
 if(error.code==='artifact_invalid'&&slot){const names:Record<string,string>={plan:'研究计划',candidates:'候选来源',materials:'留存资料',catalogue:'资料整理',analysis:'分析证据',report:'报告'};return `本阶段未提交可接纳的${names[slot]||'成果'}，研究未完成。已有成果仍保留；可调整范围后新建研究。`;}
 if(/[\u3400-\u9fff]/.test(error.message))return error.message;
 return '本阶段执行或检查未能完成，已有成果仍保留。请核对原因后再决定是否新建研究。';
}
function inline(parent:HTMLElement,text:string,citations:Citation[],id:string,aliases?:ReadonlyMap<string,number>){
 const pattern=/(\*\*[^*]+\*\*|`[^`]+`|\[[A-Za-z0-9_-]+\])/g;
 let start=0;
 for(const match of text.matchAll(pattern)){
  parent.append(document.createTextNode(text.slice(start,match.index)));
  const value=match[0];
  if(value.startsWith('[')){
   const index=resolveCitationIndex(value.slice(1,-1),citations.length,aliases);
   if(index!==undefined){
    const citation=citations[index];
    const b=button(value.slice(1,-1),'cite',()=>void showSource(id,citation.artifact_id,citation));
    b.setAttribute('aria-label',`查看来源${index+1}`);
    parent.append(b);
   }else{parent.append(document.createTextNode(value));}
  }else{parent.append(element(value.startsWith('**')?'strong':'code','',value.slice(value.startsWith('**')?2:1,value.startsWith('**')?-2:-1)));}
  start=(match.index||0)+value.length;
 }
 parent.append(document.createTextNode(text.slice(start)));
}
function markdown(parent:HTMLElement,text:string,citations:Citation[],id:string,aliases?:ReadonlyMap<string,number>){const lines=text.split('\n');let i=0;while(i<lines.length){const line=lines[i].trim();if(!line){i++;continue;}if(line.includes('|')&&i+1<lines.length&&/^\s*\|?\s*:?-{3}/.test(lines[i+1])){const table=element('table');const wrap=element('div','table-wrap');const cells=(s:string)=>s.trim().replace(/^\||\|$/g,'').split('|').map(s=>s.trim());const header=element('tr');for(const cell of cells(line)){const th=element('th');inline(th,cell,citations,id,aliases);header.append(th);}table.append(header);i+=2;while(i<lines.length&&lines[i].includes('|')&&lines[i].trim()){const row=element('tr');for(const cell of cells(lines[i])){const td=element('td');inline(td,cell,citations,id,aliases);row.append(td);}table.append(row);i++;}wrap.append(table);parent.append(wrap);continue;}
 const heading=line.match(/^(#{1,4})\s+(.+)/);if(heading){const h=element(heading[1].length<=2?'h2':'h3');inline(h,heading[2],citations,id,aliases);parent.append(h);i++;continue;}
 if(/^[-*]\s+/.test(line)){const ul=element('ul');while(i<lines.length&&/^[-*]\s+/.test(lines[i].trim())){const li=element('li');inline(li,lines[i].trim().replace(/^[-*]\s+/,''),citations,id,aliases);ul.append(li);i++;}parent.append(ul);continue;}
 const paragraph=element('p');const chunk=[line];i++;while(i<lines.length&&lines[i].trim()&&!/^(#{1,4}\s|[-*]\s)/.test(lines[i].trim())&&!lines[i].includes('|')){chunk.push(lines[i].trim());i++;}inline(paragraph,chunk.join(' '),citations,id,aliases);parent.append(paragraph);
 }}
function detail(data:View,id:string){if(data.engine==='goal'&&data.goal){goalDetail(data,id);return;}const main=shell();main.className='detail-page';const s=data.snapshot;main.append(link('← 新研究','/','back-link'));const heading=element('section','research-heading');heading.append(element('p','eyebrow','研究问题'),element('h1','',titleOf(s.inputs.question)));const full=element('details','question-full');full.append(element('summary','','查看完整问题与范围'),element('pre','',s.inputs.question+'\n\n研究范围：'+s.inputs.scope));heading.append(full);main.append(heading);
 const status=element('section','status-card '+s.status);const row=element('div','status-row');const label=element('div','status-label');if(!terminal.has(s.status)&&s.status!=='blocked')label.append(element('span','pulse'));label.append(element('strong','',stateText(s)));row.append(label);
 if(!data.read_only&&!terminal.has(s.status)&&s.status!=='cancelling')row.append(button('停止研究','quiet danger',()=>void requestCancel(id)));else row.append(button('刷新','quiet',()=>void load(id,true)));status.append(row);
 const active=stages.find(([key])=>key===s.current_state);let description=s.status==='running'?(s.progress.phase==='checking'?'正在核对':'正在进行')+'：'+(active?.[1]||'研究')+(s.visit>1?` · 第${s.visit}轮`:''):s.status==='waiting'?(s.current_state==='await_search'?'搜索服务暂不可用，稍后自动重试。已有资料已保存，也可以停止研究。':'已有内容已保存。可补充公开网址继续；等待超过边界将保留为阶段成果。'):s.status==='cancelled'?'停止已确认。已有资料和阶段成果仍可查看。':s.status==='cancelling'?'停止请求已受理，正在等待执行停止确认。':s.status==='completed'?(s.business_outcome==='search_unavailable'?'搜索服务仍不可用，本次研究已结束。已有资料与阶段成果已保留，可稍后新建研究。':s.business_outcome==='report_ready'?'报告与保存来源已连通。请结合适用条件、限制与未知使用结论。':'现有资料尚不足以形成完整报告，已保存阶段成果与限制。'):'';if(description)status.append(element('p','muted',description));
 const timeline=element('ol','stage-list');for(const [key,name]of stages){const done=data.outputs.some(o=>o.artifact.state_id===key);const item=element('li',(s.current_state===key&&s.status==='running'?'active ':done?'done ':'')+key);item.append(element('span','stage-point',done?'✓':String(stages.findIndex(v=>v[0]===key)+1)),element('span','',name));timeline.append(item);}status.append(timeline);if(s.error)notice(status,researchError(s.error));main.append(status);
 const errors=element('div','read-error');errors.id='read-error';main.append(errors);
 if(data.read_only)notice(main,'这是旧方法的研究记录，保持只读。可查看原有报告、来源与过程。','info');
 if(!data.read_only&&s.status==='waiting'&&s.current_state==='await_sources')waiting(main,id,s.revision);
 const report=latest<Report>(data,'report');if(report){const card=element('section','report-card');const meta=element('div','section-top');meta.append(element('p','eyebrow',report.answer_status==='answered'?'研究报告':'阶段成果'),element('span','muted',`${report.citations.length}条可回溯引用`));card.append(meta);const article=element('article','report-body');markdown(article,report.conclusion,report.citations,id);card.append(article);if(report.limitations.length){const limits=element('section','limitations');limits.append(element('h3','','限制与未决事项'));const ul=element('ul');for(const l of report.limitations)ul.append(element('li','',l));limits.append(ul);card.append(limits);}main.append(card);}
 else{const empty=element('section','report-empty');empty.append(element('span','paper-icon','▤'),element('h2','',s.status==='cancelled'?'尚未生成报告':s.status==='completed'?'本次研究未生成报告':s.status==='failed'||s.status==='blocked'?'报告尚未完成':'报告将在研究完成后出现在这里'),element('p','muted','下方可查看已经保存的计划、资料和阶段成果。'));main.append(empty);}
 const allMaterials=data.outputs.filter(o=>o.artifact.slot==='materials').flatMap(o=>(o.value as {items:Material[]}).items.map(item=>({item,visit:o.artifact.visit})));
 const sources=element('section','sources-card');const top=element('div','section-top');top.append(element('h2','','研究资料'),element('span','muted',`${allMaterials.filter(m=>m.item.fact.usable).length}份已保存 · ${allMaterials.length}条处置记录`));sources.append(top);
 if(!allMaterials.length){const candidates=latest<{items:{url:string}[]}>(data,'candidates');sources.append(element('p','muted',candidates?`已发现${candidates.items.length}个候选网址，正在核对与留存。`:'资料将在实际获取并保存后显示。'));}
 for(const {item,visit}of allMaterials){const f=item.fact;const ref=s.accepted_artifacts.find(a=>a.state_id==='capture'&&a.visit===visit&&a.slot===f.snapshot_slot);const row=element('div','source-row');const content=element('div');content.append(element('strong','',f.title||f.final_url||f.requested_url),element('p','source-url',f.final_url||f.requested_url),element('p','source-note',`${f.reason}${f.http_status?' · HTTP '+f.http_status:''} · ${new Date(f.captured_at).toLocaleString('zh-CN')}`));row.append(content);if(ref)row.append(button('查看保存版本','quiet',()=>void showSource(id,ref.id)));else row.append(element('span','badge muted',f.fetch_status==='denied'?'未获取':'不可用'));sources.append(row);}main.append(sources);
 const process=element('details','process-card');process.append(element('summary','','查看研究过程与已保存成果'));const plan=latest<{subquestions:string[];stop_reason:string}>(data,'plan');if(plan){process.append(element('h3','','研究计划'));const ul=element('ul');for(const q of plan.subquestions)ul.append(element('li','',q));process.append(ul);}
 const catalogue=latest<{groups:{topic:string;source_ids:string[]}[];gaps:string[];limitations?:string[]}>(data,'catalogue');if(catalogue){process.append(element('h3','','资料整理'));for(const group of catalogue.groups)process.append(element('p','',`${group.topic} · ${group.source_ids.length}条来源`));for(const gap of catalogue.gaps)notice(process,gap,'info');for(const limitation of catalogue.limitations||[])notice(process,'研究限制：'+limitation,'info');}
 const analysis=latest<{judgments:{text:string;limitations:string[]}[];gaps:string[]}>(data,'analysis');if(analysis){process.append(element('h3','','分析判断'));for(const j of analysis.judgments)process.append(element('p','',j.text));for(const gap of analysis.gaps)notice(process,gap,'info');}main.append(process);
}
function goalDetail(data:View,id:string){
 const goal=data.goal!,snapshot=data.snapshot,main=shell();main.className='detail-page';
 main.append(link('← 新研究','/','back-link'));
 const heading=element('section','research-heading');heading.append(element('p','eyebrow','研究问题'),element('h1','',titleOf(snapshot.inputs.question)));
 const original=element('details','question-full');original.append(element('summary','','完整问题与研究范围'),element('pre','',snapshot.inputs.question+'\n\n研究范围：'+snapshot.inputs.scope));heading.append(original);if(goal.spec.parent_goal_id)heading.append(link('查看前次研究','/research/'+encodeURIComponent(goal.spec.parent_goal_id),'back-link'));main.append(heading);
 const status=element('section','status-card '+snapshot.status),row=element('div','status-row');
 const checkFailed=snapshot.status==='failed'&&(snapshot.progress.phase==='goal_checking'||goal.nodes.review?.status==='failed');
 const labels:Record<string,string>={running:'研究进行中',waiting:'等待澄清或补充',completed:'研究已完成',not_met:'文章未达标',failed:checkFailed?'质量检查未完成':'研究未完成',cancelling:'正在确认停止',cancelled:'已停止',stopped:'研究已暂停',blocked:'研究已阻塞'};
 row.append(element('strong','',labels[snapshot.status]||'状态待核对'));
 if(!terminal.has(snapshot.status)&&snapshot.status!=='cancelling')row.append(button('停止研究','quiet danger',()=>void requestCancel(id)));
 row.append(button('刷新','quiet',()=>void load(id,true)));status.append(row);
 if(snapshot.status==='cancelling')status.append(element('p','muted','停止请求已受理，尚在等待实际停止确认。'));
 if(snapshot.status==='not_met')status.append(element('p','muted','执行已结束，文章仍未满足全部必需要求。已有文章、资料和缺口均保留。'));
 if(snapshot.status==='stopped')status.append(button('继续这项研究','primary',()=>void requestResume(id)));
 const timeline=element('ol','stage-list');const steps=[['scope','研究范围'],['collect','发现与保存'],['analyze','分析缺口'],['supplement','有界补查'],['synthesize','综合论证'],['draft','初稿'],['review','独立评价'],['revise','修订'],['final_evaluation','最终评价']];
 for(const [index,[key,name]]of steps.entries()){const done=key==='final_evaluation'?(data.evaluations||[]).filter(e=>e.phase==='goal_final').length===2:goal.nodes[key]?.status==='succeeded';const item=element('li',done?'done':snapshot.current_state===key?'active':'');item.append(element('span','stage-point',done?'✓':String(index+1)),element('span','',name));timeline.append(item);}status.append(timeline);
 if(snapshot.current_state==='planning'&&snapshot.status==='running')status.append(element('p','muted','正在核对研究对象与既定研究路径。'));
 if(snapshot.error)notice(status,researchError(snapshot.error));main.append(status);
 const errors=element('div','read-error');errors.id='read-error';main.append(errors);
 if(snapshot.status==='waiting')goalWaiting(main,id,snapshot.revision,goal);
 const articles=data.outputs.filter(o=>o.artifact.slot==='report');
 const selected=selectedVersions.get(id)||(articles.some(o=>o.stage==='revise')?'revise':'draft');
 const output=articles.find(o=>o.stage===selected);
 if(output){
   const report=output.value as Report,card=element('section','report-card');const tabs=element('div','version-tabs');
   for(const o of articles){const b=button(o.stage==='revise'?'最终稿':'初稿',o.stage===selected?'primary':'quiet',()=>{selectedVersions.set(id,o.stage!);detail(data,id);});b.setAttribute('aria-pressed',String(o.stage===selected));tabs.append(b);}card.append(tabs);
   const evaluations=(data.evaluations||[]).filter(e=>Object.values(e.subject.targets||{}).some(t=>t.artifact_id===output.artifact.id&&t.sha256===output.artifact.sha256));
   const finals=evaluations.filter(e=>e.phase==='goal_final');const achieved=selected==='revise'&&goal.outcome==='achieved'&&finals.length===2&&finals.every(e=>e.verdict==='pass');
   card.append(element('p','eyebrow',selected==='draft'?'初稿 · 不代表最终达标':achieved?'最终文章 · 已达标':goal.outcome==='not_met'?'最终文章 · 未达标':'最终文章 · 评价待核对'));
   const analysis=data.outputs.find(o=>o.stage==='synthesize'&&o.artifact.slot==='analysis')?.value as {evidence:Evidence[]}|undefined;
   const aliases=buildCitationAliases(report.citations,analysis?.evidence||[]);
   const body=element('article','report-body');markdown(body,report.conclusion,report.citations,id,aliases);card.append(body);
   if(report.limitations.length){const limits=element('section','limitations');limits.append(element('h3','','限制与未决事项'));for(const l of report.limitations)limits.append(element('p','',l));card.append(limits);}
   const quality=element('section','quality-results');quality.append(element('h2','','该版本的质量评价'));
   if(checkFailed&&selected==='revise')notice(quality,'最终质量检查未完成，当前文章保留可读，不能判为已达标。');
   for(const check of goal.evaluations||[])if(check.error)notice(quality,'检查错误：'+check.check_id+'。已有评价与文章均保留。');
   if(!evaluations.length)quality.append(element('p','muted','尚无针对该确切版本的已接纳评价。'));
   const names:Record<string,string>={Q1:'问题覆盖',Q2:'机制解释',Q3:'跨源综合',Q4:'论证与证据',Q5:'反证与边界',Q6:'成稿与结论',citation_integrity:'引用核对'};
   for(const evaluation of evaluations)for(const result of evaluation.evaluation.results){const block=element('section','quality-item '+result.verdict);block.append(element('h3','',`${names[result.id]||result.id} · ${{pass:'达标',fail:'未达标',error:'检查错误'}[result.verdict]||'待核对'}`),element('p','quality-message',result.message));quality.append(block);}card.append(quality);main.append(card);
 }else{const empty=element('section','report-empty');empty.append(element('h2','','文章尚未形成'),element('p','muted','下方保留已接纳的范围、资料与分析。'));main.append(empty);}
 const sources=element('section','sources-card');sources.append(element('h2','','研究资料'));
 const packages=data.outputs.filter(o=>o.artifact.slot==='corpus');let count=0;
 for(const pkg of packages){const value=pkg.value as {sources:{source_id:string;origin:string;fact:Fact}[];limitations:string[]};for(const limit of value.limitations)notice(sources,limit,'info');for(const source of value.sources){count++;const fact=source.fact,row=element('div','source-row'),content=element('div');content.append(element('strong','',fact.title||fact.requested_url),element('p','source-url',fact.final_url||fact.requested_url),element('p','source-note',`${source.origin==='user-input'?'用户提供':'搜索发现'} · ${fact.reason}`));row.append(content);if(fact.usable)row.append(button('查看保存版本','quiet',()=>void showSource(id,pkg.artifact.id,undefined,source.source_id)));else row.append(element('span','badge muted','不可作为证据'));sources.append(row);}}
 if(!count)sources.append(element('p','muted','尚无已接纳的资料处置。'));main.append(sources);
 const process=element('details','process-card');process.append(element('summary','','研究范围与分析依据'));
 const plan=data.outputs.find(o=>o.artifact.slot==='plan');if(plan){process.append(element('h3','','必答问题'));for(const q of (plan.value as {subquestions:string[]}).subquestions)process.append(element('p','',q));}
 const analyses=data.outputs.filter(o=>o.artifact.slot==='analysis');for(const o of analyses){const value=o.value as {judgments:{text:string;limitations:string[]}[];gaps:string[]};process.append(element('h3','',o.stage==='synthesize'?'综合判断':'首轮判断'));for(const j of value.judgments)process.append(element('p','',j.text));for(const gap of value.gaps)notice(process,gap,'info');}process.append(diagnosticPanel(id));main.append(process);
 if(terminal.has(snapshot.status)&&snapshot.status!=='stopped')main.append(button('基于此继续研究','quiet',()=>{draft={question:snapshot.inputs.question,scope:snapshot.inputs.scope,urls:snapshot.inputs.seed_urls.join('\n'),key:crypto.randomUUID()};parentGoalId=id;navigate('/');}));
}
function diagnosticPanel(id:string){
 const section=element('section','execution-diagnostics');section.append(element('h3','','执行诊断'));
 const output=element('div');const read=button('读取本研究的运行与预算','quiet',()=>void refresh());
 const download=element('a','quiet','导出观测记录（本页最多100条）');download.href=`/api/research/${encodeURIComponent(id)}/trace/export?limit=100`;download.download='inquiry-trace.json';section.append(read,download,output);
 async function refresh(){read.disabled=true;output.replaceChildren(element('p','muted','正在读取公开运行事实…'));
  try{
   const [budget,trace]=await Promise.all([
    api<{budget:{runs_used:number;used:{active_ms:number;tool_calls:number};incomplete_active_subjects:string[]};available:{active_ms:number;tool_calls:number};effective_limits:{max_runs:number}}>(`/api/research/${encodeURIComponent(id)}/budgets`),
    api<{enabled:boolean;complete:boolean;issues:string[];events:{event:{kind:string;run_id?:string;state_id?:string;milkie_run_id?:string;data?:{code?:string}}}[]}>(`/api/research/${encodeURIComponent(id)}/trace?limit=100`)
   ]);
   if(!section.isConnected)return;output.replaceChildren();
   const unknown=budget.budget.incomplete_active_subjects.length>0;
   output.append(element('p','',`运行 ${budget.budget.runs_used}/${budget.effective_limits.max_runs} 次 · 工具调用 ${budget.budget.used.tool_calls} 次 · 剩余工具额度 ${budget.available.tool_calls} 次`),element('p','muted',`已记录活动时间 ${(budget.budget.used.active_ms/1000).toFixed(1)} 秒${unknown?'（计量有缺口）':''} · 剩余活动额度 ${(budget.available.active_ms/1000).toFixed(1)} 秒。未知用量不补零。`));
   if(!trace.enabled||!trace.complete)notice(output,'观测记录未完整采集'+(trace.issues.length?'：'+trace.issues.join('、'):'')+'。业务结果仍以文章及评价为准。','info');
   const records=element('details');records.append(element('summary','',`查看本页 ${trace.events.length} 条阶段、Agent与工具记录`));
   for(const item of trace.events){const event=item.event;records.append(element('p','diagnostic-event',`${event.state_id||'研究'} · ${event.kind}${event.data?.code?' · '+event.data.code:''}${event.milkie_run_id?' · Agent '+event.milkie_run_id:''}${event.run_id?' · '+event.run_id:''}`));}output.append(records);
  }catch(err){if(section.isConnected){output.replaceChildren();notice(output,errorText(err)+' 可重试读取；文章和评价保持原状态。');}}
  finally{read.disabled=false;}
 }
 return section;
}
function goalWaiting(main:HTMLElement,id:string,revision:number,goal:Goal){
 const wait=goal.waiting.find(w=>!w.consumed);if(!wait)return;
 const card=element('section','waiting-card');card.append(element('h2','','澄清研究问题或补充资料'),element('p','',wait.reason));
 const form=element('form'),label=element('label','','澄清内容');label.htmlFor='clarification';const text=element('textarea');text.id='clarification';text.rows=3;text.maxLength=4000;
 const urlLabel=element('label','','补充公开网址（可选，每行一个）');urlLabel.htmlFor='goal-sources';const urls=element('textarea');urls.id='goal-sources';urls.rows=3;
 const state=clarificationDrafts.get(id)||{text:'',urls:'',key:crypto.randomUUID()};clarificationDrafts.set(id,state);text.value=state.text;urls.value=state.urls;
 const error=element('div'),submit=element('button','primary','补充并继续');submit.type='submit';
 for(const input of [text,urls])input.addEventListener('input',()=>{state.text=text.value;state.urls=urls.value;state.key=crypto.randomUUID();});
 form.append(label,text,urlLabel,urls,error,submit);form.addEventListener('submit',async e=>{e.preventDefault();submit.disabled=true;try{await api(`/api/research/${encodeURIComponent(id)}/sources`,{clarification:state.text,urls:state.urls.split('\n').map(s=>s.trim()).filter(Boolean),wait_id:wait.wait_id,nonce:wait.nonce,request_key:state.key,expected_revision:revision});clarificationDrafts.delete(id);await load(id,true);}catch(err){notice(error,errorText(err)+' 输入已保留，可刷新核对。');}finally{submit.disabled=false;}});card.append(form);main.append(card);
}
function waiting(main:HTMLElement,id:string,revision:number){const card=element('section','waiting-card');card.innerHTML='<h2>补充一点线索，继续研究</h2><p>提供能弥补缺口的公开网页，每行一个。提交时会核对当前研究版本。</p><form><label for="additional-sources">补充网址</label><textarea id="additional-sources" rows="3" required placeholder="https://…"></textarea><div class="source-error"></div><button class="primary" type="submit">补充并继续 →</button></form>';const input=card.querySelector('textarea')!;const state=sourceDraft.get(id)||{text:'',key:crypto.randomUUID()};sourceDraft.set(id,state);input.value=state.text;input.addEventListener('input',()=>{state.text=input.value;state.key=crypto.randomUUID();});card.querySelector('form')!.addEventListener('submit',async e=>{e.preventDefault();const gen=generation;const submit=card.querySelector('button')!;submit.disabled=true;try{await api(`/api/research/${encodeURIComponent(id)}/sources`,{urls:state.text.split('\n').map(v=>v.trim()).filter(Boolean),request_key:state.key,expected_revision:revision});if(gen===generation){sourceDraft.delete(id);await load(id,true);}}catch(err){if(gen===generation){notice(card.querySelector<HTMLElement>('.source-error')!,errorText(err)+' 已保留网址；请刷新核对状态。');}}finally{submit.disabled=false;}});main.append(card);}
async function requestCancel(id:string){const gen=generation;const saved=cancelKeys.get(id)||{key:crypto.randomUUID(),revision:current?.snapshot.revision??0};cancelKeys.set(id,saved);try{await api(`/api/research/${encodeURIComponent(id)}/cancel`,{request_key:saved.key,expected_revision:saved.revision});if(gen===generation)await load(id,true);}catch(err){if(gen===generation){if(err instanceof Error&&(err as Error&{code?:string}).code==='revision_conflict')cancelKeys.delete(id);notice(document.querySelector<HTMLElement>('#read-error')!,errorText(err)+' 可刷新后再次请求停止。');}}}

async function requestResume(id:string){const gen=generation;const saved=resumeKeys.get(id)||{key:crypto.randomUUID(),revision:current?.snapshot.revision??0};resumeKeys.set(id,saved);try{await api(`/api/research/${encodeURIComponent(id)}/resume`,{request_key:saved.key,expected_revision:saved.revision});if(gen===generation)await load(id,true);}catch(err){if(gen===generation){if(err instanceof Error&&(err as Error&{code?:string}).code==='revision_conflict')resumeKeys.delete(id);notice(document.querySelector<HTMLElement>('#read-error')!,errorText(err)+' 可刷新后再次请求继续。');}}}

async function load(id:string,force=false){const gen=generation;if(timer)clearTimeout(timer);try{const data=await api<View>('/api/research/'+encodeURIComponent(id));if(gen!==generation)return;if(!app.inert&&(!current||force||data.snapshot.revision!==current.snapshot.revision)){const scroll=window.scrollY;current=data;detail(data,id);window.scrollTo(0,scroll);}else document.querySelector('#read-error')?.replaceChildren();if(app.inert||!terminal.has(data.snapshot.status))timer=setTimeout(()=>void load(id),1500);}catch(err){if(gen!==generation)return;if(!current){const main=shell();main.className='detail-page';main.append(link('← 新建研究','/','back-link'),element('h1','','暂时无法读取这项研究'),element('p','muted',errorText(err)),button('重新读取','primary',()=>void load(id,true)));}else{const target=document.querySelector<HTMLElement>('#read-error')!;target.replaceChildren();notice(target,errorText(err)+' 当前仍显示上次已确认的内容。');target.append(button('重试读取','quiet',()=>void load(id,true)));}timer=setTimeout(()=>void load(id),4000);}}
function closeDrawer(){drawerVersion++;drawer.replaceChildren();document.body.classList.remove('drawer-open');app.inert=false;if(focusBeforeDrawer?.isConnected)focusBeforeDrawer.focus();focusBeforeDrawer=null;}
async function showSource(id:string,artifact:string,citation?:Citation,sourceId=citation?.source_id){const gen=generation;const version=++drawerVersion;if(!app.inert)focusBeforeDrawer=document.activeElement as HTMLElement;app.inert=true;drawer.innerHTML='<div class="drawer-backdrop"></div><aside class="source-drawer" role="dialog" aria-modal="true" aria-labelledby="source-heading"><div class="drawer-head"><h2 id="source-heading">保存的来源</h2><button type="button" class="quiet close" aria-label="关闭来源面板">✕</button></div><div class="drawer-content"><p class="muted">正在读取保存版本…</p></div></aside>';document.body.classList.add('drawer-open');drawer.querySelector('.close')!.addEventListener('click',closeDrawer);drawer.querySelector('.drawer-backdrop')!.addEventListener('click',closeDrawer);(drawer.querySelector('.close') as HTMLButtonElement).focus();const content=drawer.querySelector<HTMLElement>('.drawer-content')!;
 try{const saved=await api<Saved>(`/api/research/${encodeURIComponent(id)}/artifacts/${encodeURIComponent(artifact)}${sourceId?'?source_id='+encodeURIComponent(sourceId):''}`);if(gen!==generation||version!==drawerVersion)return;if(citation&&citation.sha256!==saved.artifact.sha256)throw new Error('引用与保存版本不一致。');if(!saved.value?.paragraphs)throw new Error('这份资料没有可读取的规范化正文。');content.replaceChildren();const f=saved.value.fact;content.append(element('h3','',f.title||'来源内容'),link(f.final_url||f.requested_url,f.final_url||f.requested_url,'source-link'),element('p','muted',`保存于 ${new Date(f.captured_at).toLocaleString('zh-CN')}`));const fingerprint=element('details','fingerprint');fingerprint.append(element('summary','','核对内容版本'),element('code','',saved.artifact.sha256));content.append(fingerprint);if(f.truncated)notice(content,'规范化内容受大小边界限制；当前展示的是已保存节选。','info');const paragraphs=element('div','source-paragraphs');content.append(paragraphs);const draw=(all:boolean)=>{paragraphs.replaceChildren();const selected=citation?.paragraph;for(let index=0;index<saved.value!.paragraphs.length;index++){if(!all&&selected!==undefined&&Math.abs(index-selected)>1)continue;if(!all&&selected===undefined&&index>=12)continue;const p=element('p',index===selected?'selected':'');p.append(element('span','paragraph-number',`第${index+1}段`));const text=saved.value!.paragraphs[index];if(index===selected&&citation&&text.includes(citation.quote)){const position=text.indexOf(citation.quote);p.append(document.createTextNode(text.slice(0,position)),element('mark','',citation.quote),document.createTextNode(text.slice(position+citation.quote.length)));}else p.append(document.createTextNode(text));paragraphs.append(p);}if(!all)paragraphs.append(button('查看全部已保存内容','quiet',()=>draw(true)));};draw(false);}
 catch(err){if(gen===generation&&version===drawerVersion){content.replaceChildren();notice(content,errorText(err));content.append(button('重试','quiet',()=>void showSource(id,artifact,citation,sourceId)));}}
}
function route(){generation++;if(timer)clearTimeout(timer);current=null;closeDrawer();document.documentElement.dataset.build=import.meta.env.VITE_BUILD_SHA||'development';const match=location.pathname.match(/^\/research\/([^/]+)$/);if(match){const main=shell();main.className='detail-page';main.append(element('p','muted loading','正在读取研究…'));void load(decodeURIComponent(match[1]));}else rootPage();}
window.addEventListener('popstate',route);window.addEventListener('keydown',e=>{if(e.key==='Escape')closeDrawer();if(e.key==='Tab'&&app.inert){const focusable=[...drawer.querySelectorAll<HTMLElement>('button,a[href],textarea,input')];const first=focusable[0],last=focusable[focusable.length-1];if(e.shiftKey&&document.activeElement===first){e.preventDefault();last?.focus();}else if(!e.shiftKey&&document.activeElement===last){e.preventDefault();first?.focus();}}});route();
