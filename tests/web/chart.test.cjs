const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
async function fixture(){
  const inputs=[5,10,20,30,120,250].map(p=>({dataset:{maPeriod:String(p)},checked:true}));
  const outputs=inputs.map(input=>({dataset:{maValue:input.dataset.maPeriod},textContent:''}));
  const controls={innerHTML:'',querySelectorAll:s=>s==='[data-ma-period]'?inputs:outputs};
  const styles=[],labels=[],rects=[],callbacks=[];
  const ctx=new Proxy({},{get:(_,p)=>p==='fillText'?(text,x,y)=>labels.push({text,x,y}):p==='fillRect'?(...r)=>rects.push(r):()=>{},set:(_,p,v)=>{if(p==='strokeStyle')styles.push(v);return true;}});
  const canvas={clientWidth:1000,clientHeight:460,getContext:()=>ctx,addEventListener(){},setAttribute(){}};
  const context=vm.createContext({console,window:{devicePixelRatio:1},document:{documentElement:{}},ResizeObserver:class{observe(){}},getComputedStyle:()=>({getPropertyValue:key=>({'--muted':'#888','--line':'#333','--up':'#f00','--down':'#0f0'})[key]})});
  const module=new vm.SourceTextModule(fs.readFileSync(path.join(__dirname,'../../crates/getrich-api/web/chart.js'),'utf8'),{context});await module.link(()=>{});await module.evaluate();
  const chart=new module.namespace.Chart(canvas,{textContent:''},{controls,onToggle:p=>callbacks.push([...p])});
  const rows=Array.from({length:300},(_,i)=>({ts:'2024-'+String(i+1).padStart(6,'0'),open:i+1,high:i+2,low:i+.5,close:i+1,volume:100}));
  chart.setData(rows,{reset:true});return {chart,controls,inputs,outputs,styles,labels,rects,callbacks,rows};
}
test('zoom keeps growing candle bodies and fills the plot below forty and eighty dates',async()=>{
  const f=await fixture();let previous=0;
  for(const span of [120,80,40,20,10,5,2,1]){f.chart.span=span;f.chart.draw();assert.equal(f.chart.grid.padding,0);assert.equal(f.chart.grid.count,span);assert.ok(f.chart.grid.candle>previous);previous=f.chart.grid.candle;}
  f.chart.span=2;f.chart.zoom(.8);assert.equal(f.chart.span,1);
});
test('six averages show selected-day values and each checkbox controls visibility',async()=>{
  const f=await fixture();assert.equal(f.outputs.find(o=>o.dataset.maValue==='250').textContent,'175.50');
  f.chart.hover=0;f.chart.draw();assert.equal(f.outputs.find(o=>o.dataset.maValue==='250').textContent,'—');
  for(const input of f.inputs){input.checked=false;f.controls.onchange({target:input});}
  assert.equal(f.chart.enabled.size,0);assert.deepEqual(f.callbacks.at(-1),[]);
  const input=f.inputs.find(i=>i.dataset.maPeriod==='120');input.checked=true;f.controls.onchange({target:input});assert.deepEqual(f.callbacks.at(-1),[120]);
});
test('disabled averages cannot distort the price axis',async()=>{
  const f=await fixture();f.chart.span=10;f.chart.draw();f.labels.length=0;f.chart.draw();
  const low=()=>Math.min(...f.labels.filter(l=>l.x===54&&!Number.isNaN(Number(l.text))).map(l=>Number(l.text)));
  assert.ok(low()<180);f.chart.enabled.clear();f.labels.length=0;f.chart.draw();assert.ok(low()>280);
});
test('weekly charts consume daily-aligned values and never substitute week-based SMA',async()=>{
  const f=await fixture();f.chart.setData(f.rows.slice(-3),{timeframe:'1w',reset:true,movingAverages:[{period:250,values:[100,101,102]}],periods:[250]});
  assert.equal(f.outputs.find(o=>o.dataset.maValue==='250').textContent,'102.00');
  f.chart.setData(f.rows.slice(-3),{timeframe:'1w',reset:true});assert.equal(f.outputs.find(o=>o.dataset.maValue==='250').textContent,'—');
});
