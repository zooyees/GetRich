const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),path=require('node:path');
async function fixture(){
 const nodes=new Map(),model={quote:{symbol:'600001',market:'CN.SH'},active:true,fail:false,gate:null,revision:0,preferences:{research_mode:'swing'},requests:[]};
 const node=id=>{if(!nodes.has(id))nodes.set(id,{innerHTML:'',addEventListener(k,fn){this[k]=fn;}});return nodes.get(id);};
 const document={getElementById:node,activeElement:null};
 const context=vm.createContext({console,document,setTimeout,clearTimeout,AbortController,Intl,fetch:async(_,o)=>{
  const {method,params}=JSON.parse(o.body);model.requests.push({method,params});if(model.gate&&method==='stock.analysis')await model.gate;
  if(method==='settings.get')return {ok:true,json:async()=>({preferences:model.preferences})};
  if(method==='settings.save'){model.preferences=params.preferences;return {ok:true,json:async()=>({preferences:model.preferences})};}
  if(method==='stock.analysis')return {ok:true,json:async()=>({symbol:params.symbol,market:params.market,mode:params.mode,price_basis:params.basis,evaluation_date:'2024-01-03',workspace:{title:params.mode,metrics:[{label:params.mode+' metric',value:1}],warnings:[]},from:'2024-01-01',as_of:'2024-01-03',sample_count:3,data_version:1,warnings:[],sources:['fixture'],metrics:{},methodology:'fixture',note:{mode:params.mode,revision:model.revision,thesis:'saved '+params.symbol,risks:'',plan:'',review:''}})};
  if(method==='research.note.save'){if(model.fail)return {ok:false,status:400,json:async()=>({ok:false,error:{message:'记录版本冲突'}})};return {ok:true,json:async()=>({...params,revision:++model.revision})};}
  return {ok:true,json:async()=>({rows:[]})};
 }});
 const cache=new Map();async function module(name){if(cache.has(name))return cache.get(name);const m=new vm.SourceTextModule(fs.readFileSync(path.join(__dirname,'../../crates/getrich-api/web',name),'utf8'),{context});cache.set(name,m);await m.link(s=>module(s.slice(1)));return m;}
 const m=await module('analysis-workspace.js');await m.evaluate();const ui=new m.namespace.AnalysisWorkspace({current:()=>model.quote,active:()=>model.active});return {ui,model,nodes,document};
}
test('local analysis works without provider settings and escapes research input',async()=>{
 const f=await fixture();await f.ui.load();const d=f.ui.drafts.get(f.ui.key());d.thesis='</textarea><script>bad()</script>';d._dirty=true;f.ui.render();assert.ok(f.nodes.get('research-panel').innerHTML.includes('&lt;script&gt;'));assert.ok(!f.nodes.get('research-panel').innerHTML.includes('<script>'));
});
test('background reads retain unsaved drafts and typing focus',async()=>{
 const f=await fixture();await f.ui.load();const key=f.ui.key();f.ui.drafts.get(key).thesis='local draft';f.ui.drafts.get(key)._dirty=true;
 f.document.activeElement={dataset:{noteField:'thesis'}};const previous=f.nodes.get('research-panel').innerHTML;f.model.revision=4;await f.ui.load();assert.equal(f.ui.drafts.get(key).thesis,'local draft');assert.equal(f.nodes.get('research-panel').innerHTML,previous);
});
test('late selection results never display another instrument and drafts survive switching',async()=>{
 const f=await fixture();await f.ui.load();const oldKey=f.ui.key();f.ui.drafts.get(oldKey).thesis='PRIVATE_OLD_DRAFT';f.ui.drafts.get(oldKey)._dirty=true;
 let resolve;f.model.gate=new Promise(r=>resolve=r);const pending=f.ui.load();f.model.quote={symbol:'600002',market:'CN.SH'};resolve();await pending;f.model.gate=null;await f.ui.load();assert.ok(!f.nodes.get('research-panel').innerHTML.includes('PRIVATE_OLD_DRAFT'));assert.equal(f.ui.drafts.get(oldKey).thesis,'PRIVATE_OLD_DRAFT');
});
test('save conflict retains draft and successful save advances its revision',async()=>{
 const f=await fixture();await f.ui.load();const key=f.ui.key();f.ui.drafts.get(key).thesis='draft';f.ui.drafts.get(key)._dirty=true;f.model.fail=true;await f.ui.action('save');assert.equal(f.ui.drafts.get(key).thesis,'draft');assert.ok(f.ui.errors.get(key).includes('冲突'));f.model.fail=false;await f.ui.action('save');assert.equal(f.ui.drafts.get(key).revision,1);assert.ok(!f.ui.drafts.get(key)._dirty);
});
test('all three styles preserve separate drafts and save with the selected style',async()=>{
 const f=await fixture();await f.ui.load();const swing=f.ui.key();f.ui.drafts.get(swing).thesis='SWING_PRIVATE';f.ui.drafts.get(swing)._dirty=true;
 await f.ui.action('mode:intraday');const short=f.ui.key();assert.notEqual(short,swing);f.ui.drafts.get(short).thesis='SHORT_PRIVATE';f.ui.drafts.get(short)._dirty=true;
 await f.ui.action('mode:value');assert.ok(f.nodes.get('research-panel').innerHTML.includes('商业逻辑与竞争优势'));assert.ok(f.nodes.get('research-panel').innerHTML.includes('value metric'));assert.ok(!f.nodes.get('research-panel').innerHTML.includes('SHORT_PRIVATE'));
 await f.ui.action('save');const saved=f.model.requests.findLast(r=>r.method==='research.note.save');assert.equal(saved.params.mode,'value');assert.equal(saved.params.analysis_as_of,'2024-01-03');
 f.document.activeElement={dataset:{noteField:'thesis'}};await f.ui.action('mode:swing');assert.equal(f.ui.drafts.get(swing).thesis,'SWING_PRIVATE');assert.ok(f.nodes.get('research-panel').innerHTML.includes('SWING_PRIVATE'));
 await f.ui.action('mode:intraday');assert.equal(f.ui.drafts.get(short).thesis,'SHORT_PRIVATE');await f.ui.preferenceWrite;assert.equal(f.model.preferences.research_mode,'intraday');
});
test('late analysis from a different style cannot replace the visible style',async()=>{
 const f=await fixture();await f.ui.load();let release;f.model.gate=new Promise(r=>release=r);const old=f.ui.load();await new Promise(r=>setImmediate(r));const current=f.ui.action('mode:value');await new Promise(r=>setImmediate(r));release();await Promise.all([old,current]);assert.equal(f.ui.mode,'value');assert.ok(f.nodes.get('research-panel').innerHTML.includes('value metric'));assert.ok(!f.nodes.get('research-panel').innerHTML.includes('5日收益 %'));
});
test('the preferred research style restores on the first load',async()=>{
 const f=await fixture();f.model.preferences.research_mode='value';await f.ui.load();assert.equal(f.ui.mode,'value');assert.equal(f.model.requests.find(r=>r.method==='stock.analysis').params.mode,'value');
});
test('late raw analysis cannot replace adjusted analysis and saved evidence retains the basis',async()=>{
 const f=await fixture(),preferences={price_basis:'raw'};f.ui.preferences=()=>preferences;await f.ui.load();let release;f.model.gate=new Promise(r=>release=r);const old=f.ui.load();await new Promise(r=>setImmediate(r));preferences.price_basis='qfq';const current=f.ui.load();await new Promise(r=>setImmediate(r));release();await Promise.all([old,current]);f.model.gate=null;assert.equal(f.ui.reports.get(f.ui.reportKey()).price_basis,'qfq');await f.ui.action('save');assert.equal(f.model.requests.findLast(r=>r.method==='research.note.save').params.basis,'qfq');
});
