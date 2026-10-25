import {$,esc,fmt,pct,amount,direction,time} from "/runtime.js";
const phaseZh={pre:"未开盘",auction:"竞价",open:"开盘",morning:"午前",lunch:"午休",afternoon:"午后",close:"尾盘",post:"收盘"};
export function renderPulse(snapshot,navigate){
  const p=snapshot.pulse,q=snapshot.quality,t=p.tape||{},historical=!!p.stale;
  $("pulse-quality").className="quality-banner "+(q.level==="good"?"good":q.level==="degraded"?"warning":"bad");
  $("pulse-quality").textContent=`${historical?"历史快照":"已导入样本"} · ${q.level==="good"?"样本质量良好":q.level==="degraded"?"样本不完整":"数据质量不足"} · 有效 ${q.quoted_valid}/${q.universe_total} · 覆盖 ${fmt(q.coverage*100,1)}% · 过期 ${q.stale||0} · 无效 ${q.invalid||0} · 缺失 ${q.missing||0}`;
  $("pulse-empty").hidden=!!p.ready;$("pulse-body").hidden=!p.ready;
  if(!p.ready){$("pulse-empty").innerHTML=`<h2>暂无可分析的 A 股样本</h2><p>先导入有效报价，或配置数据源同步。异常记录可在任务结果中查看。</p><a class="button primary" href="#/data">前往数据页</a>`;return;}
  $("pulse-headline").textContent=p.headline||p.message;$("pulse-regime").textContent=p.regime?.label||"未判断";$("pulse-conf").textContent="置信度 "+({low:"低",medium:"中",high:"高"}[p.regime?.confidence]||"未知");
  $("pulse-session").textContent=`目标交易日 ${p.session.trade_date} · ${phaseZh[p.session.phase]||p.session.phase} · ${p.session.calendar_source==="trade_cal"?"已验证交易日历":"工作日日历估算"} · 来源日期 ${p.as_of.quote_trade_date||"未知"}`;
  $("pulse-reasons").innerHTML=(p.regime?.reasons||[]).map(reason=>`<li>${esc(reason)}</li>`).join("");
  $("pulse-indices").innerHTML=(p.indices||[]).map(i=>`<span class="${direction(i.change_pct)}">${esc(i.name)} ${pct(i.change_pct)}</span>`).join("");
  const metrics=[["上涨",t.n_up,"up",{change_filter:"up",sort:"change_pct"}],["下跌",t.n_down,"down",{change_filter:"down",sort:"change_pct_asc"}],["平盘",t.n_flat,"flat",{change_filter:"flat"}],["涨停估算",t.n_limit_up_est,"up",{change_filter:"limit_up"}],["跌停估算",t.n_limit_down_est,"down",{change_filter:"limit_down"}],["涨跌幅中位数",pct(t.median_pct),direction(t.median_pct),null],["样本成交额",amount(t.amount,"CNY"),"",null],["统计样本",`${t.n_quoted} 只`,"",null]];
  $("pulse-tape").innerHTML=metrics.map(([label,value,color,jump])=>`<button class="metric" ${jump&&!historical?`data-jump='${esc(JSON.stringify({board:"CN",...jump}))}'`:"disabled"}><span>${label}</span><strong class="${color}">${value??"—"}</strong></button>`).join("");
  const sectorHtml=rows=>rows.length?rows.map(s=>`<div class="sector-row"><button class="quiet" ${historical?"disabled":""} data-industry="${esc(s.name)}">${esc(s.name)}</button><span class="${direction(s.change_pct)}">${pct(s.change_pct)}</span><span>${amount(s.amount,"CNY")}</span><small>${s.n} 只</small></div>`).join(""):"<p class='muted'>有效行业样本不足</p>";
  const sectors=p.sectors?.items||[];$("pulse-sectors-up").innerHTML=sectorHtml(sectors.filter(s=>s.direction==="up"));$("pulse-sectors-down").innerHTML=sectorHtml(sectors.filter(s=>s.direction==="down"));
  $("view-pulse").querySelectorAll("[data-jump]").forEach(button=>button.onclick=()=>navigate(JSON.parse(button.dataset.jump)));
  $("view-pulse").querySelectorAll("[data-industry]").forEach(button=>button.onclick=()=>navigate({board:"CN",industry:button.dataset.industry,sort:"change_pct"}));
  const watch=p.watchlist||{};$("pulse-watch").innerHTML=`<h2>自选相对表现</h2>${watch.note?`<p class="muted">${esc(watch.note)}</p>`:`<p>相对样本中位数 ${pct(watch.vs_median_pct)}</p><p>${(watch.leaders||[]).map(w=>`${esc(w.name)} ${pct(w.pct)}`).join(" · ")}</p>`}`;
  $("pulse-extremes").textContent=`涨跌停为板块阈值估算，不是官方榜单。${historical?"历史快照下钻已关闭。":"点击指标按当前库筛选；数量可能随数据更新变化。"}`;
  $("pulse-capital").textContent="北向、两融与新闻能力尚未启用。当前仅展示有效报价形成的样本截面。";
  $("pulse-ts").textContent=`行情截至 ${time(snapshot.source_at)} · 计算于 ${time(snapshot.computed_at)} · 数据版本 ${snapshot.snapshot_version} / 截面 ${snapshot.snapshot_revision||0}`;
}
