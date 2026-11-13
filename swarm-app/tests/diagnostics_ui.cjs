// Synthetic DOM/configuration only. Never opens a socket or invokes a model.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
class Element{constructor(){this.value='';this.children=[];this.attrs={};this.text='';}append(...v){this.children.push(...v);}setAttribute(k,v){this.attrs[k]=v;}set textContent(v){this.text=String(v);this.children=[];}get textContent(){return this.text+this.children.map(c=>c.textContent||'').join('');}set innerHTML(_){throw Error('unsafe HTML');}}
const els={},get=id=>els[id]??=new Element(),calls=[];let confirmed=true,revision=1,hold=false,release,lose=false;
let limits={version:1,chat_warning_ms:10000,chat_swap_growth_mib:2048,chat_deadline_seconds:120,image_swap_growth_mib:512,image_deadline_seconds:600,session_seconds:1800};
let features={images:false,multiagent:false};
function status(){return{version:1,revision,limits:{...limits},features:{...features},feature_controls:true,installed_features:{images:true,multiagent:true},effective_features:{images:false,multiagent:false},observation:{pressure:'critical',swap_bytes:1899954176,observed_at_unix_ms:Date.now()},last_stop:{source:'previous_controlled_run',reason:'critical',elapsed_ms:3006},runtime_available:false,no_inference_profile:true,storage_failed:false};}
get('checkRuntime').onclick=async()=>{};
const ctx={$:get,document:{createElement:()=>new Element()},Date,confirm:()=>confirmed,refreshRuntimeStatus:async()=>{},
 seiyaControlFetch:fn=>fn(undefined),seiyaHttpError:(r,d)=>Error(d.error.message),fetch:async(url,options)=>{
  assert.equal(url,'/api/diagnostics/v1');const frame=JSON.parse(options.body);calls.push(frame);
  if(frame.operation==='configure'||frame.operation==='features'){
   if(frame.revision!==revision)return{ok:false,status:409,json:async()=>({error:{message:'configuration changed; refresh before editing'}})};
   if(hold)await new Promise(r=>release=r);if(frame.operation==='configure')limits={...frame.limits};else features={...frame.features};revision++;if(lose){lose=false;throw Error('response lost; state uncertain');}
  }
  return{ok:true,json:async()=>status()};
 }};
vm.createContext(ctx);vm.runInContext(fs.readFileSync(path.join(__dirname,'../../crates/seiya-server/src/diagnostics_ui.js'),'utf8'),ctx);
const run=s=>vm.runInContext(s,ctx),tick=()=>new Promise(r=>setImmediate(r));
(async()=>{
 await tick();assert.match(run('seiyaDiagnosticsStatus.textContent'),/CRÍTICA/);assert.match(run('seiyaDiagnosticsStatus.textContent'),/3006 ms/);assert.match(run('seiyaDiagnosticsStatus.textContent'),/ensayo anterior/);assert.match(run('seiyaDiagnosticsStatus.textContent'),/desactivados/);
 const warning=run("seiyaDiagnosticsInputs.get('chat_warning_ms')");warning.value='500';warning.oninput();await run('seiyaDiagnosticsSave.onclick()');assert.equal(limits.chat_warning_ms,500);assert.equal(revision,2);assert.match(run('seiyaDiagnosticsStatus.textContent'),/no se activó inferencia/);
 const before=calls.length;warning.value='10001';warning.oninput();await run('seiyaDiagnosticsSave.onclick()');assert.equal(calls.length,before);assert.match(run('seiyaDiagnosticsStatus.textContent'),/inválido/);
 warning.value='600';warning.oninput();revision++;await run('seiyaDiagnosticsSave.onclick()');assert.equal(warning.value,'600');assert.equal(run('seiyaDiagnosticsSave.disabled'),true);assert.match(run('seiyaDiagnosticsStatus.textContent'),/No se reenviará/);
 confirmed=false;const prior=calls.length;await ctx.seiyaRefreshDiagnostics();assert.equal(calls.length,prior);confirmed=true;await ctx.seiyaRefreshDiagnostics();assert.equal(run('seiyaDiagnosticsRevision'),revision);assert.equal(warning.value,'500');
 hold=true;warning.value='700';warning.oninput();const saving=run('seiyaDiagnosticsSave.onclick()');await tick();assert.equal(warning.disabled,true);const during=calls.length;await run('seiyaDiagnosticsSave.onclick()');assert.equal(calls.length,during);release();hold=false;await saving;assert.equal(limits.chat_warning_ms,700);assert.equal(warning.disabled,false);
 lose=true;warning.value='800';warning.oninput();const lost=calls.length;await run('seiyaDiagnosticsSave.onclick()');await tick();assert.equal(calls.length,lost+1);assert.match(run('seiyaDiagnosticsStatus.textContent'),/No se reenviará/);await ctx.seiyaRefreshDiagnostics();assert.equal(warning.value,'800');assert.equal(run('seiyaDiagnosticsRevision'),revision);
 run("seiyaFeatureInputs.get('images').checked=true;seiyaFeatureInputs.get('multiagent').checked=true");await run('seiyaFeaturesSave.onclick()');assert.deepEqual(features,{images:true,multiagent:true});assert.match(run('seiyaFeaturesStatus.textContent'),/solo arrancan al solicitar trabajo/);assert.ok(calls.every(f=>['status','configure','features'].includes(f.operation)));
 console.log('Settings UI: image/multiagent switches, bounded edits, CAS conflict, dirty refresh confirmation, disabled in-flight controls and uncertain save/no replay OK (mocks; no inference)');
})().catch(e=>{console.error(e);process.exitCode=1;});
