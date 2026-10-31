const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const root=path.resolve(__dirname,'../../crates/getrich-api/web');
async function runtime(fetch){const context=vm.createContext({fetch,AbortController,setTimeout,clearTimeout,document:{getElementById(){}}});const m=new vm.SourceTextModule(fs.readFileSync(path.join(root,'runtime.js'),'utf8'),{context});await m.link(()=>{});await m.evaluate();return m.namespace;}
test('repeated same-URL refreshes share one slow request and never cancel it',async()=>{
  let resolve,calls=0,signal;const gate=new Promise(r=>resolve=r);
  const api=await runtime(async(_,o)=>{calls++;signal=o.signal;await gate;return {ok:true,json:async()=>({ok:true,data:{version:7}})};});const reads=new api.Reads();
  const first=reads.get('quote','/quote/A');const all=Array.from({length:100},()=>reads.get('quote','/quote/A'));
  assert.equal(calls,1);assert.equal(signal.aborted,false);resolve();assert.equal((await first).version,7);for(const p of all)assert.equal((await p).version,7);
});
test('changed selection and route cancellation still abort obsolete requests',async()=>{
  const requests=[];const api=await runtime((url,o)=>new Promise((_,reject)=>{requests.push({url,signal:o.signal});o.signal.addEventListener('abort',()=>reject(Object.assign(new Error('abort'),{name:'AbortError'})));}));const reads=new api.Reads();
  const old=reads.get('detail','/stock/A').catch(e=>e.name);const next=reads.get('detail','/stock/B').catch(e=>e.name);
  assert.equal(requests[0].signal.aborted,true);assert.equal(requests[1].signal.aborted,false);reads.cancel();assert.equal(await old,'AbortError');assert.equal(await next,'AbortError');
});
test('weekly and monthly tail refresh replaces the whole current period without duplicating bars',async()=>{
  const mod=await import('data:text/javascript;base64,'+Buffer.from(fs.readFileSync(path.join(root,'bar-refresh.js'),'utf8')).toString('base64'));
  assert.equal(mod.tailFrom('1w','2026-10-27'),'2026-10-26');assert.equal(mod.tailFrom('1mo','2026-10-27'),'2026-10-01');
  const rows=[{ts:'2026-10-23',close:10},{ts:'2026-10-26',close:11}],meta={data_version:1,moving_averages:[{period:250,values:[8,9]}]};
  const tail={data_version:2,rows:[{ts:'2026-10-27',close:12}],moving_averages:[{period:250,values:[9.5]}]};const next=mod.mergeTail(meta,rows,tail,mod.tailFrom('1w',rows.at(-1).ts));
  assert.deepEqual(next.rows,[rows[0],tail.rows[0]]);assert.deepEqual(next.meta.moving_averages[0].values,[8,9.5]);assert.equal(next.meta.count,2);
  assert.equal(mod.mergeTail(next.meta,next.rows,{...tail,data_version:0},'2026-10-26').meta,next.meta);
});
