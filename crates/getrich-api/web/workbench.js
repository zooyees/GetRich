import {$,esc,fmt,invoke,toast} from '/runtime.js';
const palette=['#76a7ff','#e2a941','#3ecab4','#c68ef0','#e65c77','#b3c1d5','#e0c464'];
const value=v=>v==null?'—':typeof v==='number'?fmt(v):typeof v==='boolean'?(v?'是':'否'):String(v);
const safeUrl=raw=>{try{const u=new URL(raw);return ['http:','https:'].includes(u.protocol)&&!u.username&&!u.password?u.href:null;}catch{return null;}};
export function csvExport(table){const quote=v=>{let s=v==null?'':String(v);if(typeof v==='string'&&/^[=+@\-\t\r]/.test(s))s="'"+s;return '"'+s.replaceAll('"','""')+'"';};return [table.columns.map(c=>quote(c.label)).join(','),...table.rows.map(r=>table.columns.map(c=>quote(r[c.key])).join(','))].join('\r\n');}
export class Workbench{
 constructor({active,openStock,openResearch}){
  this.active=active;this.openStock=openStock;this.openResearch=openResearch;this.tool='overview';this.group='概览';this.forms=new Map();this.results=new Map();this.errors=new Map();this.busy=new Set();this.jobs=new Map();this.hidden=new Set();this.generation=0;
  const root=$('view-workbench');root.addEventListener('click',e=>{const button=e.target.closest('[data-pro-action]');if(!button)return;e.preventDefault();this.action(button.dataset).catch(error=>{this.errors.set(this.tool,error.message);this.render();});});
  root.addEventListener('input',e=>{if(!e.target.dataset.proField)return;const form=this.form();form[e.target.dataset.proField]=e.target.type==='checkbox'?e.target.checked:e.target.value;});
  root.addEventListener('change',e=>{const f=e.target.dataset.proField;if(f){this.form()[f]=e.target.type==='checkbox'?e.target.checked:e.target.value;if(f==='dataset')this.render(true);}if(e.target.dataset.proSeries){const name=e.target.dataset.proSeries;if(e.target.checked)this.hidden.delete(name);else this.hidden.add(name);this.draw();}});
  root.addEventListener('change',e=>{if(e.target.id!=='pro-file'||!e.target.files?.length)return;const file=e.target.files[0];const tool=this.tool;if(file.size>4000000){toast('CSV文件最多4MB',true);return;}file.text().then(text=>{this.form(tool).csv=text;if(this.tool===tool)this.render();}).catch(error=>toast(error.message,true));});
  window.addEventListener('resize',()=>{clearTimeout(this.resizeTimer);this.resizeTimer=setTimeout(()=>this.draw(),80);});
  setInterval(()=>{if(this.active()&&['overview','alerts.list'].includes(this.tool)&&!this.busy.has(this.tool)&&!document.activeElement?.dataset?.proField)this.execute(this.tool,true).catch(()=>{});},15000);
 }
 schema(tool=this.tool){return this.catalog?.tools?.find(t=>t.id===tool);}
 form(tool=this.tool){if(!this.forms.has(tool)){const defaults={};for(const f of this.schema(tool)?.fields||[])defaults[f.id]=f.default;this.forms.set(tool,defaults);}return this.forms.get(tool);}
 async load(){if(!this.catalog)this.catalog=await invoke('workbench.catalog');if(!this.active())return;this.render();if(!this.results.has(this.tool))await this.execute(this.tool);}
 async execute(tool=this.tool,passive=false){
  if(this.busy.has(tool))return;const values={...this.form(tool)};this.busy.add(tool);if(!passive)this.render();
  try{const result=await invoke('workbench.execute',{tool,values});this.results.set(tool,result);this.errors.delete(tool);if(result.form_patch)Object.assign(this.form(tool),result.form_patch);if(result.job_id){this.jobs.set(tool,result.job_id);this.poll(tool,result.job_id);}if(result.open_research)await this.openResearch(result.open_research);}
  catch(error){this.errors.set(tool,error.message);}finally{if(!this.jobs.has(tool))this.busy.delete(tool);this.render();}
 }
 async poll(tool,id){
  clearTimeout(this['jobTimer:'+tool]);this['jobTimer:'+tool]=setTimeout(async()=>{
   try{const job=await invoke('job.get',{id});if(['queued','running','cancelling'].includes(job.status)){this.progress||=new Map();this.progress.set(tool,job);this.render();this.poll(tool,id);return;}if(['succeeded','partial'].includes(job.status))this.results.set(tool,job.result);else this.errors.set(tool,job.error||'任务取消或中断，可在数据中心重试');}
   catch(error){this.errors.set(tool,error.message);}this.busy.delete(tool);this.jobs.delete(tool);this.progress?.delete(tool);this.render();
  },1500);
 }
 async action(data){
  const action=data.proAction;
  if(action==='group'){this.group=data.group;this.tool=this.catalog.tools.find(t=>t.group===this.group)?.id||'overview';this.selected=null;this.render();return;}
  if(action==='tool'){this.tool=data.tool;this.selected=null;this.render();if(this.tool==='overview'&&!this.results.has(this.tool))await this.execute();return;}
  if(action==='run')return this.execute();
  if(action==='stock'){const current=this.currentStock?.();if(current){Object.assign(this.form(),{symbol:current.symbol,market:current.market});this.render();}return;}
  const result=this.results.get(this.tool),table=result?.tables?.[Number(data.table)],row=table?.rows?.[Number(data.row)];
  if((action==='load'&&row?.load)||(action==='apply'&&row?.apply)){const request=action==='apply'?row.apply:row.load;this.forms.set(request.tool,{...request.values});this.tool=request.tool;this.group=this.schema()?.group||this.group;this.selected=null;this.render();return;}
  if(action==='open'&&row?.open_stock)return this.openStock(row.open_stock);
  if(action==='detail'&&row){this.selected={table,row};this.render();return;}
  if(action==='export'&&table){const content='\ufeff'+csvExport(table);const blob=new Blob([content],{type:'text/csv;charset=utf-8'});const url=URL.createObjectURL(blob);const anchor=document.createElement('a');anchor.href=url;anchor.download='GetRich-'+this.tool+'.csv';anchor.click();setTimeout(()=>URL.revokeObjectURL(url),5000);return;}
  if(action==='snapshot'&&result){const blob=new Blob([JSON.stringify({result,exported_at:new Date().toISOString()},null,2)],{type:'application/json'});const url=URL.createObjectURL(blob);const a=document.createElement('a');a.href=url;a.download='GetRich-研究结果.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),5000);}
 }
 render(force=false){
  if(!this.active()||!this.catalog)return;
  const root=$('view-workbench'),schema=this.schema(),form=this.form(),pending=this.busy.has(this.tool);const result=this.results.get(this.tool),error=this.errors.get(this.tool);
  // Background refresh must not replace an active form while the user is typing.
  if(!force&&document.activeElement?.dataset?.proField&&!pending)return;
  let html='<div class="page-heading"><div><span class="eyebrow">PROFESSIONAL / 专业研究</span><h1>专业工作台</h1><p class="muted">统一的数据、研究规则与可审查结果，保存在本地数据库。</p></div></div>';
  html+='<nav class="pro-groups" aria-label="专业功能分组">'+this.catalog.groups.map(g=>`<button data-pro-action="group" data-group="${esc(g)}" aria-pressed="${this.group===g}" class="${this.group===g?'active':''}">${esc(g)}</button>`).join('')+'</nav><nav class="pro-tools" aria-label="功能">'+this.catalog.tools.filter(t=>t.group===this.group).map(t=>`<button data-pro-action="tool" data-tool="${esc(t.id)}" class="${t.id===this.tool?'active':''}">${esc(t.label)}</button>`).join('')+'</nav>';
  html+=`<article class="card pro-form-card"><h2>${esc(schema?.label||'工作台')}</h2><p class="microcopy">${esc(schema?.help||'')}</p><fieldset ${pending?'disabled':''}><div class="pro-fields">`;
  for(const f of schema?.fields||[]){if(f.kind==='hidden')continue;const v=form[f.id]??'',id='pro-field-'+f.id;html+=`<label class="field ${['csv','multiline'].includes(f.kind)?'pro-wide':''}" for="${id}">${esc(f.label)}`;
   if(f.kind==='select')html+=`<select id="${id}" data-pro-field="${esc(f.id)}">${f.options.map(o=>`<option value="${esc(o.value)}" ${String(v)===o.value?'selected':''}>${esc(o.label)}</option>`).join('')}</select>`;
   else if(f.kind==='checkbox')html+=`<span><input id="${id}" type="checkbox" data-pro-field="${esc(f.id)}" ${v?'checked':''}>启用</span>`;
   else if(['csv','multiline'].includes(f.kind))html+=`<textarea id="${id}" rows="${f.kind==='csv'?7:3}" maxlength="4000000" data-pro-field="${esc(f.id)}" spellcheck="false">${esc(v)}</textarea>`;
   else html+=`<input id="${id}" type="${f.kind==='date'?'date':f.kind==='number'?'number':'text'}" ${f.kind==='number'?'step="any"':''} data-pro-field="${esc(f.id)}" value="${esc(v)}">`;html+='</label>';
  }
  html+='</div>';if(this.tool==='data.import')html+=`<p class="microcopy">表头：<code>${esc(this.catalog.csv_headers[form.dataset]||'')}</code></p>`;
  html+=`<div class="actions"><button class="primary" data-pro-action="run">${pending?'后台处理中…':'执行'}</button>${(schema?.fields||[]).some(f=>f.kind==='csv')?'<label class="button">选择CSV文件<input id="pro-file" class="sr-only" type="file" accept=".csv,text/csv"></label>':''}</div></fieldset>${error?`<p class="error" role="alert">${esc(error)}</p>`:''}</article>`;
  const progress=this.progress?.get(this.tool);if(progress)html+=`<p role="status">${esc(progress.phase)} · ${progress.progress}%</p>`;
  if(result){html+=`<article class="card pro-result"><div class="actions"><h2>${esc(result.title||'执行结果')}</h2><button data-pro-action="snapshot">导出结果与口径</button></div><dl class="research-metrics pro-summary">${(result.summary||[]).map(r=>`<div><dt>${esc(r.label)}</dt><dd>${esc(value(r.value))}</dd></div>`).join('')}</dl>${(result.notices||[]).map(n=>`<p class="microcopy">${esc(n)}</p>`).join('')}${result.run_id?`<p class="microcopy">运行 ${result.run_id} · 输入指纹 ${esc(result.fingerprint)}</p>`:''}`;
   if(result.series?.length){const panes=[...new Set(result.series.map(s=>s.pane))];for(const pane of panes)html+=`<section class="pro-chart"><h3>${esc(pane)}</h3><div class="pro-legends">${result.series.filter(s=>s.pane===pane).map((s,i)=>`<label style="color:${palette[i%palette.length]}"><input type="checkbox" data-pro-series="${esc(s.name)}" ${this.hidden.has(s.name)?'':'checked'}>${esc(s.name)}</label>`).join('')}</div><canvas data-pro-pane="${esc(pane)}" role="img" aria-label="${esc(pane)}曲线"></canvas></section>`;}
   for(const [i,t] of (result.tables||[]).entries())html+=`<section class="pro-table-section"><div class="actions"><h3>${esc(t.title)}</h3><button data-pro-action="export" data-table="${i}">导出CSV</button><span class="microcopy">${t.rows.length} 条</span></div><div class="table-wrap pro-table"><table><thead><tr>${t.columns.map(c=>`<th>${esc(c.label)}</th>`).join('')}<th>操作</th></tr></thead><tbody>${t.rows.slice(0,500).map((r,j)=>`<tr>${t.columns.map(c=>`<td title="${esc(value(r[c.key]))}">${esc(value(r[c.key]).slice(0,120))}</td>`).join('')}<td class="pro-row-actions">${r.load?`<button data-pro-action="load" data-table="${i}" data-row="${j}">载入</button>`:''}${r.apply?`<button data-pro-action="apply" data-table="${i}" data-row="${j}">使用参数</button>`:''}${r.open_stock?`<button data-pro-action="open" data-table="${i}" data-row="${j}">个股</button>`:''}<button data-pro-action="detail" data-table="${i}" data-row="${j}">详情</button></td></tr>`).join('')||`<tr><td colspan="${t.columns.length+1}" class="muted">暂无记录</td></tr>`}</tbody></table></div></section>`;
   if(this.selected){const {table,row}=this.selected;html+='<section class="pro-detail"><h3>选中记录详情</h3>'+table.columns.map(c=>{const url=c.key==='url'?safeUrl(row[c.key]):null;return `<h4>${esc(c.label)}</h4>${url?`<a href="${esc(url)}" target="_blank" rel="noopener noreferrer">打开原文 ↗</a>`:`<p class="journal-text">${esc(value(row[c.key]))}</p>`}`;}).join('')+'</section>';}
   html+='</article>';
  }
  root.innerHTML=html;this.draw();
 }
 draw(){
  if(!this.active())return;const result=this.results.get(this.tool);if(!result?.series)return;
  for(const canvas of $('view-workbench').querySelectorAll('canvas[data-pro-pane]')){
   const series=result.series.filter(s=>s.pane===canvas.dataset.proPane&&!this.hidden.has(s.name));const width=canvas.clientWidth,height=230;if(!width)continue;const ratio=window.devicePixelRatio||1;canvas.width=width*ratio;canvas.height=height*ratio;const ctx=canvas.getContext('2d');ctx.scale(ratio,ratio);ctx.clearRect(0,0,width,height);const all=series.flatMap(s=>s.points.map(p=>p.value).filter(v=>typeof v==='number'&&Number.isFinite(v)));if(!all.length){ctx.fillStyle='#8f9fb7';ctx.fillText('没有可绘制数据',20,30);continue;}
   let min=Math.min(...all),max=Math.max(...all);if(canvas.dataset.proPane==="RSI"){min=0;max=100;}else if(canvas.dataset.proPane==="MACD"){min=Math.min(min,0);max=Math.max(max,0);}else if(canvas.dataset.proPane==="ATR")min=0;if(Math.abs(max-min)<.000001){const padding=Math.max(Math.abs(max)*.001,.001);min-=padding/2;max+=padding/2;}const span=max-min,left=70,right=width-16,top=15,bottom=height-28;
   ctx.font='11px system-ui';for(let i=0;i<=4;i++){const y=top+(bottom-top)*i/4;ctx.strokeStyle='#27364b';ctx.beginPath();ctx.moveTo(left,y);ctx.lineTo(right,y);ctx.stroke();ctx.fillStyle='#8f9fb7';ctx.textAlign='right';ctx.fillText(fmt(max-span*i/4),left-6,y+4);}
   series.forEach(s=>{const j=result.series.filter(x=>x.pane===canvas.dataset.proPane).findIndex(x=>x.name===s.name);ctx.strokeStyle=palette[j%palette.length];ctx.lineWidth=1.4;ctx.beginPath();let previous=false;for(let i=0;i<s.points.length;i++){const v=s.points[i].value;if(typeof v!=='number'||!Number.isFinite(v)){previous=false;continue;}const x=left+i/Math.max(1,s.points.length-1)*(right-left),y=bottom-(v-min)/span*(bottom-top);if(s.style==="histogram"){const zero=bottom-(0-min)/span*(bottom-top),barWidth=Math.max(1,Math.min(10,(right-left)/s.points.length*.7));ctx.fillStyle=v>=0?"#ff7182":"#45c79a";ctx.fillRect(x-barWidth/2,Math.min(y,zero),barWidth,Math.abs(zero-y));previous=false;continue;}if(previous)ctx.lineTo(x,y);else ctx.moveTo(x,y);previous=true;}ctx.stroke();});
   const points=series[0]?.points||[];if(points.length){ctx.fillStyle='#8f9fb7';ctx.textAlign='left';ctx.fillText(points[0].date,left,height-6);ctx.textAlign='right';ctx.fillText(points.at(-1).date,right,height-6);}
   canvas.onpointermove=event=>{const rect=canvas.getBoundingClientRect();const fraction=Math.max(0,Math.min(1,(event.clientX-rect.left-left)/(right-left)));canvas.title=series.map(s=>{const p=s.points[Math.round(fraction*(s.points.length-1))];return `${p?.date||''} ${s.name} ${value(p?.value)}`;}).join('\n');};
  }
 }
}
