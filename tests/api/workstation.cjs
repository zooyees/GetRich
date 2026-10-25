// Process-level acceptance tests. Every run owns an isolated data directory.
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const net=require('node:net');
const {spawn}=require('node:child_process');
const repo=path.resolve(__dirname,'../..');
const binary=process.env.GETRICH_TEST_BINARY||path.join(repo,'target/debug',process.platform==='win32'?'getrich.exe':'getrich');
const root=fs.mkdtempSync(path.join(repo,'target/acceptance-'));
fs.mkdirSync(path.join(root,'examples'));fs.copyFileSync(path.join(repo,'docs/examples/demo.board.json'),path.join(root,'examples/demo.board.json'));
fs.writeFileSync(path.join(root,'config.json'),JSON.stringify({data:{tushare:{enabled:false,token:''}},universe:{watchlist:[],default_board:'ALL'}}));
const children=[],controllers=[],results=[];
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function port(){return new Promise(resolve=>{const s=net.createServer();s.listen(0,'127.0.0.1',()=>{const p=s.address().port;s.close(()=>resolve(p));});});}
async function server(){
  const bind='127.0.0.1:'+await port();
  const child=spawn(binary,['serve','--bind',bind],{cwd:repo,windowsHide:true,env:{...process.env,GETRICH_DATA_ROOT:root,GETRICH_CONFIG:path.join(root,'config.json'),GETRICH_DB:path.join(root,'test.db'),GETRICH_TUSHARE_TOKEN:'',GETRICH_TUSHARE_USER:''},stdio:['ignore','pipe','pipe']});children.push(child);
  child.on('error',error=>console.error(error.message));let logs='';child.stderr.on('data',data=>logs+=data.toString());child.stdout.on('data',()=>{});
  const base='http://'+bind;
  for(let i=0;i<100;i++){try{const r=await fetch(base+'/v1/health');if(r.ok)return base;}catch{}if(child.exitCode!=null)throw Error('server exited: '+logs);await sleep(100);}
  throw Error('startup timed out: '+logs);
}
async function api(base,url,options={}){
  const r=await fetch(base+url,{...options,signal:AbortSignal.timeout(15000)});const body=await r.json();assert.ok(r.ok,JSON.stringify(body));assert.notEqual(body.ok,false,JSON.stringify(body));return body.ok===true?body.data:body;
}
const invoke=(base,method,params={})=>api(base,'/v1/invoke',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({method,params})});
async function check(name,operation){const start=Date.now();await operation();results.push({name,passed:true,ms:Date.now()-start});console.log('PASS '+name);}
async function waitJob(base,id){for(let i=0;i<150;i++){const job=await invoke(base,'job.get',{id});if(!['queued','running','cancelling'].includes(job.status))return job;await sleep(100);}throw Error('job timeout '+id);}
(async()=>{
  const a=await server();let before;
  await check('embedded modules and settings contract',async()=>{
    for(const url of ['/','/app.js','/runtime.js','/chart.js','/stock-research.js','/watch-search.js','/pulse.js','/market-map.js','/management.js']){const r=await fetch(a+url);assert.equal(r.status,200);assert.ok((await r.text()).length>100);}
    const settings=await api(a,'/v1/settings');assert.equal(settings.default_board,'ALL');assert.equal(settings.provider.configured,false);assert.ok(!JSON.stringify(settings).includes('"token":'));
    const bad=await fetch(a+'/v1/invoke',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({method:'settings.save',params:{default_board:'INVALID'}})});assert.equal(bad.status,400);
  });
  await check('preview does not write; queued file import persists',async()=>{
    const preview=await invoke(a,'import.preview',{path:'examples/demo.board.json'});assert.ok(preview.counts.requested>0);assert.equal((await api(a,'/v1/stats')).listings,0);
    const job=await invoke(a,'job.submit',{kind:'import.file',params:{path:'examples/demo.board.json',fingerprint:preview.fingerprint}});const result=await waitJob(a,job.id);assert.equal(result.status,'succeeded');assert.equal(result.result.counts.invalid,0);before=await api(a,'/v1/stats');assert.ok(before.quotes>0);
    const duplicate=await invoke(a,'import.preview',{path:'examples/demo.board.json'});assert.equal(duplicate.counts.inserted,0);assert.equal(duplicate.counts.updated,0);
  });
  await check('invalid quote and OHLC return row errors',async()=>{
    const result=await invoke(a,'import.json',{data:{quotes:[{symbol:'600090',market:'CN.SH',ts:'2024-01-02',last:-10,volume:-1}],bars:[{symbol:'600091',market:'CN.SH',ts:'2024-01-02',open:10,high:9,low:11,close:10}]}});
    assert.equal(result.status,'partial');assert.equal(result.counts.invalid,2);assert.equal(result.errors.length,2);assert.match(result.errors[0],/quotes 第 1 行/);assert.equal((await api(a,'/v1/stats')).quotes,before.quotes);
    const research=await invoke(a,'stock.research',{symbol:'600519',market:'CN.SH'});
    assert.equal(research.schema_version,1);assert.equal(research.supported,true);assert.equal(Object.keys(research.datasets).length,8);
    assert.equal(research.datasets.intraday.status,'missing');assert.deepEqual(research.datasets.announcements.rows,[]);
  });
  await check('stream saturation leaves health and normal queries available',async()=>{
    const statuses=await Promise.all(Array.from({length:64},async()=>{const c=new AbortController();controllers.push(c);const r=await fetch(a+'/v1/events',{signal:c.signal});return r.status;}));
    assert.equal(statuses.filter(s=>s===200).length,16);assert.equal(statuses.filter(s=>s===503).length,48);
    assert.equal((await api(a,'/v1/health')).listening,true);assert.ok((await api(a,'/v1/quotes?board=ALL')).rows.length>0);
    for(const c of controllers)c.abort();
  });
  await check('two processes preserve concurrent watch additions',async()=>{
    const b=await server();await Promise.all([invoke(a,'watch.set',{symbol:'600080',market:'CN.SH',watched:true}),invoke(b,'watch.set',{symbol:'600081',market:'CN.SH',watched:true})]);
    for(const base of [a,b]){const settings=await api(base,'/v1/settings');assert.ok(settings.watchlist.some(w=>w.symbol==='600080'));assert.ok(settings.watchlist.some(w=>w.symbol==='600081'));}
    const ah=await api(a,'/v1/health'),bh=await api(b,'/v1/health');assert.notEqual(ah.instance_id,bh.instance_id);assert.equal(ah.data_root,bh.data_root);
  });
  await check('6001 instruments can be traversed without duplicate or missing rows',async()=>{
    const listings=Array.from({length:6001},(_,i)=>({symbol:String(600000+i),market:'CN.SH',industry:'Acceptance'}));
    await invoke(a,'import.json',{data:{listings}});const all=new Set();let total;
    for(let offset=0;;offset+=200){const p=await api(a,'/v1/quotes?board=CN&industry=Acceptance&limit=200&offset='+offset+'&sort=last');total=p.matched;for(const row of p.rows){const key=row.market+':'+row.symbol;assert.ok(!all.has(key));all.add(key);}if(offset+p.count>=p.matched)break;}
    assert.equal(total,6001);assert.equal(all.size,6001);
  });
  await check('market map exposes the complete 6001-stock universe with auditable weights',async()=>{
    const tradeDate=new Date().toISOString().slice(0,10);
    const quotes=Array.from({length:6001},(_,i)=>({symbol:String(600000+i),market:'CN.SH',ts:tradeDate,last:10+i%5,prev_close:10,turnover:(i+1)*10000,source:'acceptance.daily'}));
    await invoke(a,'import.json',{data:{quotes}});
    const map=await api(a,'/v2/market-map?board=CN&industry=Acceptance&metric=equal&aspect=1.8');
    assert.equal(map.schema_version,1);
    assert.equal(map.coverage.displayed,6001);assert.equal(map.coverage.universe,6001);assert.equal(map.metric,'equal');assert.equal(map.trade_date,tradeDate);
    const keys=new Set(map.groups.flatMap(g=>g.tiles.map(t=>t.key)));assert.equal(keys.size,6001);
    assert.ok(map.groups.every(g=>g.tiles.every(t=>t.bounds.w>0&&t.bounds.h>0)));
    const cap=await api(a,'/v2/market-map?board=CN&industry=Acceptance&metric=float_market_cap');
    assert.equal(cap.coverage.displayed,0);assert.equal(cap.coverage.missing_weight,6001);
    const bad=await fetch(a+'/v2/market-map?board=ALL');assert.equal(bad.status,400);
  });
  await check('manual recompute publishes coherent input and snapshot versions',async()=>{
    const job=await invoke(a,'job.submit',{kind:'pulse.recompute',params:{}});assert.equal((await waitJob(a,job.id)).status,'succeeded');
    const snapshot=await api(a,'/v2/pulse');assert.ok(snapshot.snapshot_revision>0);assert.ok(snapshot.snapshot_version>0);assert.equal(typeof snapshot.config_version,'number');assert.ok(snapshot.calendar_version);assert.equal(snapshot.quality.universe_scope,'imported_sample');assert.equal(snapshot.pulse.data_quality.valid,snapshot.quality.quoted_valid);
    const next=await invoke(a,'job.submit',{kind:'pulse.recompute',params:{}});assert.equal((await waitJob(a,next.id)).status,'succeeded');assert.ok((await api(a,'/v2/pulse')).snapshot_revision>snapshot.snapshot_revision);
  });
  fs.writeFileSync(path.join(root,'results.json'),JSON.stringify({root,results},null,2));console.log(JSON.stringify({root,passed:results.length}));
})().catch(error=>{console.error(error);process.exitCode=1;}).finally(async()=>{for(const c of controllers)c.abort();for(const child of children)child.kill();await sleep(300);});
