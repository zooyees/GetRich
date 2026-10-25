import {$,esc,fmt,signed,pct,amount,direction,time,api,invoke,toast,busy,Reads,Events,applyPreferences} from "/runtime.js";
import {Chart} from "/chart.js";
import {Management,BOARDS,COLUMNS} from "/management.js";
import {renderPulse} from "/pulse.js";
import {MarketMap} from "/market-map.js";
import {StockResearch} from "/stock-research.js";

const state={view:"quotes",board:"ALL",query:"",sort:"symbol",industry:"",change_filter:"",offset:0,rows:[],matched:0,selectedKey:null,detail:null,bars:[],barMeta:{},tf:"1d",stats:null,preferences:{},snapshot:null,loading:false};
const reads=new Reads();const chart=new Chart($("kline"),$("kline-meta"));
let routeGeneration=0,detailGeneration=0,eventTimer=0,searchTimer=0;
const key=row=>row?row.market+":"+row.symbol:null;
const isCurrent=generation=>generation===routeGeneration&&state.view==="quotes";
const guard=promise=>promise.catch(error=>{if(error.name!=="AbortError")toast(error.message,true);});
const marketMap=new MarketMap({openStock:row=>{location.hash=stockHash(row);},isActive:()=>state.view==="pulse",onError:message=>toast(message,true)});
const management=new Management({
  isView:view=>state.view===view,
  openRecent:(market,symbol)=>{location.hash=stockHash({market,symbol});},
  settings:data=>{state.preferences=data.preferences||{};applyPreferences(state.preferences);applyColumns();chart.draw();marketMap.draw();},
  stats:updateStats,
  jobFinished:job=>{
    if(!["succeeded","partial","failed"].includes(job.status))return;
    toast(`任务 #${job.id} ${job.status==="succeeded"?"已完成":job.status==="partial"?"部分完成":"失败"}`,job.status==="failed");
    if(state.view==="quotes")guard(loadBoard(routeGeneration,true));
    if(state.view==="pulse"){guard(loadPulse(routeGeneration));guard(marketMap.refresh());}
    guard(api("/v1/stats").then(updateStats));
  },
});
const stockResearch=new StockResearch({current:()=>state.detail?.quote,configured:()=>!!management.settings?.provider?.configured,isActive:()=>state.view==="quotes",reload:()=>state.detail?loadStock(state.detail.quote,routeGeneration,true):Promise.resolve()});
function quoteHash(changes={}){
  const values={board:state.board,q:state.query,sort:state.sort,industry:state.industry,change_filter:state.change_filter,offset:state.offset,...changes};
  const query=new URLSearchParams();for(const [name,value] of Object.entries(values))if(value!==""&&value!=null)query.set(name,String(value));
  return "#/quotes?"+query;
}
function navigate(changes={}){location.hash=quoteHash({offset:0,...changes});}
function stockHash(row){return `#/stock/${encodeURIComponent(row.market)}/${encodeURIComponent(row.symbol)}?${quoteHash().split("?")[1]}`;}
function updateStats(stats){
  state.stats=stats;
  $("status-pill").textContent=stats.empty?"本地库为空":`${stats.listings} 只标的 · 库内最新 ${time(stats.last_quote_ts)}`;
}
function applyColumns(){
  const visible=new Set(state.preferences.columns||COLUMNS.map(c=>c[0]));visible.add("symbol");visible.add("name");
  const order=state.preferences.columns||COLUMNS.map(c=>c[0]);
  const header=$("quote-head").querySelector("tr")||$("quote-head");
  const cells=[...$("quote-head").querySelectorAll("th")];for(const id of order){const cell=cells.find(c=>c.querySelector("button").dataset.sort===id);if(cell)header.appendChild(cell);}
  for(const row of $("quote-body").rows){for(const id of order){const cell=[...row.cells].find(c=>c.dataset.col===id);if(cell)row.appendChild(cell);}}
  $("quote-head").querySelectorAll("th").forEach(th=>{const col=th.querySelector("button").dataset.sort;th.hidden=!visible.has(col);});
  $("quote-body").querySelectorAll("[data-col]").forEach(cell=>cell.hidden=!visible.has(cell.dataset.col));
}
function renderBoards(){
  $("boards").innerHTML=BOARDS.map(([id,label])=>`<button type="button" data-board="${id}" aria-pressed="${state.board===id}" class="${state.board===id?"on":""}">${label}</button>`).join("");
  $("change-filter").value=state.change_filter;
  $("industry-filter").textContent=state.industry?"行业："+state.industry:"";
}
function showPage(view){
  if(state.view==="pulse"&&view!=="pulse")marketMap.deactivate();
  state.view=view;$("workspace").hidden=view!=="quotes";
  for(const page of ["pulse","data","settings"])$("view-"+page).hidden=view!==page;
  document.querySelectorAll(".nav a").forEach(a=>{const selected=a.dataset.route===(view==="quotes"?"quotes":view);a.classList.toggle("active",selected);if(selected)a.setAttribute("aria-current","page");else a.removeAttribute("aria-current");});
  $("search-wrap").hidden=view!=="quotes";
}
async function route(){
  const generation=++routeGeneration;reads.cancel();clearTimeout(eventTimer);detailGeneration++;
  let path,params;try{const hash=location.hash||"#/quotes";const index=hash.indexOf("?");path=index<0?hash:hash.slice(0,index);params=new URLSearchParams(index<0?"":hash.slice(index+1));}catch{location.hash="#/quotes";return;}
  clearTimeout(searchTimer);
  if(path==="#/pulse"){showPage("pulse");await Promise.all([loadPulse(generation),marketMap.refresh()]);return;}
  if(path==="#/data"){showPage("data");await management.loadData();return;}
  if(path==="#/settings"){showPage("settings");await management.loadSettings();return;}
  const stock=path.match(/^#\/stock\/([^/]+)\/([^/?]+)$/);
  if(params.has("board"))state.board=params.get("board");
  if(params.has("q"))state.query=params.get("q");
  if(params.has("sort"))state.sort=params.get("sort");
  state.industry=params.get("industry")||"";state.change_filter=params.get("change_filter")||"";state.offset=Math.max(0,Number(params.get("offset"))||0);
  $("quote-search").value=state.query;showPage("quotes");renderBoards();
  $("workspace").classList.toggle("detail-open",!!stock);
  const wanted=stock?{market:decodeURIComponent(stock[1]),symbol:decodeURIComponent(stock[2])}:null;
  await loadBoard(generation,false,wanted);
}
async function loadBoard(generation,passive=false,wanted=null){
  if(!isCurrent(generation))return;
  state.loading=true;$("quote-body").setAttribute("aria-busy","true");
  if(!passive)$("quote-flash").hidden=true;
  const query=new URLSearchParams({board:state.board,q:state.query,sort:state.sort,industry:state.industry,change_filter:state.change_filter,offset:String(state.offset),limit:"200"});
  try{
    const page=await reads.get("board","/v1/quotes?"+query);
    if(!isCurrent(generation))return;
    state.rows=page.rows||[];state.matched=page.matched;updateStats(page.stats);renderList();
    const explicit=location.hash.match(/^#\/stock\/([^/]+)\/([^/?]+)/);
    let row=wanted||(explicit?{market:decodeURIComponent(explicit[1]),symbol:decodeURIComponent(explicit[2])}:null);
    if(!row)row=state.rows.find(r=>key(r)===state.selectedKey)||state.rows[0];
    if(!row){
      state.selectedKey=null;state.detail=null;state.bars=[];$("detail-content").hidden=true;$("detail-flash").hidden=false;$("detail-flash").textContent="当前筛选无匹配股票";
      $("quote-flash").hidden=false;$("quote-flash").innerHTML=`<h2>${page.empty_kind==="no_watch"?"暂无自选":"暂无匹配数据"}</h2><p>${esc(page.empty_message)}</p>${page.stats.empty?'<a class="button primary" href="#/data">导入数据</a>':""}`;
      return;
    }
    state.selectedKey=key(row);renderSelection();
    await loadStock(row,generation,passive);
  }catch(error){if(error.name==="AbortError")return;$("quote-flash").hidden=false;$("quote-flash").textContent="行情读取失败："+error.message;throw error;}
  finally{if(isCurrent(generation)){state.loading=false;$("quote-body").removeAttribute("aria-busy");}}
}
function renderList(){
  $("quote-body").innerHTML=state.rows.map(row=>`<tr data-key="${esc(key(row))}" tabindex="0" aria-selected="${key(row)===state.selectedKey}" class="${key(row)===state.selectedKey?"selected":""}"><td data-col="symbol" class="symbol">${esc(row.symbol)}<small>${esc(row.market)}</small></td><td data-col="name" class="name">${esc(row.name||row.symbol)}</td><td data-col="last" class="num">${fmt(row.last)}</td><td data-col="change_pct" class="num ${direction(row.change_pct)}">${pct(row.change_pct)}</td><td data-col="change_amt" class="num ${direction(row.change)}">${signed(row.change)}</td><td data-col="turnover" class="num">${amount(row.turnover,row.currency)}</td><td data-col="amplitude" class="num">${row.amplitude==null?"—":fmt(row.amplitude)+"%"}</td></tr>`).join("");
  $("quote-foot").textContent=state.matched?`${state.offset+1}–${state.offset+state.rows.length} / 筛选 ${state.matched} 只`:"0 只";
  $("page-prev").disabled=state.offset===0;$("page-next").disabled=state.offset+state.rows.length>=state.matched;
  $("quote-head").querySelectorAll("button").forEach(button=>{const base=state.sort.replace(/_(asc|desc)$/,"");const selected=base===button.dataset.sort;button.classList.toggle("on",selected);button.parentElement.setAttribute("aria-sort",selected?(state.sort.endsWith("_asc")||["symbol","name"].includes(state.sort)?"ascending":"descending"):"none");});
  applyColumns();
}
function renderSelection(){for(const row of $("quote-body").rows){const selected=row.dataset.key===state.selectedKey;row.classList.toggle("selected",selected);row.setAttribute("aria-selected",String(selected));}}
async function loadStock(row,generation,passive=false){
  const request=++detailGeneration;const selected=key(row);state.selectedKey=selected;
  if(!passive){$("detail-content").hidden=true;$("detail-flash").hidden=false;$("detail-flash").textContent="正在载入详情…";}
  try{
    const prefix=`/v1/stocks/${encodeURIComponent(row.market)}/${encodeURIComponent(row.symbol)}`;
    const [detail,bars]=await Promise.all([reads.get("detail",prefix),reads.get("bars",prefix+`/bars?timeframe=${state.tf}&limit=20000`)]);
    if(!isCurrent(generation)||request!==detailGeneration||state.selectedKey!==selected)return;
    const changed=key(state.detail?.quote)!==key(detail.quote);
    state.detail=detail;state.barMeta=bars;state.bars=bars.rows||[];renderDetail(!passive,bars.empty_message);
    guard(stockResearch.update());
    if(changed&&!passive)guard(invoke("history.view",{symbol:row.symbol,market:row.market}));
  }catch(error){if(error.name==="AbortError")return;if(request!==detailGeneration)return;$("detail-flash").hidden=false;$("detail-flash").textContent="详情读取失败："+error.message;throw error;}
}
function gridItem(name,value){return `<div><dt>${esc(name)}</dt><dd>${esc(value)}</dd></div>`;}
function renderDetail(reset=false,message=""){
  const detail=state.detail;if(!detail)return;const q=detail.quote;
  $("detail-flash").hidden=true;$("detail-content").hidden=false;$("detail-name").textContent=q.name||q.symbol;
  const outside=!state.rows.some(row=>key(row)===key(q));
  $("detail-code").textContent=`${q.symbol} · ${q.board_label||q.market} · ${q.currency||""}${outside?" · 当前列表范围外":""}`;
  $("detail-time").textContent=`行情时间 ${q.source_kind==="daily"?(q.trade_date||q.ts?.slice(0,10)||"—")+" · 日线日期精度":time(q.ts,q.market)}${q.data_status?" · "+q.data_status:""}`;
  $("detail-last").className=direction(q.change_pct);$("detail-last").textContent=detail.empty.no_quote?"暂无报价":fmt(q.last);$("detail-chg").className=direction(q.change_pct);$("detail-chg").textContent=detail.empty.no_quote?"":`${signed(q.change)} / ${pct(q.change_pct)}`;
  $("watch-btn").textContent=detail.watched?"★ 已自选":"☆ 加入自选";$("watch-btn").setAttribute("aria-pressed",String(detail.watched));$("watch-btn").setAttribute("aria-label",detail.watched?"移除自选":"加入自选");
  $("quote-strip").innerHTML=[["今开",fmt(q.open)],["最高",fmt(q.high)],["最低",fmt(q.low)],["昨收",fmt(q.prev_close)],["成交量（股）",amount(q.volume)],["成交额",amount(q.turnover,q.currency)],["振幅",q.amplitude==null?"—":fmt(q.amplitude)+"%"],["行业",detail.listing?.industry||"未分类"]].map(([label,value])=>gridItem(label,value)).join("");
  const listing=detail.listing||{};$("profile-dl").innerHTML=[["行业",listing.industry||listing.sector||"—"],["交易所",listing.exchange||"—"],["上市日",listing.listing_date||"—"],["状态",listing.status||"—"],["接收时间",time(q.received_at,q.market)],["报价类型",q.source_kind||"—"],...(detail.metadata||[]).map(m=>[m.key,m.value])].map(([a,b])=>gridItem(a,b)).join("");
  $("tf-tabs").querySelectorAll("button").forEach(button=>{const selected=stockResearch.intraday?button.dataset.tf==="intraday":button.dataset.tf===state.tf;button.setAttribute("aria-selected",String(selected));button.classList.toggle("active",selected);});
  $("period-note").textContent=state.barMeta.partial_last_period?"最新周/月周期仍在进行，以已导入日线为准":"";
  chart.setData(state.bars,{reset,message:message||"暂无该周期 K 线"});
  $("bars-table").innerHTML=state.bars.slice(-200).map(b=>`<tr><td>${esc(b.ts)}</td><td>${fmt(b.open)}</td><td>${fmt(b.high)}</td><td>${fmt(b.low)}</td><td>${fmt(b.close)}</td></tr>`).join("");
  $("pull-history").hidden=!String(q.market).startsWith("CN.")||!management.settings?.provider?.configured;
  $("detail-back").href=quoteHash();
  stockResearch.render();
}
async function loadPulse(generation){
  if(state.view!=="pulse"||generation!==routeGeneration)return;
  try{
    const snapshot=await reads.get("pulse","/v2/pulse");
    if(state.view!=="pulse"||generation!==routeGeneration)return;
    state.snapshot=snapshot;renderPulse(snapshot,navigate);marketMap.setPulse(snapshot);
  }catch(error){if(error.name==="AbortError")return;$("pulse-quality").className="quality-banner bad";$("pulse-quality").textContent=`市场截面读取失败：${error.message}${state.snapshot?" · 下方保留上次成功快照，未代表当前状态":""}`;if(!state.snapshot)$("pulse-body").hidden=true;}
}
function scheduleRefresh(){
  clearTimeout(eventTimer);const generation=routeGeneration;
  eventTimer=setTimeout(()=>{if(generation!==routeGeneration)return;if(state.view==="quotes")guard(loadBoard(generation,true));if(state.view==="pulse"){guard(loadPulse(generation));guard(marketMap.refresh());}if(state.view==="data")guard(management.loadData());},400);
}
const events=new Events(async event=>{
  if(["job.updated"].includes(event.type)){await management.loadJobs();return;}
  if(event.type==="settings.changed"){
    const data=await api("/v1/settings");management.settings=data;state.preferences=data.preferences||{};applyPreferences(state.preferences);applyColumns();chart.draw();marketMap.draw();if(state.view==="settings")management.renderWatch(data.watchlist||[]);scheduleRefresh();return;
  }
  if(["quote.updated","ingest.done","snapshot.available","resync_required"].includes(event.type))scheduleRefresh();
});
function bind(){
  $("boards").onclick=event=>{const button=event.target.closest("[data-board]");if(button)navigate({board:button.dataset.board,industry:""});};
  $("quote-search").oninput=()=>{clearTimeout(searchTimer);searchTimer=setTimeout(()=>{if(state.view==="quotes")navigate({q:$("quote-search").value.trim()});},200);};
  $("change-filter").onchange=()=>navigate({change_filter:$("change-filter").value});
  $("clear-filters").onclick=()=>navigate({q:"",industry:"",change_filter:"",board:"ALL"});
  $("quote-head").onclick=event=>{const button=event.target.closest("[data-sort]");if(!button)return;let next=button.dataset.sort;const base=state.sort.replace(/_(asc|desc)$/,"");if(base===next)next=state.sort.endsWith("_asc")?next:["symbol","name"].includes(state.sort)?next+"_desc":next+"_asc";navigate({sort:next});};
  $("quote-body").onclick=event=>{const tr=event.target.closest("[data-key]");const row=state.rows.find(row=>key(row)===tr?.dataset.key);if(row)location.hash=stockHash(row);};
  $("quote-body").onkeydown=event=>{if(event.key==="Enter"){const tr=event.target.closest("[data-key]");const row=state.rows.find(row=>key(row)===tr?.dataset.key);if(row)location.hash=stockHash(row);}};
  $("page-prev").onclick=()=>navigate({offset:Math.max(0,state.offset-200)});$("page-next").onclick=()=>navigate({offset:state.offset+200});
  $("tf-tabs").onclick=event=>{const tf=event.target.closest("[data-tf]")?.dataset.tf;if(!tf||!state.detail)return;stockResearch.setIntraday(tf==="intraday");if(tf==="intraday"){renderDetail();return;}state.tf=tf;guard(loadStock(state.detail.quote,routeGeneration));};
  $("watch-btn").onclick=()=>guard(busy($("watch-btn"),async()=>{const d=state.detail;if(!d)return;const result=await invoke("watch.set",{symbol:d.quote.symbol,market:d.quote.market,name:d.quote.name,watched:!d.watched});if(state.detail&&key(state.detail.quote)===key(d.quote)){state.detail.watched=result.watched;renderDetail();}if(state.board==="WATCH")await loadBoard(routeGeneration,true);toast(result.watched?"已加入自选":"已移除自选");}));
  $("pulse-refresh").onclick=()=>guard(marketMap.sync());
  $("pull-history").onclick=()=>guard(stockResearch.ensure("history",true));
  $("chart-in").onclick=()=>chart.zoom(.8);$("chart-out").onclick=()=>chart.zoom(1.25);$("chart-left").onclick=()=>chart.move(-Math.max(1,Math.round(chart.span/2)));$("chart-right").onclick=()=>chart.move(Math.max(1,Math.round(chart.span/2)));$("chart-reset").onclick=()=>chart.reset();
  document.addEventListener("keydown",event=>{
    const typing=event.target.closest("input,textarea,select,[contenteditable=true]");
    if(!typing&&(event.key==="/"||(event.ctrlKey&&event.key==="k"))){event.preventDefault();if(state.view!=="quotes")location.hash=quoteHash();setTimeout(()=>$("quote-search").focus(),100);return;}
    if(event.key==="Escape"){if(typing){event.target.blur();return;}if(state.view==="quotes")navigate({q:""});return;}
    if(typing||state.view!=="quotes")return;
    if(["ArrowDown","j","ArrowUp","k"].includes(event.key)&&event.target!==$("kline")){
      event.preventDefault();const current=Math.max(0,state.rows.findIndex(r=>key(r)===state.selectedKey));const next=Math.max(0,Math.min(state.rows.length-1,current+(["ArrowDown","j"].includes(event.key)?1:-1)));if(state.rows[next])location.hash=stockHash(state.rows[next]);
    }
    if(["1","2","3"].includes(event.key)&&state.detail){state.tf={"1":"1d","2":"1w","3":"1mo"}[event.key];guard(loadStock(state.detail.quote,routeGeneration));}
    if(event.key.toLowerCase()==="w"&&state.detail)$("watch-btn").click();
  });
  window.addEventListener("hashchange",()=>guard(route()));
  window.addEventListener("beforeunload",()=>events.stop());
}
async function health(){
  try{const h=await api("/v1/health");$("connection").textContent="本地服务已连接";$("connection").className="status-dot connected";$("footer-status").textContent=`v${h.version} · 实例 ${h.instance_id||"—"}`;}catch(error){$("connection").textContent="连接异常";$("connection").className="status-dot offline";}
}
bind();
try{const settings=await management.initialize();state.board=settings.default_board||"ALL";}catch(error){toast("设置初始化失败："+error.message,true);}
await guard(route());events.start();guard(health());
setInterval(()=>{if(document.hidden)return;if(state.view==="pulse"){guard(loadPulse(routeGeneration));guard(marketMap.refresh());}if(state.view==="data")guard(management.loadJobs());},5000);
setInterval(()=>{if(!document.hidden)guard(health());},15000);
