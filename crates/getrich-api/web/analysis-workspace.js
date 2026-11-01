import {$,esc,fmt,amount,invoke} from '/runtime.js';
const labels=[['5日收益 %','return_5_pct'],['20日收益 %','return_20_pct'],['60日收益 %','return_60_pct'],['RSI14','rsi14'],['MA20','ma20'],['MA60','ma60'],['MACD DIF','macd_dif'],['MACD DEA','macd_dea'],['MACD 柱','macd_histogram'],['ATR14','atr14'],['20日年化波动 %','volatility20_annual_pct'],['60根最大回撤 %','max_drawdown60_pct'],['相对前20日均量','volume_ratio20']];
const modes=[['intraday','盘中短线'],['swing','日线波段'],['value','基本面长线']];
const prompts={intraday:['盘中结构与触发证据','失效条件与流动性风险','观察价位与仓位纪律','盘中执行复盘'],swing:['趋势结构与研究证据','波段失效条件与风险','观察区间与执行计划','波段复盘记录'],value:['商业逻辑与竞争优势','财务风险与反证','估值假设与跟踪事项','财报更新与假设复核']};
const noteFields=[['thesis','研究假设与证据'],['risks','风险与反证'],['plan','观察条件与计划'],['review','复盘记录']];
export class AnalysisWorkspace {
  constructor({current,active,modeChanged=()=>{},preferences=()=>({})}){
    this.current=current;this.active=active;this.preferences=preferences;this.modeChanged=modeChanged;this.mode='swing';this.drafts=new Map();this.reports=new Map();this.pending=new Set();this.saving=new Set();this.errors=new Map();this.histories=new Map();this.screen=null;
    this.screenCriteria={board:'CN',min_change_pct:2,min_turnover:1e8};
    $('research-panel').addEventListener('change',event=>{const field=event.target.dataset.screenField;if(!['board','min_change_pct','min_turnover'].includes(field))return;this.screenCriteria[field]=field==='board'?event.target.value:Number(event.target.value)*(field==='min_turnover'?1e8:1);});
    $('research-panel').addEventListener('input',event=>{const name=event.target.dataset.noteField;if(!noteFields.some(([f])=>f===name))return;const key=this.key(),draft=this.drafts.get(key);if(draft){draft[name]=event.target.value;draft._dirty=true;}});
    $('research-panel').addEventListener('click',event=>{const button=event.target.closest('[data-analysis-action]');if(!button||!this.active())return;event.preventDefault();this.action(button.dataset.analysisAction).catch(error=>{this.errors.set(this.key(),error.message);this.render();});});
  }
  key(){const q=this.current();return q?q.market+':'+q.symbol+':'+this.mode:'';}
  basis(){return this.mode==="swing"?(this.preferences().price_basis||"raw"):"raw";}
  reportKey(){return this.key()+":"+this.basis()+":"+(this.preferences().adjustment_source||"");}
  async load(){
    if(!this.initialized){
      this.initialized=invoke('settings.get',{}).then(s=>{const mode=s.preferences?.research_mode;if(!this.userModeChosen&&modes.some(([id])=>id===mode))this.mode=mode;}).catch(()=>{});
    }
    await this.initialized;
    const q=this.current(),key=this.key(),mode=this.mode,reportKey=this.reportKey(),basis=this.basis(),factor_source=this.preferences().adjustment_source||"";if(!q||!this.active()||this.pending.has(reportKey))return;
    this.pending.add(reportKey);this.render();
    try{const report=await invoke('stock.analysis',{symbol:q.symbol,market:q.market,mode,basis,factor_source});this.reports.set(reportKey,report);const old=this.drafts.get(key);if(!old||!old._dirty&&report.note.revision>=old.revision)this.drafts.set(key,{...report.note});this.errors.delete(key);}
    catch(error){this.errors.set(key,error.message);}
    finally{this.pending.delete(reportKey);this.render();}
  }
  async action(action){
    const q=this.current(),key=this.key(),mode=this.mode,reportKey=this.reportKey(),basis=this.basis(),factor_source=this.preferences().adjustment_source||"";if(!q)return;
    if(action.startsWith('mode:')){
      const next=action.slice(5);if(!modes.some(([id])=>id===next))return;
      this.userModeChosen=true;this.mode=next;this.render(true);
      // Serialize preference writes so fast switching cannot persist an older mode last.
      this.preferenceWrite=(this.preferenceWrite||Promise.resolve()).catch(()=>{}).then(()=>invoke('settings.save',{preferences:{research_mode:next}}));
      this.preferenceWrite.catch(error=>{this.errors.set(this.key(),error.message);this.render();});
      await this.load();return this.modeChanged();
    }
    if(action==='reload')return this.load();
    if(action==='discard'){this.drafts.set(key,{...this.reports.get(reportKey)?.note});return this.load();}
    if(action==='save'){
      if(this.saving.has(key))return;const draft=this.drafts.get(key);if(!draft)return;
      this.saving.add(key);this.render();
      try{const note=await invoke('research.note.save',{...draft,symbol:q.symbol,market:q.market,mode,basis,factor_source,analysis_as_of:this.reports.get(reportKey)?.evaluation_date});const saved={...note};delete saved._dirty;this.drafts.set(key,saved);const report=this.reports.get(reportKey);if(report)report.note=saved;this.errors.delete(key);}
      catch(error){this.errors.set(key,error.message);}finally{this.saving.delete(key);this.render();}return;
    }
    if(action==='history'){const value=await invoke('research.note.history',{symbol:q.symbol,market:q.market,mode});this.histories.set(key,value.rows);this.render();return;}
    if(action==='screen'||action==='watch-screen'||action==='custom-screen'){
      if(this.screenPending)return;this.screenPending=true;this.render();
      try{if(action==='screen')this.screenCriteria={board:'CN',min_change_pct:2,min_turnover:1e8};if(action==='watch-screen')this.screenCriteria={board:'WATCH',min_change_pct:0,min_turnover:0};this.screen=await invoke('research.screen',this.screenCriteria);}finally{this.screenPending=false;this.render();}
    }
  }
  render(force=false){
    if(!this.active())return;const key=this.key(),reportKey=this.reportKey(),r=this.reports.get(reportKey),d=this.drafts.get(key)||{},error=this.errors.get(key);
    if(!force&&document.activeElement?.dataset?.noteField&&d._dirty&&!this.saving.has(key))return;
    const panel=$('research-panel');
    let html=`<article class="card"><div class="actions"><div class="research-modes" role="group" aria-label="研究方式">${modes.map(([id,label])=>`<button data-analysis-action="mode:${id}" aria-pressed="${this.mode===id}" class="${this.mode===id?'active':''}">${label}</button>`).join('')}</div><button data-analysis-action="reload" ${this.pending.has(reportKey)?'disabled':''}>重算本地分析</button><span class="microcopy">本地计算 · 不触发服务器下载</span></div>${error?`<p class="error">${esc(error)}</p>`:''}`;
    if(!r){panel.innerHTML=html+'<p>正在读取本地研究数据…</p></article>';return;}
    const w=r.workspace||{},mode=this.mode;
    const fields=mode==='swing'?labels.map(([label,name])=>({label,value:r.metrics[name]})):(w.metrics||[]);
    html+=mode==='swing'?`<p class="microcopy">${esc(r.from||'—')} — ${esc(r.as_of||'—')} · ${r.sample_count} 根日线 · 数据版本 ${r.data_version}</p>`:`<p class="microcopy">研究截止 ${esc(r.evaluation_date||'—')} · 数据时间 ${esc(w.as_of||'—')} · 样本 ${w.sample_count||0}${mode==='value'?' · 财报期 '+esc(w.report_period||'—')+' · 公告日 '+esc(w.disclosed_at||'—'):''}</p>`;
    html+=(mode==='swing'?r.warnings:w.warnings||[]).map(w=>`<p class="microcopy">${esc(w)}</p>`).join('');
    html+=`<dl class="research-metrics analysis-metrics">${fields.map(f=>`<div><dt>${esc(f.label)}</dt><dd>${fmt(f.value)}</dd></div>`).join('')}</dl><details><summary>计算口径与来源</summary><p>${esc(w.methodology||r.methodology)}</p>${mode==='swing'?'<p>'+esc(r.sources.join('、'))+'</p>':Object.entries(w.provenance||{}).map(([name,p])=>`<p>${esc(name)} · ${esc(p.source||'—')} · 更新 ${esc(p.updated_at||'—')} · 状态 ${esc(p.status||'—')}${p.error?' · '+esc(p.error):''}</p>`).join('')}</details></article>`;
    const fieldsForNote=noteFields.map(([field],i)=>[field,prompts[mode][i]]);
    html+=`<article class="card"><h2>研究记录</h2><p class="microcopy">草稿按股票与研究方式分别保留在当前会话；点击保存写入本地数据库，每次保存保留一个历史版本。</p><fieldset ${this.saving.has(key)?'disabled':''}><div class="journal-fields">${fieldsForNote.map(([field,label])=>`<label class="field">${label}<textarea rows="3" maxlength="10000" data-note-field="${field}">${esc(d[field]||'')}</textarea></label>`).join('')}</div><div class="actions"><button class="primary" data-analysis-action="save">保存研究记录</button><button data-analysis-action="discard">放弃草稿，载入已保存版本</button><button data-analysis-action="history">查看最近100个版本</button><span>版本 ${d.revision||0}${d._dirty?' · 未保存':''}</span></div></fieldset>`;
    if(this.histories.has(key))html+=this.histories.get(key).map(note=>`<details><summary>版本 ${note.revision} · ${esc(note.updated_at||'')}</summary>${fieldsForNote.map(([field,label])=>`<h3>${label}</h3><p class="journal-text">${esc(note[field]||'—')}</p>`).join('')}<p class="microcopy">保存时数据日期 ${esc(note.analysis_snapshot?.workspace?.as_of||note.analysis_snapshot?.as_of||'无有效数据')} · 数据批次 ${note.analysis_snapshot?.data_version||0} · ${esc(note.analysis_snapshot?.price_basis==="qfq"?"前复权":note.analysis_snapshot?.price_basis==="hfq"?"后复权":"原始价格 / 未复权")}</p><dl class="research-metrics">${(mode==='swing'?labels.slice(0,4).map(([label,name])=>({label,value:note.analysis_snapshot?.metrics?.[name]})):note.analysis_snapshot?.workspace?.metrics||[]).map(f=>`<div><dt>${esc(f.label)}</dt><dd>${fmt(f.value)}</dd></div>`).join('')}</dl></details>`).join('');
    html+=`</article><article class="card"><h2>本地条件选股</h2><div class="inline-form"><label class="field">范围<select data-screen-field="board"><option value="CN" ${this.screenCriteria.board==='CN'?'selected':''}>沪深京A股</option><option value="WATCH" ${this.screenCriteria.board==='WATCH'?'selected':''}>A股自选</option></select></label><label class="field">最低涨幅 %<input type="number" min="-100" max="100" step="0.1" data-screen-field="min_change_pct" value="${this.screenCriteria.min_change_pct}"></label><label class="field">最低成交额（亿元）<input type="number" min="0" step="0.1" data-screen-field="min_turnover" value="${this.screenCriteria.min_turnover/1e8}"></label><button data-analysis-action="custom-screen" ${this.screenPending?'disabled':''}>运行条件选股</button></div><div class="actions">`;html+='<button data-analysis-action="screen" '+(this.screenPending?'disabled':'')+'>活跃股：涨幅≥2% / 成交额≥1亿元</button><button data-analysis-action="watch-screen" '+(this.screenPending?'disabled':'')+'>A股自选：涨幅≥0%</button></div>';
    if(this.screen){const s=this.screen;html+=`<p class="microcopy">${esc(s.as_of||'无有效日期')} · 可用 ${s.eligible} · 排除 ${s.excluded} · 匹配 ${s.matched}（最多100）</p><p class="microcopy">${esc(s.notice)}</p><div class="table-wrap"><table><thead><tr><th>股票</th><th>涨幅</th><th>成交额 CNY</th></tr></thead><tbody>${s.rows.map(row=>`<tr><td><a href="#/stock/${encodeURIComponent(row.market)}/${encodeURIComponent(row.symbol)}?board=CN">${esc(row.name||row.symbol)} · ${esc(row.symbol)}</a></td><td>${fmt(row.change_pct)}%</td><td>${amount(row.turnover)}</td></tr>`).join('')}</tbody></table></div>`;}
    panel.innerHTML=html+'</article>';
  }
}
