import {$,esc,fmt,time,api,invoke,busy,toast,statusZh,applyPreferences} from "/runtime.js";
import {WatchSearch} from "/watch-search.js";
export const BOARDS=[["WATCH","自选"],["ALL","全部"],["CN","沪深京"],["CN.SH","沪市"],["CN.SZ","深市"],["CHINEXT","创业板"],["STAR","科创板"],["CN.BJ","北证"],["HK","港股"],["US","美股"]];
export const COLUMNS=[["symbol","代码"],["name","名称"],["last","最新"],["change_pct","涨跌幅"],["change_amt","涨跌额"],["turnover","成交额"],["amplitude","振幅"]];
export function download(name,data){const url=URL.createObjectURL(new Blob([JSON.stringify(data,null,2)],{type:"application/json"}));const link=document.createElement("a");link.href=url;link.download=name;link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);}
export class Management{
  constructor(callbacks){this.callbacks=callbacks;this.settings=null;this.upload=null;this.preview=null;this.completed=new Set();this.jobsInitialized=false;this.watchSearch=new WatchSearch(()=>this.callbacks.isView("settings"));this.bind();}
  async initialize(){this.settings=await api("/v1/settings");applyPreferences(this.settings.preferences);this.callbacks.settings(this.settings);return this.settings;}
  async loadData(){
    const outcomes=await Promise.allSettled([api("/v1/stats"),this.loadJobs(),api("/v1/runs"),api("/v1/tushare")]);
    if(!this.callbacks.isView("data"))return;
    const [stats,,runs,provider]=outcomes;
    if(stats.status==="fulfilled"){
      const s=stats.value;this.callbacks.stats(s);
      $("data-stats").innerHTML=[["标的",s.listings],["报价",s.quotes],["日线",s.daily_bars??"—"],["已存周线",s.weekly_bars??"—"],["已存月线",s.monthly_bars??"—"],["全部 K 线",s.bars]].map(([label,value])=>`<div class="metric"><span>${label}</span><strong>${fmt(value,0)}</strong></div>`).join("");
    }else $("data-stats").textContent="库状态读取失败："+stats.reason.message;
    if(runs.status==="fulfilled")$("runs-box").innerHTML=(runs.value.runs||[]).map(run=>`<div class="run-row"><strong>#${run.id} · ${esc(run.source)}</strong><span>${esc(statusZh(run.status))} · 标的 ${run.instruments_upserted} / 报价 ${run.quotes_upserted} / K ${run.bars_upserted}</span><small>${esc(time(run.finished_at||run.started_at))}</small>${run.error?`<p class="error-text">${esc(run.error)}</p>`:""}</div>`).join("")||"暂无导入记录";
    if(provider.status==="fulfilled"){const p=provider.value;$("data-provider").textContent=p.configured?`Tushare · ${p.in_session?"交易时段":"非交易时段"} · ${p.enabled?"后台刷新已启用":"后台刷新未启用"}`:"尚未配置 Tushare，先到设置完成保存与连接测试。";for(const id of ["tushare-sync","tushare-valuations","tushare-indices"])$(id).disabled=!p.configured;}
    for(const result of outcomes)if(result.status==="rejected")toast(result.reason.message,true);
  }
  async loadSettings(){
    const data=await api("/v1/settings");this.settings=data;this.callbacks.settings(data);
    if(!this.callbacks.isView("settings"))return;
    $("default-board").innerHTML=BOARDS.map(([id,label])=>`<option value="${id}">${label}</option>`).join("");$("default-board").value=data.default_board;
    const p=data.provider||{};$("tushare-status").textContent=`${p.configured?"已配置":"未配置"} · ${p.in_session?"交易时段":"非交易时段"}${p.last_success_at?" · 上次成功 "+time(p.last_success_at):""}${p.last_error?" · "+p.last_error:""}`;
    $("provider-user").value=p.user_id||"";$("provider-token").placeholder=p.token_mask||"未配置";$("provider-enabled").checked=!!p.enabled;$("provider-poll").value=p.poll_ms||5000;$("provider-universe").value=p.universe||"full";$("provider-test").disabled=!p.configured;
    const prefs=data.preferences||{};$("pref-theme").value=prefs.theme||"dark";$("pref-colors").value=prefs.colors||"red_up";$("pref-width").value=prefs.list_width||440;
    $("pref-columns").innerHTML=[...new Set([...(prefs.columns||[]),...COLUMNS.map(c=>c[0])])].map(key=>{const label=COLUMNS.find(c=>c[0]===key)?.[1]||key;return `<div class="column-option"><label class="check"><input type="checkbox" value="${key}" ${["symbol","name"].includes(key)?"checked disabled":(prefs.columns||[]).includes(key)?"checked":""}>${label}</label><button type="button" data-column-up aria-label="前移 ${label}">↑</button><button type="button" data-column-down aria-label="后移 ${label}">↓</button></div>`;}).join("");
    this.renderWatch(data.watchlist||[]);
    $("recent-views").innerHTML=(prefs.recent||[]).map(value=>{const [market,symbol]=value.split(":");return `<button type="button" data-recent-market="${esc(market)}" data-recent-symbol="${esc(symbol)}">${esc(value)}</button>`;}).join("")||"暂无近期查看";
    $("storage-info").innerHTML=[["配置文件",data.config_path],["数据库",data.db_path],["数据目录",data.data_root]].map(([key,value])=>`<dt>${key}</dt><dd>${esc(value)}</dd>`).join("");
  }
  renderWatch(list){
    this.watchSearch.setWatchlist(list);
    $("watch-edit").innerHTML=list.map(w=>`<li><span><strong>${esc(w.name||w.symbol)}</strong> ${esc(w.symbol)} · ${esc(w.market)}</span><div class="actions"><button data-move="-1" data-symbol="${esc(w.symbol)}" data-market="${esc(w.market)}" aria-label="上移 ${esc(w.symbol)}">↑</button><button data-move="1" data-symbol="${esc(w.symbol)}" data-market="${esc(w.market)}" aria-label="下移 ${esc(w.symbol)}">↓</button><button data-remove="${esc(w.symbol)}" data-market="${esc(w.market)}">移除</button></div></li>`).join("")||"<li>暂无自选，可在上方添加。</li>";
  }
  async loadJobs(){
    const result=await api("/v1/jobs");
    $("jobs-box").innerHTML=(result.jobs||[]).map(job=>{
      const active=["queued","running"].includes(job.status),canCancel=active&&job.phase!=="正在原子写入";
      return `<article class="job"><div class="job-title"><strong>#${job.id} · ${esc(job.kind)}</strong><span class="badge">${statusZh(job.status)}</span>${canCancel?`<button data-cancel="${job.id}">取消</button>`:["failed","partial","cancelled","interrupted"].includes(job.status)?`<button data-retry="${job.id}">重试</button>`:""}</div><progress max="100" value="${job.progress}" aria-label="任务进度"></progress><p>${esc(job.phase)} · ${job.progress}%</p>${job.error?`<p class="error-text">${esc(job.error)}</p>`:""}${job.result?`<details><summary>完整结果</summary><pre>${esc(JSON.stringify(job.result,null,2))}</pre></details>`:""}<small>${esc(time(job.finished_at||job.started_at||job.created_at))}</small></article>`;
    }).join("")||'<p class="muted">暂无任务，可先校验演示数据。</p>';
    for(const job of result.jobs||[]){if(["succeeded","failed","partial","cancelled","interrupted"].includes(job.status)&&!this.completed.has(job.id)){this.completed.add(job.id);if(this.jobsInitialized)this.callbacks.jobFinished(job);}}
    this.jobsInitialized=true;
    return result;
  }
  async submitJob(kind,params={}){const job=await invoke("job.submit",{kind,params});toast(`任务 #${job.id} 已提交，可在数据页查看进度`);await this.loadJobs();return job;}
  async previewImport(){
    const params=this.upload?{data:this.upload}:{path:$("import-path").value.trim()};
    if(!params.data&&!params.path)throw new Error("请选择文件或填写数据目录内的路径");
    const result=await invoke("import.preview",params);this.preview={params,fingerprint:result.fingerprint};const c=result.counts;
    $("import-preview").hidden=false;$("import-preview").innerHTML=`<h3>校验预览</h3><p>共 ${c.requested} 条 · 新增 ${c.inserted} · 更新 ${c.updated} · 不变 ${c.unchanged} · 陈旧 ${c.stale} · 无效 ${c.invalid}</p>${result.errors.length?`<details open><summary>行级错误（${result.errors.length}）</summary>${result.errors.map(e=>`<p class="error-text">${esc(e.dataset)} 第 ${e.row} 行 ${esc(e.symbol)}：${esc(e.message)}</p>`).join("")}</details>`:""}<button id="import-apply" class="primary" ${result.can_apply?"":"disabled"}>应用有效记录</button>`;
    $("import-result").textContent="预览不修改数据库。应用时会再次核对输入内容。";
  }
  async save(params){const data=await invoke("settings.save",params);this.settings=data;applyPreferences(data.preferences);this.callbacks.settings(data);$("settings-result").textContent="设置已保存";toast("设置已保存");return data;}
  bind(){
    const action=(element,event,handler)=>element.addEventListener(event,e=>{e.preventDefault();busy(e.submitter||e.currentTarget,()=>handler(e)).catch(()=>{});});
    action($("data-refresh"),"click",()=>this.loadData());
    action($("import-form"),"submit",()=>this.previewImport());
    $("demo-import").onclick=()=>{this.upload=null;$("import-upload").value="";$("import-path").value="examples/demo.board.json";this.preview=null;$("import-preview").hidden=true;};
    $("import-path").oninput=()=>{this.upload=null;this.preview=null;$("import-preview").hidden=true;};
    $("import-upload").onchange=async()=>{try{const file=$("import-upload").files[0];if(!file)return;if(file.size>6*1024*1024)throw new Error("文件超过 6 MiB，请放入数据目录后使用服务端路径导入");this.upload=JSON.parse(await file.text());$("import-path").value="";this.preview=null;await this.previewImport();}catch(e){toast(e.message,true);}};
    $("import-preview").onclick=e=>{if(e.target.id!=="import-apply"||!this.preview)return;busy(e.target,async()=>{const p=this.preview;await this.submitJob(p.params.data?"import.json":"import.file",{...p.params,fingerprint:p.fingerprint});this.preview=null;$("import-preview").hidden=true;}).catch(()=>{});};
    action($("tushare-sync"),"click",()=>this.submitJob("tushare.sync"));
    action($("tushare-valuations"),"click",()=>this.submitJob("tushare.valuations"));
    action($("tushare-indices"),"click",()=>this.submitJob("tushare.indices"));
    $("jobs-box").onclick=e=>{const button=e.target.closest("button");if(!button)return;busy(button,async()=>{await invoke(button.dataset.cancel?"job.cancel":"job.retry",{id:Number(button.dataset.cancel||button.dataset.retry)});await this.loadJobs();}).catch(()=>{});};
    action($("provider-form"),"submit",async()=>{const provider={user_id:$("provider-user").value.trim(),enabled:$("provider-enabled").checked,poll_ms:Number($("provider-poll").value),universe:$("provider-universe").value};if($("provider-token").value.trim())provider.token=$("provider-token").value.trim();await this.save({provider});$("provider-token").value="";await this.loadSettings();});
    action($("provider-test"),"click",()=>this.submitJob("provider.test"));
    action($("provider-clear"),"click",async()=>{await this.save({provider:{token:"",enabled:false}});await this.loadSettings();});
    $("default-board").onchange=async()=>{const old=this.settings.default_board;try{await this.save({default_board:$("default-board").value});}catch(e){$("default-board").value=old;toast(e.message,true);}};
    $("pref-columns").onclick=e=>{const row=e.target.closest(".column-option");if(!row)return;if(e.target.hasAttribute("data-column-up")&&row.previousElementSibling)row.previousElementSibling.before(row);if(e.target.hasAttribute("data-column-down")&&row.nextElementSibling)row.nextElementSibling.after(row);};
    $("recent-views").onclick=e=>{const b=e.target.closest("[data-recent-market]");if(b)this.callbacks.openRecent(b.dataset.recentMarket,b.dataset.recentSymbol);};
    $("workspace-presets").onclick=e=>{const b=e.target.closest("[data-preset]");if(!b)return;busy(b,async()=>{await this.save({default_board:b.dataset.preset==="watch"?"WATCH":"ALL",preferences:{list_width:b.dataset.preset==="watch"?340:420}});await this.loadSettings();}).catch(()=>{});};
    action($("preferences-form"),"submit",()=>this.save({preferences:{theme:$("pref-theme").value,colors:$("pref-colors").value,list_width:Number($("pref-width").value),columns:[...$("pref-columns").querySelectorAll("input:checked")].map(e=>e.value)}}));
    action($("watch-form"),"submit",async()=>{await invoke("watch.set",{symbol:$("watch-symbol").value.trim(),market:$("watch-market").value,name:$("watch-name").value.trim()||null,watched:true});await this.loadSettings();});
    $("watch-edit").onclick=e=>{const button=e.target.closest("button");if(!button)return;busy(button,async()=>{if(button.dataset.remove)await invoke("watch.set",{symbol:button.dataset.remove,market:button.dataset.market,watched:false});else await invoke("watch.move",{symbol:button.dataset.symbol,market:button.dataset.market,direction:Number(button.dataset.move)});await this.loadSettings();}).catch(()=>{});};
    action($("watch-import-btn"),"click",async()=>{await invoke("watch.import",{watchlist:JSON.parse($("watch-import").value)});await this.loadSettings();});
    $("watch-export").onclick=()=>download("GetRich-watchlist.json",this.settings?.watchlist||[]);
    action($("diagnostics-export"),"click",async()=>{const runtime=await api("/v2/system/realtime");download("GetRich-diagnostics.json",{version:"0.2.0",settings:this.settings,runtime});});
  }
}
