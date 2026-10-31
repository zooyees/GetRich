// Isolated browser audit: serves repository assets and a captured public market snapshot.
// NODE_PATH must provide Playwright; no real provider or user browser profile is used.
const fs=require('node:fs'),path=require('node:path'),http=require('node:http'),assert=require('node:assert/strict');
const {chromium}=require('playwright');
const repo=path.resolve(__dirname,'../..'),assets=path.join(repo,'crates/getrich-api/web');
const fixture=JSON.parse(fs.readFileSync(process.argv[2]||path.join(repo,'target/refresh-audit/map.json'),'utf8'));
let delay=0,fail=false,requests=0;
const bootstrap=`import {MarketMap} from '/market-map.js';window.map=new MarketMap({isActive:()=>true,openStock:()=>{}});await window.map.refresh();window.ready=true;`;
const server=http.createServer(async(req,res)=>{
  res.setHeader('Cache-Control','no-store');
  if(req.url.startsWith('/v2/market-map')){requests++;await new Promise(r=>setTimeout(r,delay));res.writeHead(fail?503:200,{'Content-Type':'application/json'});res.end(JSON.stringify(fail?{ok:false,error:{message:'simulated outage'}}:fixture));return;}
  if(req.url==='/v1/settings'){res.writeHead(200,{'Content-Type':'application/json'});res.end('{"provider":{"configured":false}}');return;}
  const name=req.url==='/'?'index.html':req.url.slice(1);
  if(!/^[\w.-]+$/.test(name)){res.writeHead(404);res.end();return;}
  try{res.setHeader('Content-Type',name.endsWith('.js')?'text/javascript':name.endsWith('.css')?'text/css':'text/html');res.end(name==='app.js'?bootstrap:fs.readFileSync(path.join(assets,name)));}catch{res.writeHead(404);res.end();}
});
const listen=()=>new Promise(r=>server.listen(0,'127.0.0.1',r));
const stats=values=>{const a=[...values].sort((a,b)=>a-b);return {samples:a.length,p50_ms:+a[Math.floor(a.length*.5)].toFixed(2),p95_ms:+a[Math.min(a.length-1,Math.floor(a.length*.95))].toFixed(2),max_ms:+a.at(-1).toFixed(2)};};
(async()=>{
  await listen();const browser=await chromium.launch({headless:true,channel:process.env.GETRICH_BROWSER_CHANNEL||'chrome'});
  try{
    const page=await browser.newPage({viewport:{width:1920,height:1080},deviceScaleFactor:1.5});
    await page.goto('http://127.0.0.1:'+server.address().port);await page.waitForFunction(()=>window.ready);
    await page.evaluate(()=>{document.getElementById('workspace').hidden=true;document.getElementById('view-pulse').hidden=false;});
    await page.waitForTimeout(400);await page.evaluate(async()=>{if(window.map.pendingRead)await window.map.pendingRead;});
    const paint=await page.evaluate(()=>{const times=[];window.map.draw();for(let i=0;i<60;i++){const t=performance.now();window.map.draw();times.push(performance.now()-t);}return times;});
    const interaction=await page.evaluate(async()=>{
      const map=window.map,canvas=map.canvas,rect=canvas.getBoundingClientRect();let calls=0;const original=map.draw.bind(map);map.draw=()=>{calls++;return original();};
      const start=performance.now();for(let i=0;i<120;i++)canvas.dispatchEvent(new PointerEvent('pointermove',{clientX:rect.left+10+i*7%rect.width,clientY:rect.top+10+(i*19)%(rect.height-20)}));
      const handler_ms=performance.now()-start;await new Promise(requestAnimationFrame);await new Promise(requestAnimationFrame);map.draw=original;
      return {events:120,paint_calls:calls,handler_ms:+handler_ms.toFixed(2)};
    });
    await page.evaluate(async()=>{window.map.observer.disconnect();clearTimeout(window.map.resizeTimer);if(window.map.pendingRead)await window.map.pendingRead;});
    delay=750;const before=requests;
    const continuity=await page.evaluate(async()=>{const m=window.map,old=m.data,key=m.tiles()[0].key;m.selected=key;m.view={scale:2,x:-100,y:-100};const request=m.refresh();const same=Array.from({length:30},()=>m.refresh());const retained=m.data===old&&m.hits.length>0;await Promise.all([request,...same]);return {retained,selected:m.selected===key,zoom:m.view.scale,tiles:m.tiles().length};});
    const coalesced_requests=requests-before;
    assert.equal(coalesced_requests,1,JSON.stringify(await page.evaluate(()=>({pending:!!window.map.pendingRead,generation:window.map.generation,requestKey:window.map.requestKey}))));assert.equal(continuity.retained,true);assert.equal(continuity.selected,true);assert.equal(continuity.zoom,2);
    fail=true;await page.evaluate(()=>window.map.refresh());assert.equal(await page.evaluate(()=>window.map.tiles().length),continuity.tiles);
    assert.match(await page.locator('#map-error-text').textContent(),/simulated outage/);
    fail=false;await page.evaluate(()=>window.map.refresh());assert.equal(await page.locator('#map-error').isHidden(),true);
    delay=20;
    for(let i=0;i<12;i++){
      fixture.data_version=100+i;fixture.historical=false;fixture.quote_mode='realtime';
      await page.evaluate(()=>window.map.refresh());
      assert.equal(await page.evaluate(()=>window.map.data.data_version),100+i);
      assert.equal(await page.evaluate(()=>window.map.tiles().length),continuity.tiles);
    }
    const result={viewport:'1920x1080',dpr:1.5,tiles:continuity.tiles,paint:stats(paint),interaction,slow_response_ms:750,coalesced_requests,continuity,outage_retains_data:true,recovery:true,successive_versions:12,scope:'headless browser CPU timings; not desktop GPU FPS'};
    const output=process.env.GETRICH_PERF_OUTPUT||path.join(repo,'target/refresh-audit/browser.json');fs.writeFileSync(output,JSON.stringify(result,null,2));console.log(JSON.stringify(result));
  }finally{await browser.close();server.close();server.closeAllConnections();}
})().catch(e=>{console.error(e);server.close();server.closeAllConnections();process.exitCode=1;});
