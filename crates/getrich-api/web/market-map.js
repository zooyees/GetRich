import {$,esc,fmt,pct,amount,direction,api,time,invoke,statusZh} from "/runtime.js";

export function tileColor(value,green=false){
  if(value==null||!Number.isFinite(value))return "rgb(55,65,81)";
  const neutral=[46,54,68],up=green?[18,156,110]:[233,66,83],down=green?[233,66,83]:[18,156,110];
  const target=value>=0?up:down,t=Math.sqrt(Math.min(1,Math.abs(value)/5));
  return `rgb(${neutral.map((v,i)=>Math.round(v+(target[i]-v)*t)).join(",")})`;
}
export function inside(rect,x,y){return x>=rect.x&&y>=rect.y&&x<rect.x+rect.w&&y<rect.y+rect.h;}
export function zoomAt(view,factor,x,y,width,height){
  const scale=Math.max(1,Math.min(12,view.scale*factor)),ratio=scale/view.scale;
  return clampView({scale,x:x-(x-view.x)*ratio,y:y-(y-view.y)*ratio},width,height);
}
export function clampView(view,w,h){return {...view,x:Math.max(w*(1-view.scale),Math.min(0,view.x)),y:Math.max(h*(1-view.scale),Math.min(0,view.y))};}
const MARKETS=[["CN","沪深京 A 股"],["CN.SH","上证 A 股"],["CN.SZ","深证 A 股"],["CN.BJ","北证 A 股"],["STAR","科创板"],["CHINEXT","创业板"],["HK","港股"],["US","美股"]];
const INDEX_NAMES=["上证指数","深证成指","创业板指","科创50","沪深300"];

