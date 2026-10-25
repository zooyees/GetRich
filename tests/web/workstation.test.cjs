const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const vm=require('node:vm');
const root=path.resolve(__dirname,'../../crates/getrich-api/web');
const deferred=()=>{let resolve;const promise=new Promise(r=>resolve=r);return {promise,resolve};};
async function fixture(){
  const nodes=new Map(),timers=new Map();let timer=0;
  class Element{
    constructor(){this.hidden=false;this.textContent='';this.innerHTML='';this.value='';this.checked=false;this.disabled=false;this.dataset={};this.rows=[];this.cells=[];this.clientWidth=0;this.clientHeight=0;this.attrs={};this.classList={toggle(){},add(){},remove(){}};}
    getContext(){return {};}
    addEventListener(name,fn){this['on'+name]=fn;}setAttribute(k,v){this.attrs[k]=v;}removeAttribute(k){delete this.attrs[k];}
    querySelectorAll(){return [];}querySelector(){return null;}appendChild(){}focus(){}click(){this.onclick?.({target:this,preventDefault(){}});}closest(){return this;}
  }
  const document={getElementById(id){if(!nodes.has(id))nodes.set(id,new Element());return nodes.get(id);},querySelectorAll(){return [];},addEventListener(){},documentElement:{dataset:{},style:{setProperty(){}}},hidden:false};
  const model={rows:[{symbol:'600001',market:'CN.SH',name:'测试',last:11,change_pct:10,has_quote:true}],stockWait:null,pulseWait:null,requests:[],research:{datasets:{history:{rows:[],needs_refresh:true}}},job:{id:91,status:'queued'}};
  const stats={listings:1,quotes:1,bars:1,empty:false};
  const settings={default_board:'CN',preferences:{theme:'light',columns:['symbol','name','last'],recent:[]},provider:{configured:false},watchlist:[]};
  const fetch=async (url,options={})=>{
    let data;if(url.startsWith('/v1/events'))return {ok:true,body:{getReader:()=>({read:async()=>({done:true})})}};
    if(url.startsWith('/v1/quotes')){if(model.searchWait&&new URL('http://local'+url).searchParams.get('q')==='old')return model.searchWait.promise;data={rows:model.rows,matched:model.rows.length,stats,empty:!model.rows.length,empty_kind:'no_match',empty_message:'无匹配'};}
    else if(url.startsWith('/v1/settings'))data=settings;
    else if(url.startsWith('/v1/stocks/')){
      if(model.stockWait)return model.stockWait.promise;
      const parts=url.split('/');const market=decodeURIComponent(parts[3]),symbol=decodeURIComponent(parts[4]);
      data=url.includes('/bars?')?{rows:[],empty_message:'暂无'}:{quote:model.rows.find(r=>r.symbol===symbol&&r.market===market)||{symbol,market},listing:{},metadata:[],empty:{no_quote:false},watched:true};
    }else if(url.startsWith('/v2/pulse')){if(model.pulseWait)return model.pulseWait.promise;data={quality:{},pulse:{}};}
    else if(url.startsWith('/v1/jobs'))data={jobs:[]};
    else if(url.startsWith('/v1/runs'))data={runs:[]};
    else if(url.startsWith('/v1/health'))data={version:'test',instance_id:'fixture'};
    else if(url.startsWith('/v1/invoke')){const p=JSON.parse(options.body||'{}');model.requests.push(p);data=p.method==='stock.research'?model.research:p.method==='job.submit'||p.method==='job.get'?model.job:{watched:false};}
    else data={};
    return {ok:true,json:async()=>({ok:true,data})};
  };
  const context=vm.createContext({console,document,window:{addEventListener(){}},location:{hash:'#/quotes?board=CN'},fetch,URLSearchParams,URL,AbortController,TextDecoder,Intl,Date,Blob,
    setTimeout(fn,delay){timers.set(++timer,{fn,delay});return timer;},clearTimeout(id){timers.delete(id);},setInterval(){},ResizeObserver:class{observe(){}},getComputedStyle(){return {getPropertyValue:()=>''};}});
  const cache=new Map();
  async function module(name){
    if(cache.has(name))return cache.get(name);
    let source=fs.readFileSync(path.join(root,name),'utf8');
    if(name==='app.js')source+='\nexport {state,route,loadBoard,loadStock,scheduleRefresh,management,events,stockResearch};';
    if(name==='pulse.js')source='export function renderPulse() {}';
    if(name==='market-map.js')source='export class MarketMap {constructor(){} deactivate(){} async refresh(){} setPulse(){} draw(){}}';
    const mod=new vm.SourceTextModule(source,{context,identifier:name});cache.set(name,mod);
    await mod.link(spec=>module(spec.slice(1)));return mod;
  }
  const app=await module('app.js');await app.evaluate();
  return {app:app.namespace,context,nodes,timers,model,settings};
}
test('search is scoped to the quote route',async()=>{
  const f=await fixture();f.context.location.hash='#/settings';await f.app.route();
  f.nodes.get('quote-search').value='不会跳页';f.nodes.get('quote-search').oninput();
  for(const t of [...f.timers.values()])if(t.delay===200)t.fn();
  assert.equal(f.app.state.view,'settings');assert.equal(f.context.location.hash,'#/settings');
});

