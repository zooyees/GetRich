import {$,esc,api} from "/runtime.js";
export class WatchSearch {
  constructor(isActive){
    this.isActive=isActive;this.generation=0;this.rows=[];this.selected=-1;this.watchlist=[];this.query="";this.market="ALL";
    $("watch-search").oninput=()=>this.schedule();$("watch-search-market").onchange=()=>this.schedule();
    $("watch-search").onkeydown=event=>{if(!this.rows.length)return;if(event.key==="ArrowDown"||event.key==="ArrowUp"){event.preventDefault();this.selected=Math.max(0,Math.min(this.rows.length-1,this.selected+(event.key==="ArrowDown"?1:-1)));this.render();}if(event.key==="Enter"){event.preventDefault();this.choose(this.selected<0?0:this.selected);}};
    $("watch-search-results").onclick=event=>{const button=event.target.closest("[data-watch-result]");if(button)this.choose(Number(button.dataset.watchResult));};
  }
  schedule(){clearTimeout(this.timer);this.generation++;this.controller?.abort();this.rows=[];this.selected=-1;this.query=$("watch-search").value.trim();this.market=$("watch-search-market").value||"ALL";this.render();if(this.query)this.timer=setTimeout(()=>this.search(),180);}
  async search(){
    const generation=++this.generation,query=this.query,market=this.market;this.controller?.abort();this.controller=new AbortController();
    if(!query||!this.isActive())return;$("watch-search-note").textContent="正在查找…";
    try{const page=await api("/v1/quotes?"+new URLSearchParams({board:market,q:query,sort:"symbol",limit:"20"}),{signal:this.controller.signal});
      if(generation!==this.generation||!this.isActive()||query!==$("watch-search").value.trim()||market!==($("watch-search-market").value||"ALL"))return;
      this.rows=page.rows||[];this.matched=page.matched||0;this.render();
    }catch(error){if(error.name!=="AbortError"&&generation===this.generation&&this.isActive())$("watch-search-note").textContent="搜索失败："+error.message;}
  }
  setWatchlist(list){this.watchlist=list;this.render();}
  choose(index){const row=this.rows[index];if(!row)return;this.selected=index;$("watch-symbol").value=row.symbol;$("watch-market").value=row.market;$("watch-name").value=row.name||"";this.render();$("watch-search-note").textContent=`已选择 ${row.name||row.symbol} · ${row.symbol} · ${row.market}，点击“添加自选”保存。`;}
  render(){
    $("watch-search-results").innerHTML=this.rows.map((row,index)=>{const watched=this.watchlist.some(w=>w.symbol===row.symbol&&w.market===row.market);return `<button type="button" data-watch-result="${index}" class="${index===this.selected?"on":""}" aria-pressed="${index===this.selected}"><strong>${esc(row.name||row.symbol)}</strong><span>${esc(row.symbol)} · ${esc(row.market)}${watched?" · 已自选":""}</span></button>`;}).join("");
    $("watch-search-results").hidden=!this.rows.length;
    $("watch-search-note").textContent=!this.query?"从本地标的库搜索，选择结果后自动填写下方代码、市场和名称。":this.rows.length?`找到 ${this.matched} 只，显示前 ${this.rows.length} 只；↑↓ 选择，Enter 填入。`:"未找到匹配标的，可更换关键词或市场，或先同步股票列表。";
  }
}
