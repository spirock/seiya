// Web onboarding with a synthetic DOM/HTTP adapter only. No sockets or models.
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm'), assert = require('node:assert/strict');
const root = path.join(__dirname, '../../crates/seiya-server/src');
const markup = fs.readFileSync(path.join(root, 'desktop_setup.html'), 'utf8');
const code = fs.readFileSync(path.join(root, 'desktop_setup.js'), 'utf8');
class Element {
  constructor(tag='input') { this.tag=tag; this.value=''; this.hidden=false; this.disabled=false; this.dataset={}; this.listeners={}; this.textContent=''; }
  addEventListener(event, fn) { this.listeners[event]=fn; }
  emit(event) { return this.listeners[event]?.({preventDefault(){}}); }
  set innerHTML(_) { throw Error('unsafe HTML insertion'); }
}
const tick=()=>new Promise(r=>setImmediate(r));
async function fixture(available=true, stored=new Map()) {
  const els={}, picks=[], calls=[], nav=[]; let lost=false, phase='unconfigured', hold, release;
  for (const match of markup.matchAll(/<(\w+)[^>]*\bid="([^"]+)"[^>]*>/g)) els[match[2]]=new Element(match[1]);
  const setupIds=Object.keys(els);
  for (const match of markup.matchAll(/<button[^>]*data-pick="([^"]+)"[^>]*>/g)) { const e=new Element('button');e.dataset.pick=match[1];picks.push(e); }
  const get=id=>els[id]??=new Element();
  const buttons=()=>[...Object.values(els).filter(e=>e.tag==='button'),...picks];
  const draft={llm_binary:'/synthetic/engine',model_store:'/synthetic/models',image_binary:'',image_assets:'',roles:{brain:'gemma4:e2b-mlx',workers:[],image:''}};
  const storageCalls=[];
  const ctx={URL,JSON,Error,Object,Array,document:{getElementById:get,querySelectorAll(selector){
    if(selector==='[data-pick]')return picks;
    if(selector==='#setup button')return buttons();
    if(selector==='#setup input, #setup textarea')return setupIds.map(id=>els[id]).filter(e=>['input','textarea'].includes(e.tag));
    throw Error('unexpected selector '+selector);
  }},location:{href:'http://127.0.0.1:8015/trial/setup/',assign:url=>nav.push(url)},
  localStorage:{getItem(key){storageCalls.push(['get',key]);return stored.get(key)||null},setItem(key,v){storageCalls.push(['set',key]);stored.set(key,v)}},
  setInterval(fn){ctx.poll=fn},fetch:async(url,options)=>{
    assert.equal(url.origin,'http://127.0.0.1:8015');
    assert.equal(options.credentials,'same-origin');
    const name=url.pathname.split('/').at(-1), body=options.body?JSON.parse(options.body):null; calls.push({name,body});
    let value={};
    if(name==='detect')value={draft,baseline:[],warning:null};
    else if(name==='status')value={platform:available?'macos':'windows',runtime_available:available,phase,error:null};
    else if(name==='review')value={approval:'one-use',draft:body.draft,executable:available&&body.draft.roles.brain==='gemma4:e2b-mlx',effects:'filesystem only'};
    else if(name==='confirm'){if(hold)await new Promise(r=>release=r);if(lost){lost=false;throw Error('response lost')}value={saved:true};phase=body.start?'starting':'saved';}
    else if(name==='pick')value={path:'/synthetic/chosen'};
    else if(name==='enter')value={url:'http://127.0.0.1:8015/session/'+ 'a'.repeat(64)};
    else if(name==='stop')phase='stopping';
    else if(name!=='quit')throw Error('unexpected endpoint '+name);
    return{ok:true,status:name==='quit'?202:200,json:async()=>value};
  }};
  vm.createContext(ctx);vm.runInContext(code,ctx);await tick();
  return{get,picks,calls,nav,stored,storageCalls,ctx,lose(){lost=true},hold(){hold=true},release(){hold=false;release()}};
}
(async()=>{
  const f=await fixture();
  assert.ok(f.calls.every(v=>v.body===null));
  assert.equal(f.get('prompt').disabled,true);assert.equal(f.get('send').disabled,true);
  assert.ok(!f.storageCalls.some(([,key])=>key.includes('chats')));
  f.get('setupWorkers').value='worker-a\nworker-b';
  await f.get('setupSaveRoles').emit('click');await tick();
  assert.deepEqual(JSON.parse(f.stored.get('seiya.desktop.roles.v1')).workers,['worker-a','worker-b']);
  assert.ok(!f.stored.get('seiya.desktop.roles.v1').includes('/synthetic'));
  f.get('setupWorkers').value='';
  f.get('setupForm').emit('submit');await tick();
  assert.equal(f.get('setupApproval').hidden,false);assert.equal(f.get('setupStart').disabled,false);
  f.get('setupForm').emit('input');assert.equal(f.get('setupApproval').hidden,true);
  const before=f.calls.length;await f.get('setupSave').emit('click');await tick();assert.equal(f.calls.length,before);
  f.get('setupBrain').value='<img src=x onerror=alert(1)>';
  f.get('setupForm').emit('submit');await tick();
  assert.equal(f.get('setupStart').disabled,true);assert.match(f.get('setupSummary').textContent,/<img/);
  await f.get('setupSave').emit('click');await tick();
  assert.deepEqual(f.calls.filter(c=>c.name==='confirm').at(-1).body,{approval:'one-use',start:false});
  f.get('setupBrain').value='gemma4:e2b-mlx';f.get('setupForm').emit('submit');await tick();
  f.hold();const saving=f.get('setupStart').emit('click');await tick();
  assert.equal(f.get('setupBrain').disabled,true);
  const during=f.calls.filter(c=>c.name==='confirm').length;
  await f.get('setupStart').emit('click');assert.equal(f.calls.filter(c=>c.name==='confirm').length,during);
  f.release();await saving;assert.equal(f.get('setupBrain').disabled,false);
  f.get('setupForm').emit('submit');await tick();f.lose();
  const count=f.calls.filter(c=>c.name==='confirm').length;
  await f.get('setupStart').emit('click');await tick();await f.ctx.poll();
  assert.equal(f.calls.filter(c=>c.name==='confirm').length,count+1);
  assert.equal(f.get('setupApproval').hidden,true);
  await f.picks[0].emit('click');assert.equal(f.get('setupLlm').value,'/synthetic/chosen');
  const windows=await fixture(false,f.stored);
  assert.equal(windows.get('setupReview').disabled,true);
  assert.equal(windows.get('setupWorkers').value,'worker-a\nworker-b');
  assert.equal(windows.calls.filter(c=>c.name==='confirm').length,0);
  console.log('Desktop web setup: inert launch, role drafts, immutable review, unsupported activation, safe text, in-flight exclusion, no POST replay and Windows capability limits OK (DOM/HTTP mocks)');
})().catch(e=>{console.error(e);process.exitCode=1});