test('explicit stock navigation survives passive refresh outside the active list',async()=>{
  const f=await fixture();f.model.rows=[{symbol:'AAPL',market:'US',name:'Apple'}];
  f.context.location.hash='#/stock/CN.SH/600519?board=US';await f.app.route();
  assert.equal(f.app.state.detail.quote.symbol,'600519');
  await f.app.loadBoard(2,true);assert.equal(f.app.state.selectedKey,'CN.SH:600519');assert.equal(f.app.state.detail.quote.symbol,'600519');
  f.model.rows=[];await f.app.loadBoard(2,true);assert.equal(f.app.state.detail.quote.symbol,'600519');
});

test('watch search fills name, code and exact market, with keyboard selection',async()=>{
  const f=await fixture();f.context.location.hash='#/settings';await f.app.route();
  f.model.rows=[{symbol:'600519',market:'CN.SH',name:'贵州茅台'}];
  f.nodes.get('watch-search').value='茅台';f.nodes.get('watch-search-market').value='ALL';
  const search=f.app.management.watchSearch;search.schedule();await search.search();
  assert.match(f.nodes.get('watch-search-results').innerHTML,/贵州茅台/);
  f.nodes.get('watch-search').onkeydown({key:'Enter',preventDefault(){}});
  assert.equal(f.nodes.get('watch-symbol').value,'600519');assert.equal(f.nodes.get('watch-market').value,'CN.SH');assert.equal(f.nodes.get('watch-name').value,'贵州茅台');
  search.query='new';f.nodes.get('watch-search').value='new';search.market='US';f.nodes.get('watch-search-market').value='US';f.model.rows=[];await search.search();assert.match(f.nodes.get('watch-search-note').textContent,/未找到/);
});

test('late watch search cannot replace a newer result',async()=>{
  const f=await fixture();f.context.location.hash='#/settings';await f.app.route();
  const search=f.app.management.watchSearch;f.nodes.get('watch-search-market').value='ALL';
  f.nodes.get('watch-search').value='old';search.query='old';f.model.searchWait=deferred();const pending=search.search();
  f.nodes.get('watch-search').value='茅台';search.query='茅台';f.model.rows=[{symbol:'600519',market:'CN.SH',name:'贵州茅台'}];await search.search();
  f.model.searchWait.resolve({ok:true,json:async()=>({rows:[{symbol:'WRONG',market:'US'}],matched:1})});await pending;
  assert.equal(search.rows[0].symbol,'600519');assert.ok(!f.nodes.get('watch-search-results').innerHTML.includes('WRONG'));
});

test('one snapshot bar still starts one automatic history job and manual retry is possible',async()=>{
  const f=await fixture();f.settings.provider.configured=true;
  await f.app.stockResearch.update();await f.app.stockResearch.update();
  assert.equal(f.model.requests.filter(r=>r.method==='job.submit').length,1);
  assert.equal(f.nodes.get('research-refresh').disabled,true);
  f.model.job={id:91,status:'partial',result:{warnings:['权限不足']}};
  const poll=[...f.timers.values()].find(t=>t.delay===1500);await poll.fn();
  assert.equal(f.nodes.get('research-refresh').disabled,false);
  assert.match(f.nodes.get('research-status').textContent,/权限不足/);
  await f.app.stockResearch.ensure('history',true);
  assert.equal(f.model.requests.filter(r=>r.method==='job.submit').length,2);
});

