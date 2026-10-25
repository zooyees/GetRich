export function sma(values,n){let sum=0;return values.map((v,i)=>{sum+=v;if(i>=n)sum-=values[i-n];return i+1>=n?sum/n:null;});}
export function range(length,span,end=length){if(!length)return {start:0,end:0,span:0};span=Math.max(1,Math.min(length,Math.round(span||120)));end=Math.max(span,Math.min(length,end));return {start:end-span,end,span};}
export function hitIndex(x,left,slot,padding,count){const i=Math.floor((x-left)/slot-padding);return i<0||i>=count?null:i;}
export class Chart{
  constructor(canvas,meta){
    this.canvas=canvas;this.meta=meta;this.rows=[];this.span=120;this.end=0;this.hover=null;this.loading=false;
    this.observer=new ResizeObserver(()=>this.draw());this.observer.observe(canvas);
    canvas.addEventListener("mousemove",event=>{if(!this.grid)return;const rect=canvas.getBoundingClientRect();this.hover=hitIndex(event.clientX-rect.left,this.grid.left,this.grid.slot,this.grid.padding,this.grid.count);this.draw();});
    canvas.addEventListener("mouseleave",()=>{this.hover=null;this.draw();});
    canvas.addEventListener("wheel",event=>{if(!event.ctrlKey&&!event.metaKey)return;event.preventDefault();this.zoom(event.deltaY<0?.8:1.25);},{passive:false});
    canvas.addEventListener("keydown",event=>{
      if(["ArrowLeft","ArrowRight","+","-","Home","End"].includes(event.key))event.preventDefault();
      if(event.key==="ArrowLeft")this.move(-Math.max(1,Math.round(this.span/4)));
      if(event.key==="ArrowRight")this.move(Math.max(1,Math.round(this.span/4)));
      if(event.key==="+")this.zoom(.8);if(event.key==="-")this.zoom(1.25);
      if(event.key==="End")this.reset();if(event.key==="Home"){this.end=Math.min(this.span,this.rows.length);this.draw();}
    });
  }
  setData(rows,{reset=false,loading=false,message="暂无该周期 K 线"}={}){
    const atEnd=this.end===this.rows.length;this.rows=rows||[];this.message=message;this.loading=loading;
    if(reset){this.span=Math.min(120,this.rows.length);this.end=this.rows.length;this.hover=null;}else if(atEnd||!this.end)this.end=this.rows.length;
    this.ma=[sma(this.rows.map(r=>r.close),5),sma(this.rows.map(r=>r.close),10),sma(this.rows.map(r=>r.close),20)];this.draw();
  }
  zoom(factor){if(!this.rows.length)return;this.span=Math.max(1,Math.min(this.rows.length,Math.round(this.span*factor)));this.draw();}
  move(delta){this.end=Math.max(this.span,Math.min(this.rows.length,this.end+delta));this.hover=null;this.draw();}
  reset(){this.span=Math.min(120,this.rows.length);this.end=this.rows.length;this.hover=null;this.draw();}
  draw(){
    const canvas=this.canvas,ctx=canvas.getContext("2d"),w=canvas.clientWidth,h=canvas.clientHeight;if(!w||!h)return;
    const dpr=window.devicePixelRatio||1;canvas.width=w*dpr;canvas.height=h*dpr;ctx.setTransform(dpr,0,0,dpr,0,0);ctx.clearRect(0,0,w,h);
    const css=getComputedStyle(document.documentElement),muted=css.getPropertyValue("--muted").trim(),line=css.getPropertyValue("--line").trim();
    const up=css.getPropertyValue("--up").trim(),down=css.getPropertyValue("--down").trim();ctx.font="11px system-ui";ctx.fillStyle=muted;
    if(this.loading||!this.rows.length){this.grid=null;ctx.textAlign="center";ctx.fillText(this.loading?"正在载入 K 线…":this.message,w/2,h/2);this.meta.textContent=this.loading?"加载中":this.message;return;}
    const visible=range(this.rows.length,this.span,this.end);this.span=visible.span;this.end=visible.end;
    const rows=this.rows.slice(visible.start,visible.end);const ma=this.ma.map(a=>a.slice(visible.start,visible.end));
    let min=Math.min(...rows.map(r=>r.low)),max=Math.max(...rows.map(r=>r.high));
    for(const a of ma)for(const value of a)if(value!=null){min=Math.min(min,value);max=Math.max(max,value);}
    const margin=Math.max((max-min)*.06,Math.abs(max)*.001,.01);min-=margin;max+=margin;
    const left=62,right=w-18,top=20,priceBottom=h*.70,volumeBottom=h-32,volumeTop=priceBottom+20;
    const slots=Math.max(40,rows.length),padding=slots-rows.length,slot=(right-left)/slots;
    const x=i=>left+(padding+i+.5)*slot,y=p=>top+(max-p)/(max-min)*(priceBottom-top);
    ctx.strokeStyle=line;ctx.lineWidth=1;ctx.textAlign="right";
    for(let i=0;i<=4;i++){const price=min+(max-min)*i/4,py=y(price);ctx.fillText(price.toFixed(2),left-8,py+4);ctx.beginPath();ctx.moveTo(left,py);ctx.lineTo(right,py);ctx.stroke();}
    const volumeMax=Math.max(1,...rows.map(r=>r.volume||0));
    ctx.textAlign="right";ctx.fillText("量 "+volumeMax.toLocaleString("zh-CN",{notation:"compact"}),left-8,volumeTop+9);
    rows.forEach((row,i)=>{const color=row.close>=row.open?up:down,cx=x(i),width=Math.max(1,Math.min(9,slot*.65));ctx.strokeStyle=color;ctx.fillStyle=color;ctx.beginPath();ctx.moveTo(cx,y(row.high));ctx.lineTo(cx,y(row.low));ctx.stroke();ctx.fillRect(cx-width/2,y(Math.max(row.open,row.close)),width,Math.max(1,y(Math.min(row.open,row.close))-y(Math.max(row.open,row.close))));const vh=(row.volume||0)/volumeMax*(volumeBottom-volumeTop);ctx.globalAlpha=.45;ctx.fillRect(cx-width/2,volumeBottom-vh,width,vh);ctx.globalAlpha=1;});
    ma.forEach((a,idx)=>{ctx.strokeStyle=[muted,"#3b82f6","#d97706"][idx];ctx.beginPath();let started=false;a.forEach((value,i)=>{if(value==null)return;if(!started){ctx.moveTo(x(i),y(value));started=true;}else ctx.lineTo(x(i),y(value));});ctx.stroke();});
    ctx.fillStyle=muted;ctx.textAlign="center";for(const i of [...new Set([0,Math.floor(rows.length/2),rows.length-1])])ctx.fillText(rows[i].ts.slice(0,10),Math.max(left+42,Math.min(right-42,x(i))),h-12);
    const hi=this.hover!=null&&rows[this.hover]?this.hover:rows.length-1,row=rows[hi];
    ctx.strokeStyle=muted;ctx.setLineDash([3,3]);ctx.beginPath();ctx.moveTo(x(hi),top);ctx.lineTo(x(hi),volumeBottom);ctx.stroke();ctx.setLineDash([]);
    this.meta.textContent=`${row.ts.slice(0,10)}  开 ${row.open.toFixed(2)}  高 ${row.high.toFixed(2)}  低 ${row.low.toFixed(2)}  收 ${row.close.toFixed(2)}  · 未复权`;
    canvas.setAttribute("aria-label",this.meta.textContent+"；方向键平移，加减键缩放，End 回到最新");
    this.grid={left,slot,padding,count:rows.length};
  }
}
