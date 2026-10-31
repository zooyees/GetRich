export function tailFrom(timeframe,last){
  const value=String(last||'').slice(0,10),date=new Date(value+'T00:00:00Z');
  if(!/^\d{4}-\d{2}-\d{2}$/.test(value)||!Number.isFinite(date.getTime()))return null;
  if(timeframe==='1w')date.setUTCDate(date.getUTCDate()-(date.getUTCDay()+6)%7);
  else if(timeframe==='1mo')date.setUTCDate(1);
  else if(timeframe!=='1d')return null;
  return date.toISOString().slice(0,10);
}
export function mergeTail(previous,rows,tail,from){
  if(!tail?.rows?.length||(tail.data_version??0)<(previous.data_version??0))return {meta:previous,rows};
  const prefix=rows.filter(b=>b.ts<from),next=[...prefix,...tail.rows];
  const moving_averages=(previous.moving_averages||[]).map(ma=>({period:ma.period,values:[...ma.values.slice(0,prefix.length),...(tail.moving_averages?.find(m=>m.period===ma.period)?.values||tail.rows.map(()=>null))]}));
  return {rows:next,meta:{...previous,...tail,rows:next,moving_averages,count:next.length,from:next[0]?.ts,to:next.at(-1)?.ts,empty:false}};
}
