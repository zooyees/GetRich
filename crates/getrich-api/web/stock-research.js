import {$,esc,fmt,amount,time,api,invoke,statusZh} from "/runtime.js";

const groups={history:["history"],intraday:["intraday"],f10:["valuation","company","financial","holders"],funds:["funds"],announcements:["announcements"]};
export const safeLink=value=>{try{const url=new URL(value);return ["http:","https:"].includes(url.protocol)&&!url.username&&!url.password?url.href:null;}catch{return null;}};
const text=value=>value==null?"—":String(value);
const metric=(name,value)=>`<div><dt>${esc(name)}</dt><dd>${esc(value)}</dd></div>`;
const note=data=>`${data?.error?`<p class="error">${esc(data.error)}${data.rows?.length?"；以下保留上次成功数据。":""}</p>`:""}<p class="microcopy">${esc(data?.source||"Tushare")} · 数据日期 ${esc(data?.as_of||"—")} · 获取 ${esc(data?.updated_at?time(data.updated_at):"尚未获取")}</p>`;
const active=job=>["queued","running","cancelling"].includes(job?.status);
export class StockResearch {
  constructor({current,configured,reload,isActive}){
    this.current=current;this.configured=configured;this.reload=reload;this.isActive=isActive;this.key="";this.tab="history";this.intraday=false;this.data={};this.attempted=new Set();this.jobs=new Map();this.pending=new Set();this.errors=new Map();this.generation=0;
    $("research-tabs").onclick=event=>{const tab=event.target.closest("[data-research]")?.dataset.research;if(!tab)return;this.tab=tab;this.render();this.ensure(tab).catch(e=>this.fail(tab,e));};
    $("research-refresh").onclick=()=>this.ensure(this.section(),true).catch(e=>this.fail(this.section(),e));
    setInterval(()=>{if(this.isActive()&&this.section()==="intraday")this.update();},30000);
    new ResizeObserver(()=>this.draw()).observe($("intraday-canvas"));
    $("intraday-canvas").onpointermove=event=>{const rect=event.target.getBoundingClientRect();this.hover=(event.clientX-rect.left-60)/(rect.width-120)*240;this.draw();};
    $("intraday-canvas").onpointerleave=()=>{this.hover=null;this.draw();};
  }
  section(){return this.tab==="history"&&this.intraday?"intraday":this.tab;}
  id(section){return this.key+":"+section;}
  fail(section,error){this.errors.set(this.id(section),error.message);this.render();}
  async update(){
    const quote=this.current();if(!quote)return;
    const key=quote.market+":"+quote.symbol;
    if(key!==this.key){this.key=key;this.data={};this.generation++;}
    const generation=this.generation;
    try{const data=await invoke("stock.research",{symbol:quote.symbol,market:quote.market});if(generation!==this.generation||key!==this.key||!this.isActive())return;this.data=data;this.render();await this.ensure("history");if(this.section()!=="history")await this.ensure(this.section());}
    catch(e){if(generation===this.generation)this.fail(this.section(),e);}
  }
  async ensure(section,force=false){
    const quote=this.current();if(!quote||!this.isActive()||!quote.market.startsWith("CN.")||!this.configured())return;
    const id=this.id(section);if(this.pending.has(id)||active(this.jobs.get(id)))return;
    const needs=groups[section]?.some(name=>this.data.datasets?.[name]?.needs_refresh);
    const live=["intraday","history"].includes(section)&&this.data.datasets?.[section]?.status==="ready";
    if(!force&&(!needs||(this.attempted.has(id)&&!live)))return;
    this.pending.add(id);this.attempted.add(id);this.errors.delete(id);this.render();
    try{const job=await invoke("job.submit",{kind:"stock.research",params:{symbol:quote.symbol,market:quote.market,section}});this.jobs.set(id,job);this.poll(id,job.id);}
    catch(e){this.errors.set(id,e.message);}
    finally{this.pending.delete(id);this.render();}
  }
  async poll(id,jobId){
    clearTimeout(this.timers?.get(id));this.timers||=new Map();
    this.timers.set(id,setTimeout(async()=>{
      if(!this.isActive()||!id.startsWith(this.key+":")){this.jobs.delete(id);return;}
      try{const job=await invoke("job.get",{id:jobId});this.jobs.set(id,job);this.render();if(active(job))this.poll(id,jobId);else{await this.update();if(id.endsWith(":history"))await this.reload();}}
      catch(e){this.errors.set(id,e.message);this.jobs.delete(id);this.render();}
    },1500));
  }
  setIntraday(value){this.intraday=value;this.tab="history";this.render();if(value)this.ensure("intraday").catch(e=>this.fail("intraday",e));}
  render(){
    if(!this.isActive()||!this.current())return;
    const section=this.section(),id=this.id(section),job=this.jobs.get(id),pending=this.pending.has(id)||active(job);
    $("research-tabs").querySelectorAll("button").forEach(button=>{const selected=button.dataset.research===this.tab;button.setAttribute("aria-selected",String(selected));button.classList.toggle("active",selected);});
    $("candle-panel").hidden=this.tab!=="history"||this.intraday;$("intraday-panel").hidden=section!=="intraday";$("research-panel").hidden=this.tab==="history";
    $("research-refresh").disabled=pending;$("research-refresh").textContent=section==="history"?"更新完整历史":"刷新数据";
    const message=this.errors.get(id)||job?.error||job?.result?.warnings?.join("；")||(pending?`${statusZh(job?.status||"queued")} ${job?.phase||"正在提交"}`:!this.configured()?"连接 Tushare 后可获取数据":"");
    $("research-status").textContent=message;$("research-status").className=job?.error||this.errors.has(id)?"error":"muted";
    const ds=this.data.datasets||{};
    if(section==="intraday"){$("intraday-note").innerHTML=note(ds.intraday);this.draw();}
    if(this.tab==="f10")this.renderF10(ds);
    if(this.tab==="funds")this.renderFunds(ds.funds);
    if(this.tab==="announcements")this.renderAnnouncements(ds.announcements);
    if(section==="history")$("history-range").innerHTML=note(ds.history);
  }
  renderF10(ds){
    const c=ds.company?.rows?.[0]||{},f=ds.financial?.rows?.[0]||{},v=ds.valuation?.rows?.[0]||{};
    const metrics=values=>`<dl class="research-metrics">${values.map(([name,value])=>metric(name,value)).join("")}</dl>`;
    $("research-panel").innerHTML=`<article class="card"><h2>估值与股本</h2>${metrics([["总市值",amount(v.total_mv)],["流通市值",amount(v.circ_mv)],["市盈率 TTM",fmt(v.pe_ttm)],["市净率",fmt(v.pb)],["换手率 %",fmt(v.turnover_rate)],["量比",fmt(v.volume_ratio)],["股息率 TTM %",fmt(v.dv_ttm)],["总股本（股）",amount(v.total_share)]])}${note(ds.valuation)}</article><article class="card"><h2>公司概况</h2>${[["公司全称","com_name"],["法人代表","chairman"],["总经理","manager"],["董秘","secretary"],["员工人数","employees"],["办公地址","office"],["主营业务","main_business"]].map(([label,field])=>`<p><span class="muted">${label}：</span>${esc(text(c[field]))}</p>`).join("")}<details><summary>公司介绍</summary><p>${esc(text(c.introduction))}</p></details>${note(ds.company)}</article><article class="card"><h2>财务指标</h2><p class="muted">报告期 ${esc(text(f.end_date))} · 公告日期 ${esc(text(f.ann_date))}</p>${metrics([["每股收益",fmt(f.eps)],["每股净资产",fmt(f.bps)],["ROE %",fmt(f.roe)],["毛利率 %",fmt(f.grossprofit_margin)],["净利率 %",fmt(f.netprofit_margin)],["资产负债率 %",fmt(f.debt_to_assets)],["营收同比 %",fmt(f.or_yoy)],["净利润同比 %",fmt(f.netprofit_yoy)]])}${note(ds.financial)}</article><article class="card"><h2>前十大股东</h2><div class="table-wrap"><table><thead><tr><th>股东名称</th><th>持有股数</th><th>占比 %</th></tr></thead><tbody>${(ds.holders?.rows||[]).map(r=>`<tr><td>${esc(text(r.holder_name))}</td><td>${amount(r.hold_amount)}</td><td>${fmt(r.hold_ratio)}</td></tr>`).join("")||'<tr><td colspan="3">暂无股东数据</td></tr>'}</tbody></table></div>${note(ds.holders)}</article>`;
  }
  renderFunds(data){const rows=data?.rows||[],last=rows[0]||{};$("research-panel").innerHTML=`<article class="card"><h2>资金流向</h2><dl class="research-metrics">${[["净流入（供应商口径）",last.net_mf_amount],["大单净额",last.large_net],["超大单净额",last.extra_large_net]].map(([label,value])=>metric(label,amount(value))).join("")}</dl><p class="microcopy">金额为元；净流入按供应商口径，可能与分类差额之和不同。</p>${note(data)}<div class="table-wrap"><table><thead><tr><th>日期</th><th>净流入</th><th>大单净额</th><th>超大单净额</th></tr></thead><tbody>${rows.map(r=>`<tr><td>${esc(text(r.trade_date))}</td>${["net_mf_amount","large_net","extra_large_net"].map(f=>`<td class="num ${r[f]>0?"up":r[f]<0?"down":""}">${amount(r[f])}</td>`).join("")}</tr>`).join("")||'<tr><td colspan="4">暂无资金数据</td></tr>'}</tbody></table></div></article>`;}
  renderAnnouncements(data){$("research-panel").innerHTML=`<article class="card"><h2>公司公告</h2><a href="https://www.cninfo.com.cn/new/index" target="_blank" rel="noopener noreferrer">巨潮资讯 · 官方公告查询 ↗</a><p class="muted">近一年披露记录 · 点击标题打开原文</p>${note(data)}<div class="announcement-list">${(data?.rows||[]).map(r=>{const url=safeLink(r.url);return `<article><time>${esc(text(r.ann_date))}</time>${url?`<a href="${esc(url)}" target="_blank" rel="noopener noreferrer">${esc(text(r.title))} ↗</a>`:`<span>${esc(text(r.title))}</span>`}</article>`;}).join("")||'<p>暂无公告记录</p>'}</div></article>`;}
  draw(){
    if(!this.isActive()||this.section()!=="intraday")return;
    const canvas=$("intraday-canvas"),w=canvas.clientWidth,h=canvas.clientHeight;if(!w||!h)return;
    const ctx=canvas.getContext("2d"),dpr=window.devicePixelRatio||1;canvas.width=w*dpr;canvas.height=h*dpr;ctx.setTransform(dpr,0,0,dpr,0,0);ctx.clearRect(0,0,w,h);
    const rows=this.data.datasets?.intraday?.rows||[],css=getComputedStyle(document.documentElement),muted=css.getPropertyValue("--muted"),line=css.getPropertyValue("--line");ctx.fillStyle=muted;ctx.font="11px system-ui";
    if(!rows.length){ctx.textAlign="center";ctx.fillText("暂无分钟数据，请刷新或查看接口权限提示",w/2,h/2);return;}
    const prev=this.data.datasets?.intraday?.prev_close,values=rows.flatMap(r=>[r.close,r.vwap].filter(v=>v>0)),center=prev>0?prev:(Math.max(...values)+Math.min(...values))/2,delta=Math.max(...values.map(v=>Math.abs(v-center)),center*.005)*1.08;
    const left=60,right=w-60,top=20,bottom=h-90,x=t=>left+t/240*(right-left),y=p=>top+(center+delta-p)/(2*delta)*(bottom-top);
    for(let i=0;i<5;i++){const price=center+delta*(1-i/2),py=y(price);ctx.strokeStyle=line;ctx.beginPath();ctx.moveTo(left,py);ctx.lineTo(right,py);ctx.stroke();ctx.textAlign="right";ctx.fillStyle=muted;ctx.fillText(price.toFixed(2),left-8,py+4);if(prev>0){ctx.textAlign="left";ctx.fillText(((price/prev-1)*100).toFixed(2)+"%",right+8,py+4);}}
    for(const [field,color]of [["close","#4b9aff"],["vwap","#d9aa42"]]){ctx.strokeStyle=color;ctx.beginPath();let last=null;for(const r of rows){if(r[field]==null)continue;if(last==null||r.minute-last>2)ctx.moveTo(x(r.minute),y(r[field]));else ctx.lineTo(x(r.minute),y(r[field]));last=r.minute;}ctx.stroke();}
    const maxVol=Math.max(1,...rows.map(r=>r.volume||0));ctx.fillStyle="#4b9aff66";for(const r of rows)ctx.fillRect(x(r.minute),h-28-(r.volume||0)/maxVol*45,Math.max(1,(right-left)/300),(r.volume||0)/maxVol*45);
    ctx.fillStyle=muted;ctx.textAlign="center";for(const [minute,label]of [[0,"09:30"],[60,"10:30"],[120,"11:30 / 13:00"],[180,"14:00"],[240,"15:00"]])ctx.fillText(label,x(minute),h-8);
    const selected=this.hover==null?rows.at(-1):rows.reduce((a,b)=>Math.abs(a.minute-this.hover)<Math.abs(b.minute-this.hover)?a:b);
    $("intraday-meta").textContent=`${selected.time} · 价格 ${fmt(selected.close)} · 均价 ${fmt(selected.vwap)} · 成交量 ${amount(selected.volume)} 股 · 成交额 ${amount(selected.amount)}`;
  }
}