test('announcement titles are escaped and unsafe external protocols are rejected',async()=>{
  const f=await fixture();f.app.stockResearch.tab='announcements';
  f.app.stockResearch.data={datasets:{announcements:{rows:[{ann_date:'20261023',title:'<img src=x onerror=evil()>',url:'javascript:evil()'},{title:'报告',url:'https://example.com/report.pdf'}]}}};
  f.app.stockResearch.render();const html=f.nodes.get('research-panel').innerHTML;
  assert.ok(!html.includes('href="javascript:'));assert.ok(html.includes('&lt;img'));assert.ok(html.includes('rel="noopener noreferrer"'));
});
test('late pulse response cannot navigate away from settings',async()=>{
  const f=await fixture();f.model.pulseWait=deferred();f.context.location.hash='#/pulse';const pending=f.app.route();
  f.context.location.hash='#/settings';await f.app.route();f.model.pulseWait.resolve({ok:true,json:async()=>({quality:{},pulse:{}})});await pending;
  assert.equal(f.app.state.view,'settings');assert.equal(f.context.location.hash,'#/settings');
});
test('selection follows instrument key after sorting and clears on empty results',async()=>{
  const f=await fixture();f.model.rows.push({symbol:'600002',market:'CN.SH',last:12});
  f.app.state.selectedKey='CN.SH:600001';f.model.rows.reverse();await f.app.loadBoard(1,true);
  assert.equal(f.app.state.selectedKey,'CN.SH:600001');
  f.model.rows=[];await f.app.loadBoard(1,true);assert.equal(f.app.state.selectedKey,null);assert.equal(f.app.state.detail,null);assert.equal(f.nodes.get('detail-content').hidden,true);
});
test('in-flight stock response is discarded after leaving quotes',async()=>{
  const f=await fixture();f.model.stockWait=deferred();const old=f.app.loadStock(f.model.rows[0],1);
  f.context.location.hash='#/settings';await f.app.route();f.model.stockWait.resolve({ok:true,json:async()=>({rows:[],quote:{symbol:'WRONG'},empty:{}})});await old;
  assert.equal(f.app.state.view,'settings');assert.notEqual(f.app.state.detail?.quote?.symbol,'WRONG');
});
test('removing a watch from settings preserves route',async()=>{
  const f=await fixture();f.context.location.hash='#/settings';await f.app.route();
  const button={disabled:false,dataset:{remove:'600001',market:'CN.SH'},setAttribute(){},removeAttribute(){}};
  f.nodes.get('watch-edit').onclick({target:{closest:()=>button}});for(let i=0;i<20;i++)await Promise.resolve();
  assert.equal(f.app.state.view,'settings');assert.equal(f.context.location.hash,'#/settings');assert.equal(button.disabled,false);
});
test('default board is loaded before initial quotes',async()=>{const f=await fixture();assert.equal(f.app.state.board,'CN');});
test('static control identifiers exist in HTML',()=>{
  const html=fs.readFileSync(path.join(root,'index.html'),'utf8');const ids=new Set([...html.matchAll(/id="([^"]+)"/g)].map(m=>m[1]));
  for(const file of ['app.js','management.js','pulse.js'])for(const m of fs.readFileSync(path.join(root,file),'utf8').matchAll(/\$\("([^"]+)"\)/g)){if(m[1]!=='import-apply')assert.ok(ids.has(m[1]),`${file}: missing #${m[1]}`);}
});
test('short charts, pointer padding and SMA retain correct values',async()=>{
  const mod=await import('data:text/javascript;base64,'+Buffer.from(fs.readFileSync(path.join(root,'chart.js'),'utf8')).toString('base64'));
  for(const n of [0,1,2,7,8,80,800])for(const span of [0,1,7,8,120,1000]){const r=mod.range(n,span);assert.ok(r.start>=0&&r.end<=n&&r.start<=r.end);}
  assert.equal(mod.hitIndex(705,0,10,70,10),0);assert.equal(mod.hitIndex(795,0,10,70,10),9);assert.equal(mod.hitIndex(10,0,10,70,10),null);
  const values=Array.from({length:100},(_,i)=>i+1);const ma=mod.sma(values,5);assert.equal(ma[50],49);assert.equal(ma.slice(45)[5],49);
});
