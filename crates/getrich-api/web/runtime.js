export const $ = id => document.getElementById(id);
export const esc = value => String(value ?? "").replace(/[&<>"']/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c]));
export const fmt = (value,digits=2) => value == null || !Number.isFinite(Number(value)) ? "—" : Number(value).toLocaleString("zh-CN",{minimumFractionDigits:digits,maximumFractionDigits:digits});
export const signed = value => value == null ? "—" : `${Number(value)>0?"+":""}${fmt(value)}`;
export const pct = value => value == null ? "—" : `${signed(value)}%`;
export const amount = (value,currency="") => value == null ? "—" : `${Math.abs(value)>=1e8?fmt(value/1e8)+"亿":Math.abs(value)>=1e4?fmt(value/1e4)+"万":fmt(value,0)}${currency?" "+currency:""}`;
export const direction = value => Number(value)>0?"up":Number(value)<0?"down":"flat";
export function time(value,market="CN.SH"){
  if(!value)return "—";
  const zone=market==="US"?"America/New_York":market==="HK"?"Asia/Hong_Kong":"Asia/Shanghai";
  const date=new Date(value);
  if(!Number.isFinite(date.getTime()))return String(value);
  return new Intl.DateTimeFormat("zh-CN",{timeZone:zone,year:"numeric",month:"2-digit",day:"2-digit",hour:"2-digit",minute:"2-digit",second:"2-digit",hour12:false}).format(date)+` · ${market==="US"?"纽约":market==="HK"?"香港":"上海"}`;
}
export function toast(message,error=false){
  const box=$("notice");box.textContent=message;box.classList.toggle("error",error);box.hidden=false;
  clearTimeout(toast.timer);toast.timer=setTimeout(()=>box.hidden=true,8000);
}
export async function api(url,options={}){
  const controller=new AbortController();
  const abort=()=>controller.abort(); options.signal?.addEventListener("abort",abort,{once:true});
  if(options.signal?.aborted)abort();
  const timer=setTimeout(abort,options.timeout||20000);
  try{
    const response=await fetch(url,{...options,signal:controller.signal});
    const body=await response.json();
    if(!response.ok||body.ok===false){const error=new Error(body.error?.message||`请求失败（${response.status}）`);error.code=body.error?.code;error.status=response.status;throw error;}
    return body.ok===true?body.data:body;
  }catch(error){if(error.name==="AbortError"&&!options.signal?.aborted)throw new Error("请求超时，后台任务可在数据页查询");throw error;}
  finally{clearTimeout(timer);options.signal?.removeEventListener("abort",abort);}
}
export const invoke=(method,params={})=>api("/v1/invoke",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({method,params})});
export class Reads{
  constructor(){this.requests=new Map();}
  async get(key,url){this.requests.get(key)?.abort();const ctrl=new AbortController();this.requests.set(key,ctrl);try{return await api(url,{signal:ctrl.signal});}finally{if(this.requests.get(key)===ctrl)this.requests.delete(key);}}
  cancel(){for(const ctrl of this.requests.values())ctrl.abort();this.requests.clear();}
}
export async function busy(button,operation){
  if(button?.disabled)return;
  if(button){button.disabled=true;button.setAttribute("aria-busy","true");}
  try{return await operation();}catch(error){toast(error.message,true);throw error;}
  finally{if(button){button.disabled=false;button.removeAttribute("aria-busy");}}
}
export const statusZh=status=>({queued:"等待执行",running:"执行中",cancelling:"取消中",cancelled:"已取消",succeeded:"已完成",partial:"部分完成",failed:"失败",interrupted:"执行中断",ok:"已完成"}[status]||status);
export function applyPreferences(prefs={}){
  document.documentElement.dataset.theme=prefs.theme||"dark";
  document.documentElement.dataset.colors=prefs.colors||"red_up";
  document.documentElement.style.setProperty("--list-width",`${Math.max(300,Math.min(720,prefs.list_width||440))}px`);
}
export class Events{
  constructor(callback){this.callback=callback;this.epoch="";this.seq=0;this.stopped=false;this.controller=null;}
  async start(){
    if(this.stopped)return;
    this.controller=new AbortController();
    try{
      const q=new URLSearchParams({since_seq:String(this.seq),stream_epoch:this.epoch});
      if(!this.epoch)q.delete("stream_epoch");
      const response=await fetch("/v1/events?"+q,{signal:this.controller.signal});
      if(!response.ok||!response.body)throw new Error("事件连接不可用");
      const reader=response.body.getReader();const decoder=new TextDecoder();let buffer="";
      while(!this.stopped){
        const {done,value}=await reader.read();if(done)break;
        buffer+=decoder.decode(value,{stream:true});
        let index;
        while((index=buffer.indexOf("\n"))>=0){
          const line=buffer.slice(0,index).trim();buffer=buffer.slice(index+1);if(!line)continue;
          let event;try{event=JSON.parse(line);}catch{continue;}
          if(event.type==="subscribed"){
            const changed=this.epoch&&this.epoch!==event.stream_epoch;this.epoch=event.stream_epoch;
            if(changed){this.seq=0;await this.callback({type:"resync_required"});}
            if(this.seq===0)this.seq=event.seq||0;
            continue;
          }
          if(event.type==="ping")continue;
          if(event.stream_epoch&&event.stream_epoch!==this.epoch){this.epoch=event.stream_epoch;this.seq=0;await this.callback({type:"resync_required"});}
          if(event.type==="resync_required"){this.seq=event.seq||0;await this.callback(event);continue;}
          if(Number(event.seq)<=this.seq)continue;
          await this.callback(event);this.seq=Number(event.seq)||this.seq;
        }
        if(buffer.length>1024*1024)throw new Error("事件数据过大");
      }
    }catch(error){if(this.stopped)return;}
    if(!this.stopped)this.timer=setTimeout(()=>this.start(),3000);
  }
  stop(){this.stopped=true;clearTimeout(this.timer);this.controller?.abort();}
}
