const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const path=require('node:path');
const root=path.resolve(__dirname,'../../crates/getrich-api/web');

test('refresh action submits one acquisition job while repeated local reads never resubmit',async()=>{
  const f=await fixture();
  await f.map.sync();await f.map.sync();await f.map.refresh();
  const submitted=f.model.requests.filter(r=>r.body?.method==='job.submit');
  assert.equal(submitted.length,1);assert.equal(submitted[0].body.params.kind,'market.sync');
  assert.equal(f.nodes.get('pulse-refresh').disabled,true);
  f.model.job={id:42,status:'partial',phase:'partial',progress:100,result:{message:'已更新 1 只',warnings:['市值权限不足']}};
  await f.map.refresh();
  assert.equal(f.map.tiles().length,1);
  assert.match(f.nodes.get('map-sync-status').textContent,/市值权限不足/);
  assert.equal(f.nodes.get('pulse-refresh').disabled,false);
});
test('missing configuration leaves refresh usable and empty configured stores bootstrap only once',async()=>{
  const f=await fixture();f.model.configured=false;
  await f.map.sync();assert.equal(f.nodes.get('pulse-refresh').disabled,false);
  assert.match(f.nodes.get('map-sync-status').textContent,/配置 Tushare/);
  const g=await fixture();g.model.data.coverage.universe=0;g.model.data.groups=[];
  await g.map.refresh();await g.map.refresh();
  assert.equal(g.model.requests.filter(r=>r.body?.method==='job.submit').length,1);
});
const deferred=()=>{let resolve;return {promise:new Promise(r=>resolve=r),resolve};};
function snapshot(symbol='600001'){
  const q={symbol,name:'测试银行',market:'CN.SH',currency:'CNY',last:11,change_pct:10,trade_date:'2024-01-03',source:'fixture.daily',source_kind:'daily'};
  return {query:{board:'CN'},computed_at:'2024-01-03T12:00:00Z',data_version:1,trade_date:'2024-01-03',historical:true,currency:'CNY',metric:'equal',metric_label:'等面积',coverage:{universe:1,quoted:1,displayed:1,missing_quote:0,other_date:0,invalid:0,missing_weight:0,delayed:0,unclassified:0,up:1,down:0,flat:0,unknown_change:0},notes:['fixture'],groups:[{name:'银行',mean_pct:10,bounds:{x:0,y:0,w:1,h:1},tiles:[{key:'CN.SH:'+symbol,quote:q,industry:'银行',weight:1,bounds:{x:0,y:0,w:1,h:1}}]}]};
}
async function fixture(){
  const nodes=new Map(),draws=[],listeners=new Map(),model={active:true,data:snapshot(),pending:[],opens:[],requests:[],configured:true,job:{id:42,status:'queued',phase:'等待执行',progress:0}};
  const ctx=new Proxy({},{get:(_,key)=>key==='fillRect'?(...args)=>draws.push(args):()=>{},set:()=>true});
  const node=id=>{if(!nodes.has(id))nodes.set(id,{id,hidden:false,value:'',checked:false,style:{},dataset:{},innerHTML:'',textContent:'',setAttribute(k,v){this[k]=v;},removeAttribute(k){delete this[k];},querySelectorAll:()=>[],getBoundingClientRect:()=>({left:0,top:0,width:1000,height:600}),getContext:()=>ctx,addEventListener(type,callback){listeners.set(id+':'+type,callback);},setPointerCapture(){},releasePointerCapture(){}});return nodes.get(id);};
  const document={getElementById:node,documentElement:{dataset:{colors:'red_up'}},fullscreenElement:null};
  const context=vm.createContext({console,document,window:{devicePixelRatio:2},URLSearchParams,AbortController,Intl,Date,ResizeObserver:class{observe(){}},setTimeout:()=>1,clearTimeout(){},fetch:async(url,options={})=>{
      model.requests.push({url,body:options.body?JSON.parse(options.body):null});
      if(url==='/v1/settings')return {ok:true,json:async()=>({provider:{configured:model.configured}})};
      if(url==='/v1/invoke')return {ok:true,json:async()=>model.job};
      const pending=model.pending.shift();return pending?pending.promise:{ok:true,json:async()=>model.data};
    }});
  const cache=new Map();async function module(name){if(cache.has(name))return cache.get(name);const mod=new vm.SourceTextModule(fs.readFileSync(path.join(root,name),'utf8'),{context,identifier:name});cache.set(name,mod);await mod.link(spec=>module(spec.slice(1)));return mod;}
  const mod=await module('market-map.js');await mod.evaluate();const map=new mod.namespace.MarketMap({isActive:()=>model.active,openStock:q=>model.opens.push(q)});
  return {map,model,nodes,draws,listeners,exports:mod.namespace,document};
}
test('heatmap paints, selects and opens the underlying instrument',async()=>{
  const f=await fixture();await f.map.refresh();assert.ok(f.draws.length>1);assert.equal(f.map.hits.length,1);assert.match(f.nodes.get('map-state').textContent,/历史截面/);
  f.listeners.get('market-canvas:pointerdown')({clientX:500,clientY:300,pointerId:1});
  f.listeners.get('market-canvas:pointerup')({clientX:500,clientY:300,pointerId:1});
  assert.equal(f.map.selected,'CN.SH:600001');assert.equal(f.nodes.get('map-selected-name').textContent,'测试银行');
  f.nodes.get('map-open-stock').onclick();assert.equal(f.model.opens[0].symbol,'600001');
  f.listeners.get('market-canvas:dblclick')({clientX:500,clientY:300});assert.equal(f.model.opens.length,2);
});
test('late map responses cannot overwrite newer filters or a route change',async()=>{
  const f=await fixture(),old=deferred();f.model.pending.push(old);const first=f.map.refresh();
  f.model.data=snapshot('600002');await f.map.change({metric:'equal'});
  old.resolve({ok:true,json:async()=>snapshot('WRONG')});await first;assert.equal(f.map.tiles()[0].quote.symbol,'600002');
  const late=deferred();f.model.pending.push(late);const request=f.map.refresh();f.model.active=false;f.map.deactivate();late.resolve({ok:true,json:async()=>snapshot('WRONG')});await request;assert.equal(f.map.tiles()[0].quote.symbol,'600002');
});
test('zoom respects pointer anchors and pan bounds; unknown colors differ from flat',async()=>{
  const f=await fixture(),m=f.exports;
  const view=m.zoomAt({scale:1,x:0,y:0},2,250,150,1000,600);assert.equal(view.scale,2);assert.equal(view.x,-250);assert.equal(view.y,-150);
  const reset=m.zoomAt(view,.1,250,150,1000,600);assert.equal(reset.scale,1);assert.equal(reset.x,0);assert.equal(reset.y,0);
  assert.notEqual(m.tileColor(null),m.tileColor(0));assert.equal(m.tileColor(4),m.tileColor(-4,true));
  const capped=m.clampView({scale:3,x:9999,y:-9999},1000,600);assert.equal(capped.x,0);assert.equal(capped.y,-1200);
});
test('search and watch highlights preserve the underlying data and keyboard selection',async()=>{
  const f=await fixture();await f.map.refresh();f.nodes.get('map-search').value='测试';f.nodes.get('map-search').oninput();assert.match(f.nodes.get('map-search-count').textContent,/1 个匹配/);assert.equal(f.map.tiles().length,1);
  f.nodes.get('map-highlight-watch').checked=true;f.nodes.get('map-highlight-watch').onchange();assert.equal(f.map.tiles().length,1);
  f.listeners.get('market-canvas:keydown')({key:'ArrowRight',preventDefault(){}});assert.equal(f.map.selected,'CN.SH:600001');
  f.listeners.get('market-canvas:keydown')({key:'Enter',preventDefault(){}});assert.equal(f.model.opens[0].symbol,'600001');
});
test('every map control is present once in the shipped HTML',()=>{
  const html=fs.readFileSync(path.join(root,'index.html'),'utf8'),ids=[...html.matchAll(/id="([^"]+)"/g)].map(m=>m[1]);
  assert.equal(new Set(ids).size,ids.length,'duplicate control IDs');
  for(const match of fs.readFileSync(path.join(root,'market-map.js'),'utf8').matchAll(/\$\("([^"]+)"\)/g))assert.ok(ids.includes(match[1]),'missing #'+match[1]);
});
test('resizing a zoomed and panned map keeps the viewport within bounds',async()=>{
  const f=await fixture();await f.map.refresh();f.map.view={scale:3,x:-2000,y:-1200};
  f.map.canvas.getBoundingClientRect=()=>({left:0,top:0,width:500,height:300});f.map.draw();
  assert.equal(f.map.view.x,-1000);assert.equal(f.map.view.y,-600);assert.equal(f.map.view.scale,3);
});