export class MarketMap {
  constructor({openStock,isActive,onError}){
    this.openStock=openStock;this.isActive=isActive;this.onError=onError;
    this.syncJob=null;this.syncPending=false;this.autoSyncAttempted=false;this.syncError=null;
    $("map-controls").hidden=true;$("map-inspector").hidden=true;
    const toggle=(id,button,open)=>{$(id).hidden=!open;$(button).setAttribute("aria-expanded",String(open));};
    $("map-toggle-filters").onclick=()=>toggle("map-controls","map-toggle-filters",$("map-controls").hidden);
    $("map-filters-close").onclick=()=>toggle("map-controls","map-toggle-filters",false);
    $("map-toggle-details").onclick=()=>toggle("map-inspector","map-toggle-details",$("map-inspector").hidden);
    $("map-details-close").onclick=()=>toggle("map-inspector","map-toggle-details",false);
    this.canvas=$("market-canvas");this.ctx=this.canvas.getContext("2d");
    this.query={board:"CN",metric:"auto",industry:"",aspect:1.6,watch_only:false};
    this.view={scale:1,x:0,y:0};this.data=null;this.hits=[];this.headers=[];this.selected=null;this.hover=null;this.generation=0;this.controller=null;this.search="";this.highlightWatch=false;this.frame=0;
    $("map-markets").innerHTML=MARKETS.map(([id,label])=>`<button type="button" data-map-board="${id}" aria-pressed="${id==="CN"}">${label}</button>`).join("");
    $("map-markets").onclick=event=>{const button=event.target.closest("[data-map-board]");if(button)this.change({board:button.dataset.mapBoard,industry:""});};
    $("map-metric").onchange=()=>this.change({metric:$("map-metric").value});
    $("map-watch-only").onchange=()=>this.change({watch_only:$("map-watch-only").checked});
    $("map-highlight-watch").onchange=()=>{this.highlightWatch=$("map-highlight-watch").checked;this.draw();};
    $("map-search").oninput=()=>{this.search=$("map-search").value.trim().toLowerCase();this.renderSearch();this.draw();};
    $("map-search-results").onclick=event=>{const button=event.target.closest("[data-map-key]");if(button)this.select(button.dataset.mapKey,true);};
    $("map-reset").onclick=()=>{this.view={scale:1,x:0,y:0};this.draw();};
    $("map-in").onclick=()=>this.zoom(1.5);$("map-out").onclick=()=>this.zoom(1/1.5);
    $("map-home").onclick=()=>this.change({industry:""});
    $("map-fullscreen").onclick=()=>this.fullscreen();
    $("map-retry").onclick=()=>this.refresh();
    $("map-open-stock").onclick=()=>{const tile=this.find(this.selected);if(tile)this.openQuote(tile.quote);};
    $("map-open-industry").onclick=()=>{const tile=this.find(this.selected);if(tile)this.change({industry:tile.industry});};
    this.canvas.addEventListener("wheel",event=>{event.preventDefault();const rect=this.canvas.getBoundingClientRect();this.view=zoomAt(this.view,event.deltaY<0?1.15:1/1.15,event.clientX-rect.left,event.clientY-rect.top,rect.width,rect.height);this.draw();},{passive:false});
    this.canvas.addEventListener("pointerdown",event=>{this.drag={x:event.clientX,y:event.clientY,v:{...this.view},moved:false};this.canvas.setPointerCapture(event.pointerId);});
    this.canvas.addEventListener("pointermove",event=>{
      const rect=this.canvas.getBoundingClientRect(),x=event.clientX-rect.left,y=event.clientY-rect.top;
      if(this.drag){const dx=event.clientX-this.drag.x,dy=event.clientY-this.drag.y;if(Math.hypot(dx,dy)>4)this.drag.moved=true;if(this.drag.moved){this.view=clampView({...this.drag.v,x:this.drag.v.x+dx,y:this.drag.v.y+dy},rect.width,rect.height);this.hover=null;this.draw();return;}}
      const hit=this.hits.find(t=>inside(t.rect,x,y));
      if(this.hover!==hit?.tile.key){this.hover=hit?.tile.key||null;this.draw();}
      const tip=$("map-tooltip");tip.hidden=!hit;if(hit){tip.innerHTML=`<strong>${esc(hit.tile.quote.name||hit.tile.quote.symbol)}</strong><span>${esc(hit.tile.quote.symbol)} · ${esc(hit.tile.industry)}</span><b class="${direction(hit.tile.quote.change_pct)}">${pct(hit.tile.quote.change_pct)}</b><span>${this.data.metric_label} ${this.weight(hit.tile)}</span>`;tip.style.left=Math.max(8,Math.min(rect.width-220,x+16))+"px";tip.style.top=Math.max(8,Math.min(rect.height-130,y+16))+"px";}
    });
    this.canvas.addEventListener("pointerup",event=>{const drag=this.drag;this.drag=null;this.canvas.releasePointerCapture(event.pointerId);if(drag?.moved)return;const rect=this.canvas.getBoundingClientRect(),x=event.clientX-rect.left,y=event.clientY-rect.top;const tile=this.hits.find(t=>inside(t.rect,x,y));if(tile)this.select(tile.tile.key);else{const header=this.headers.find(h=>inside(h.rect,x,y));if(header)this.change({industry:header.name});}});
    this.canvas.addEventListener("pointercancel",()=>{this.drag=null;});
    this.canvas.addEventListener("pointerleave",()=>{this.hover=null;$("map-tooltip").hidden=true;this.draw();});
    this.canvas.addEventListener("dblclick",event=>{const rect=this.canvas.getBoundingClientRect();const hit=this.hits.find(t=>inside(t.rect,event.clientX-rect.left,event.clientY-rect.top));if(hit)this.openQuote(hit.tile.quote);});
    this.canvas.addEventListener("keydown",event=>{
      const tiles=this.tiles();if(!tiles.length)return;
      if(["ArrowLeft","ArrowRight","ArrowUp","ArrowDown"].includes(event.key)){event.preventDefault();const i=tiles.findIndex(t=>t.key===this.selected),delta=["ArrowLeft","ArrowUp"].includes(event.key)?-1:1;this.select(tiles[(i+delta+tiles.length)%tiles.length].key,true);}
      if(event.key==="Enter"){event.preventDefault();const tile=this.find(this.selected);if(tile)this.openQuote(tile.quote);}
      if(event.key==="Escape"){this.view={scale:1,x:0,y:0};this.draw();}
      if(["+","="].includes(event.key)){event.preventDefault();this.zoom(1.5);}if(event.key==="-"){event.preventDefault();this.zoom(1/1.5);}
    });
    this.observer=new ResizeObserver(()=>{if(!this.isActive())return;this.draw();clearTimeout(this.resizeTimer);this.resizeTimer=setTimeout(()=>{const r=this.canvas.getBoundingClientRect();if(r.height>0&&Math.abs(this.query.aspect-r.width/r.height)>.12){this.query.aspect=r.width/r.height;this.refresh();}},250);});this.observer.observe(this.canvas);
  }
  openQuote(quote){$("map-inspector").hidden=true;$("map-toggle-details").setAttribute("aria-expanded","false");this.openStock(quote);}
  tiles(){return (this.data?.groups||[]).flatMap(g=>g.tiles);}
  find(key){return this.tiles().find(t=>t.key===key);}
  deactivate(){this.generation++;this.controller?.abort();clearTimeout(this.resizeTimer);$("map-tooltip").hidden=true;}
  async change(changes){this.query={...this.query,...changes};this.view={scale:1,x:0,y:0};this.selected=null;this.data=null;this.renderInspector();this.draw();$("market-indices").hidden=["US","HK"].includes(this.query.board);return this.refresh();}
  async refresh(){
    if(!this.isActive())return;
    const generation=++this.generation;this.controller?.abort();this.controller=new AbortController();
    const rect=this.canvas.getBoundingClientRect();if(rect.height>0)this.query.aspect=rect.width/rect.height;
    $("map-state").textContent="读取同日截面…";$("map-error").hidden=true;this.canvas.setAttribute("aria-busy","true");
    try{
      const data=await api("/v2/market-map?"+new URLSearchParams(this.query),{signal:this.controller.signal});
      if(generation!==this.generation||!this.isActive())return;
      this.data=data;this.hover=null;$("map-tooltip").hidden=true;if(!this.find(this.selected))this.selected=null;
      this.renderMeta();this.renderIndices();this.renderSearch();this.renderInspector();this.draw();
      await this.updateSync();
      if(generation!==this.generation||!this.isActive())return;
      if(data.coverage.universe===0&&!this.autoSyncAttempted&&!["HK","US"].includes(this.query.board)){this.autoSyncAttempted=true;const settings=await api("/v1/settings");if(this.isActive()&&settings.provider?.configured)await this.sync();}
    }catch(error){if(error.name==="AbortError")return;if(generation!==this.generation||!this.isActive())return;$("map-error").hidden=false;$("map-error-text").textContent="星图读取失败："+error.message;$("map-state").textContent=this.data?"读取失败 · 保留上次截面":"读取失败";}
    finally{if(generation===this.generation)this.canvas.removeAttribute("aria-busy");}
  }
  async sync(){
    if(["HK","US"].includes(this.query.board))return this.refresh();
    if(this.syncPending||["queued","running","cancelling"].includes(this.syncJob?.status))return this.syncJob;
    this.autoSyncAttempted=true;this.syncPending=true;this.syncJob=null;this.syncError=null;this.renderSyncStatus();
    try{
      const settings=await api("/v1/settings");
      if(!settings.provider?.configured){throw new Error("请先在设置中配置 Tushare 数据源，再刷新行情。");}
      this.syncJob=await invoke("job.submit",{kind:"market.sync",params:{}});
      if(this.isActive())await this.refresh();
      return this.syncJob;
    }catch(error){this.syncError=error.message;this.onError?.(error.message);}
    finally{this.syncPending=false;this.renderSyncStatus();}
  }
  async updateSync(){
    if(this.syncJob&&["queued","running","cancelling"].includes(this.syncJob.status)){
      try{const job=await invoke("job.get",{id:this.syncJob.id});
      if(!this.isActive())return;this.syncJob=job;this.syncError=null;}catch(error){this.syncError="同步任务状态读取失败："+error.message;}
    }
    this.renderSyncStatus();
  }
  renderSyncStatus(){
    if(!this.isActive())return;
    const node=$("map-sync-status"),job=this.syncJob;
    const active=this.syncPending||["queued","running","cancelling"].includes(job?.status);
    $("pulse-refresh").disabled=active;
    if(!job&&!this.syncPending&&!this.syncError){node.hidden=true;return;}
    node.hidden=false;node.className=this.syncError||job&&["failed","partial","interrupted"].includes(job.status)?"error-text":"microcopy";
    node.textContent=this.syncError|| (job?["queued","running","cancelling"].includes(job.status)?`同步中 · ${job.phase} · ${job.progress||0}%`:`${statusZh(job.status)} · ${job.error||job.result?.message||"行情已更新"}${job.result?.warnings?.length?"；"+job.result.warnings.join("；"):""}`:"正在提交行情同步…");
  }
  renderMeta(){
    const d=this.data,c=d.coverage;
    $("map-state").textContent=`${d.historical?"历史截面":"同日截面"} · ${d.trade_date||"无有效报价"}`;
    $("map-state").className=d.historical?"map-state historical":"map-state";
    $("map-scope").textContent=`已导入 ${c.universe.toLocaleString()} · 同日有效 ${c.quoted.toLocaleString()} · 展示 ${c.displayed.toLocaleString()}`;
    $("map-metric-label").textContent=`面积 · ${d.metric_label}${d.metric==="equal"?"":" / "+d.currency}`;
    $("map-date").textContent=d.trade_date||"待更新";$("map-date").title="报价来源交易日，非页面刷新时间";
    $("map-breadth").innerHTML=`<span class="up">上涨 <b>${c.up}</b></span><span class="down">下跌 <b>${c.down}</b></span><span>平盘 <b>${c.flat}</b></span><span>未知 <b>${c.unknown_change}</b></span>`;
    const total=c.displayed||1;$("map-breadth-bar").innerHTML=[['up',c.up],['flat',c.flat],['unknown',c.unknown_change],['down',c.down]].map(([type,n])=>`<i class="${type}" style="width:${n/total*100}%"></i>`).join("");
    $("map-quality-list").innerHTML=[["无报价",c.missing_quote],["其他日期",c.other_date],["无效 / 币种不符",c.invalid],["缺少面积权重",c.missing_weight],["盘中延迟",c.delayed],["未分类",c.unclassified]].map(([name,n])=>`<div><span>${name}</span><b>${n}</b></div>`).join("");
    $("map-notes").innerHTML=d.notes.map(n=>`<p>${esc(n)}</p>`).join("");
    $("map-home").disabled=!this.query.industry;$("map-breadcrumb").textContent=this.query.industry||"市场全景";
    $("map-markets").querySelectorAll("button").forEach(b=>b.setAttribute("aria-pressed",b.dataset.mapBoard===this.query.board));
    $("map-empty").hidden=c.displayed>0;$("map-empty-title").textContent=c.missing_weight>0?"当前指标缺少同日权重":"当前范围暂无有效报价";
    $("map-empty-text").textContent=c.missing_weight>0?"切换「等面积」查看报价，或在数据中心同步同日流通市值。":"请先同步行情或导入数据；旧日期、未来日期与异常记录不会混入同日截面。";
    $("map-updated").textContent=`计算 ${time(d.computed_at)} · 数据版本 ${d.data_version}`;
    this.renderLegend();
  }
  renderLegend(){const green=document.documentElement.dataset.colors==="green_up";$("map-legend").innerHTML=[-5,-3,-1,0,1,3,5].map(n=>`<span style="background:${tileColor(n,green)}">${n>0?"+":""}${n}%${n===-5?"以下":n===5?"以上":""}</span>`).join("")+`<span style="background:${tileColor(null)}">缺失</span>`;}
  renderSearch(){const matches=this.search?this.tiles().filter(t=>this.matches(t)):[];$("map-search-results").innerHTML=matches.slice(0,12).map(t=>`<button data-map-key="${esc(t.key)}"><span>${esc(t.quote.name||t.quote.symbol)}<small>${esc(t.quote.symbol)}</small></span><b class="${direction(t.quote.change_pct)}">${pct(t.quote.change_pct)}</b></button>`).join("");$("map-search-count").textContent=this.search?`${matches.length} 个匹配，显示前 12 个`:"搜索将高亮匹配股票和行业";}
  matches(t){return `${t.quote.symbol} ${t.quote.name||""} ${t.industry}`.toLowerCase().includes(this.search);}
  weight(tile){return this.data.metric==="equal"?"1 份":amount(tile.weight,this.data.currency);}
  select(key,center=false){this.selected=key;$("map-inspector").hidden=false;$("map-toggle-details").setAttribute("aria-expanded","true");if(center){this.view={scale:1,x:0,y:0};this.draw();const hit=this.hits.find(h=>h.tile.key===key),r=this.canvas.getBoundingClientRect();if(hit){const p=hit.rect,scale=Math.max(1,Math.min(12,Math.min(r.width/Math.max(p.w,1),r.height/Math.max(p.h,1))*.35));this.view=clampView({scale,x:r.width/2-(p.x+p.w/2)*scale,y:r.height/2-(p.y+p.h/2)*scale},r.width,r.height);}}this.renderInspector();this.draw();}
  renderInspector(){
    const tile=this.find(this.selected);$("map-selection-empty").hidden=!!tile;$("map-selection").hidden=!tile;if(!tile)return;
    const q=tile.quote;$("map-selected-name").textContent=q.name||q.symbol;$("map-selected-code").textContent=`${q.symbol} · ${q.market} ${tile.watched?"· 自选":""}`;
    $("map-selected-last").textContent=fmt(q.last);$("map-selected-pct").textContent=pct(q.change_pct);$("map-selected-pct").className=direction(q.change_pct);
    $("map-selected-fields").innerHTML=[["行业",tile.industry],[this.data.metric_label,this.weight(tile)],["成交额",amount(q.turnover,q.currency)],["交易日",q.trade_date||"—"],["源时间",q.source_kind==="daily"?`${q.trade_date} · 日线`:time(q.ts,q.market)],["接收时间",time(q.received_at,q.market)],["报价来源",q.source||"未标明"],["权重日期",tile.weight_as_of||"不适用"],["权重来源",tile.weight_source||"不适用"]].map(([k,v])=>`<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join("");
    $("map-selection-live").textContent=`已选 ${q.name||q.symbol}，${pct(q.change_pct)}，${tile.industry}`;
  }
  zoom(factor){const r=this.canvas.getBoundingClientRect();this.view=zoomAt(this.view,factor,r.width/2,r.height/2,r.width,r.height);this.draw();}
  async fullscreen(){try{if(document.fullscreenElement)await document.exitFullscreen();else await $("map-workspace").requestFullscreen();this.draw();}catch{this.onError?.("当前浏览器不支持全屏，请使用窗口最大化。")}}
  setPulse(){this.renderIndices();}
  renderIndices(){
    const indices=this.data?.indices||[];
    $("market-indices").innerHTML=INDEX_NAMES.map((label,i)=>{const item=indices[i],value=item?.quote;return `<div class="market-index"><span>${esc(item?.name||label)}</span><strong>${fmt(value?.last)}</strong><small class="${direction(value?.change_pct)}">${pct(value?.change_pct)}</small><em>${value?esc(value.trade_date||"未知日期")+" · "+(value.source_kind==="daily"?"日线":"已保存报价"):"未同步指数日线"}</em></div>`;}).join("");
    $("market-indices").hidden=["US","HK"].includes(this.query.board);
  }
  draw(){
    const rect=this.canvas.getBoundingClientRect(),w=rect.width,h=rect.height;if(!w||!h)return;
    this.view=clampView(this.view,w,h);
    const ratio=Math.min(2,window.devicePixelRatio||1);if(this.canvas.width!==Math.round(w*ratio)||this.canvas.height!==Math.round(h*ratio)){this.canvas.width=Math.round(w*ratio);this.canvas.height=Math.round(h*ratio);}
    const ctx=this.ctx;ctx.setTransform(ratio,0,0,ratio,0,0);ctx.fillStyle="#0e1521";ctx.fillRect(0,0,w,h);this.hits=[];this.headers=[];
    if(!this.data)return;
    const green=document.documentElement.dataset.colors==="green_up",s=this.view.scale;ctx.textBaseline="middle";
    for(const group of this.data.groups){
      const b=group.bounds,g={x:this.view.x+b.x*w*s,y:this.view.y+b.y*h*s,w:b.w*w*s,h:b.h*h*s};
      if(g.x>w||g.y>h||g.x+g.w<0||g.y+g.h<0)continue;
      const header=g.w>55&&g.h>45?23:0;
      if(header){ctx.fillStyle="#1d2838";ctx.fillRect(g.x+1,g.y+1,g.w-2,header);ctx.save();ctx.beginPath();ctx.rect(g.x+5,g.y,g.w-10,header);ctx.clip();ctx.fillStyle="#dbe5f5";ctx.font="600 11px 'Microsoft YaHei', sans-serif";ctx.textAlign="left";ctx.fillText(group.name,g.x+7,g.y+header/2);if(g.w>155){ctx.textAlign="right";ctx.fillStyle=group.mean_pct==null?"#9aa8be":group.mean_pct>=0?(green?"#55d6ac":"#ff8c98"):(green?"#ff8c98":"#55d6ac");ctx.fillText(pct(group.mean_pct),g.x+g.w-7,g.y+header/2);}ctx.restore();this.headers.push({name:group.name,rect:{...g,h:header}});}
      const body={x:g.x+1,y:g.y+header+1,w:Math.max(0,g.w-2),h:Math.max(0,g.h-header-2)};
      for(const tile of group.tiles){
        const p=tile.bounds,r={x:body.x+p.x*body.w,y:body.y+p.y*body.h,w:p.w*body.w,h:p.h*body.h};
        if(r.w<.3||r.h<.3||r.x>w||r.y>h||r.x+r.w<0||r.y+r.h<0)continue;
        this.hits.push({tile,rect:r});const highlighted=this.search?this.matches(tile):this.highlightWatch?tile.watched:true;
        ctx.globalAlpha=highlighted?1:.22;ctx.fillStyle=tileColor(tile.quote.change_pct,green);ctx.fillRect(r.x+.5,r.y+.5,Math.max(0,r.w-1),Math.max(0,r.h-1));ctx.globalAlpha=1;
        const selected=tile.key===this.selected,hover=tile.key===this.hover;
        if(selected||hover||((this.search||this.highlightWatch)&&highlighted)){ctx.strokeStyle=selected?"#ffd782":hover?"#fff":"#90b9ff";ctx.lineWidth=selected?2.5:1.5;ctx.strokeRect(r.x+1,r.y+1,Math.max(0,r.w-2),Math.max(0,r.h-2));}
        if(r.w<35||r.h<22)continue;
        ctx.save();ctx.beginPath();ctx.rect(r.x+4,r.y+2,Math.max(0,r.w-8),Math.max(0,r.h-4));ctx.clip();ctx.textAlign="center";ctx.fillStyle="#fff";ctx.globalAlpha=highlighted?1:.4;
        const size=Math.max(10,Math.min(19,r.w/6,r.h/3.8));ctx.font=`600 ${size}px 'Microsoft YaHei', sans-serif`;
        const name=tile.quote.name||tile.quote.symbol;const lines=r.h>=42;
        ctx.fillText(name,r.x+r.w/2,r.y+r.h/2-(lines?size*.65:0));
        if(lines){ctx.font=`500 ${Math.max(10,size-1)}px 'Segoe UI', sans-serif`;ctx.fillText(pct(tile.quote.change_pct),r.x+r.w/2,r.y+r.h/2+size*.7);}
        if(tile.watched&&r.w>70&&r.h>65){ctx.fillStyle="#ffe2a8";ctx.textAlign="left";ctx.font="11px sans-serif";ctx.fillText("★",r.x+6,r.y+10);}
        ctx.restore();
      }
    }
    $("map-zoom").textContent=`${Math.round(s*100)}%`;this.renderLegend();
  }
}
