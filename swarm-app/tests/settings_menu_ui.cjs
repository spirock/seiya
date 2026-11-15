// Static settings scripts with synthetic DOM/APIs only; no model, frame or network.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const SRC='crates/seiya-server/src/';
class Element{
 constructor(tag='div'){this.tag=tag;this.children=[];this.attrs={};this.style={};this.listeners={};this.value='';this.hidden=false;this.dataset={};}
 append(...nodes){for(const n of nodes){n.remove();this.children.push(n);n.parentNode=this;}}
 prepend(node){node.remove();this.children.unshift(node);node.parentNode=this;}
 remove(){if(this.parentNode)this.parentNode.children=this.parentNode.children.filter(n=>n!==this);this.parentNode=null;}
 insertAdjacentElement(where,node){assert.ok(this.parentNode);node.remove();const i=this.parentNode.children.indexOf(this);this.parentNode.children.splice(i+(where==='afterend'?1:0),0,node);node.parentNode=this.parentNode;}
 setAttribute(k,v){this.attrs[k]=String(v);}getAttribute(k){return this.attrs[k];}focus(){this.focused=true;}
 set textContent(v){this.text=String(v);for(const c of [...this.children])c.remove();}get textContent(){return(this.text||'')+this.children.map(c=>c.textContent||'').join('');}
 set innerHTML(_){throw Error('unsafe HTML');}addEventListener(n,fn){(this.listeners[n]??=[]).push(fn);}
 async fire(type,extra={}){const e={type,preventDefault(){this.prevented=true;},...extra};for(const fn of this.listeners[type]||[])await fn(e);if(this['on'+type])await this['on'+type](e);return e;}
 dispatchEvent(event){this.fire(event.type,event);return true;}
}
const tick=async()=>{await new Promise(r=>setImmediate(r));await new Promise(r=>setImmediate(r));};
const home='http://127.0.0.1:8015/app/'+ 'b'.repeat(32)+'/';
function browser({base=home,diagnostics=true}={}){
 const elements={},get=id=>elements[id]??=new Element(),aside=get('aside'),scroll=get('scroll'),calls=[],timers=[],navigations=[];
 scroll.scrollTop=89;scroll.append(get('welcome'),get('history'),get('jobsPanel'),get('imagesPanel'));
 get('history').textContent='Existing conversation';get('prompt').value='Draft while navigating';get('composer').append(get('form'));get('form').append(get('prompt'));get('checkRuntime').onclick=async()=>{};
 const source=get('seiyaModelsSource');source.hidden=true;
 if(base){
  const tags=[...fs.readFileSync(SRC+'desktop_setup.html','utf8').matchAll(/<([a-z0-9]+)[^>]*\bid="([^"]+)"/g)];
  for(const [,tag,id]of tags){get(id).tag=tag;get(id).id=id;if(id!=='setup')get('setup').append(get(id));}
  source.append(get('setup'));get('setup').style.position='fixed';get('header').append(get('seiyaModelsSettingsLink'));
 }
 const limits={version:1,chat_warning_ms:10000,chat_swap_growth_mib:2048,chat_deadline_seconds:120,image_swap_growth_mib:512,image_deadline_seconds:600,session_seconds:1800};
 const context={$:get,busy:false,seiyaHistoryDeleting:false,URL,Date,Map,confirm:()=>true,
  document:{getElementById:id=>elements[id]||null,createElement:tag=>new Element(tag),querySelector:s=>s==='aside'?aside:null,querySelectorAll:selector=>{
   const all=[];function visit(n){for(const c of n?.children||[]){all.push(c);visit(c);}}visit(elements.setup);
   if(selector==='#setup button')return all.filter(n=>n.tag==='button');if(selector==='#setup input, #setup textarea')return all.filter(n=>['input','textarea'].includes(n.tag));if(selector==='[data-pick]')return[];return[];
  }},location:{href:'http://127.0.0.1:8015/app/'+ 'a'.repeat(32)+'/',assign:url=>navigations.push(url)},
  Event:class{constructor(type,options){this.type=type;Object.assign(this,options);}preventDefault(){}},MutationObserver:class{observe(){}},
  localStorage:{getItem:()=>null,setItem:()=>{throw Error('no storage writes while navigating');}},setInterval:fn=>{timers.push(fn);return timers.length;},refreshRuntimeStatus:async()=>{},openChat:id=>{context.lastChat=id;},
  seiyaControlFetch:fn=>fn(undefined),seiyaHttpError:(r,d)=>Error(d.error.message),fetch:async(url,options={})=>{
   const target=String(url),frame=options.body?JSON.parse(options.body):null;calls.push({target,frame,options});
   if(target==='/api/diagnostics/v1'){assert.equal(frame.operation,'status');return{ok:true,json:async()=>({version:1,revision:1,limits,features:{images:false,multiagent:false},feature_controls:true,installed_features:{images:true,multiagent:true},observation:{pressure:'normal',swap_bytes:0}})};}
   assert.ok(target.startsWith(home+'api/'),'only authenticated same-origin model APIs');assert.equal(options.credentials,'same-origin');assert.equal(options.redirect,'error');assert.equal(options.method,undefined,'navigation must not mutate configuration');
   const data=target.endsWith('/detect')?{baseline:[],draft:{llm_binary:'/synthetic/ollama',model_store:'/synthetic/store',image_binary:'',image_assets:'',roles:{brain:'gemma4:e2b-mlx',workers:[],image:''}}}:{platform:'macos',architecture:'arm64',runtime_available:true,phase:'running'};
   return{ok:true,json:async()=>data};
  }};
 vm.createContext(context);
 if(diagnostics)vm.runInContext(fs.readFileSync(SRC+'diagnostics_ui.js','utf8'),context);
 const setup=fs.readFileSync(SRC+'desktop_setup.js','utf8').replace(/trial\\\//g,'app\\/')
  .replace("new URL('api/' + path, location.href)","new URL('api/' + path, seiyaModelsBase.href)")
  .replace("const options = {credentials:'same-origin', cache:'no-store'};","const options = {credentials:'same-origin', cache:'no-store', redirect:'error'};")
  .replace("  el('prompt').disabled = true; el('send').disabled = true;",'')
  .replace("  el('form').addEventListener('submit', e => e.preventDefault());",'')
  .replace("  el('runtimeStatus').textContent = 'Configura Seiya antes de conversar.';",'')
  .replace("      el('runtimeStatus').textContent = 'Seiya: ' + v.phase;",'');
 vm.runInContext(`const seiyaModelsHome=${JSON.stringify(base)};let seiyaModelsInitialized=false;function seiyaInitializeModels(){if(seiyaModelsInitialized)return;if(!seiyaModelsBase)throw Error('no models');seiyaModelsInitialized=true;\n${setup}\n${fs.readFileSync(SRC+'desktop_setup_feedback.js','utf8')}\n}\n${fs.readFileSync(SRC+'settings_menu_ui.js','utf8')}`,context);
 return{context,get,calls,timers,navigations,run:s=>vm.runInContext(s,context)};
}
(async()=>{
 let b=browser();await tick();const initial=b.calls.length;assert.equal(b.run('seiyaSettingsMenu.hidden'),true);assert.equal(b.run('seiyaModelsInitialized'),false);assert.equal(b.timers.length,0);
 b.run('seiyaSettingsRoot.onclick()');assert.equal(b.run('seiyaSettingsMenu.hidden'),false);assert.equal(b.calls.length,initial);assert.equal(b.run('seiyaSettingsOptions.size'),2);
 b.run("seiyaSettingsOptions.get('general').onclick()");assert.equal(b.run('seiyaSettingsView'),'general');assert.equal(b.run('seiyaChatSurface.hidden'),true);assert.equal(b.get('composer').hidden,true);assert.equal(b.run('seiyaGeneralSettings.hidden'),false);
 b.run("seiyaDiagnosticsInputs.get('chat_warning_ms').value='3000';seiyaDiagnosticsInputs.get('chat_warning_ms').oninput()");
 b.run("seiyaSettingsOptions.get('models').onclick()");await tick();assert.equal(b.run('seiyaSettingsView'),'models');assert.equal(b.run('seiyaGeneralSettings.hidden'),true);assert.equal(b.get('setup').parentNode,b.get('scroll'));assert.equal(b.get('setup').style.position,'static');assert.equal(b.get('setupBrain').value,'gemma4:e2b-mlx');assert.equal(b.get('prompt').disabled,undefined);assert.equal(b.timers.length,1);assert.equal(b.navigations.length,0);
 const modelCalls=b.calls.length;b.get('setupBrain').value='edited role, not submitted';b.run("seiyaSettingsOptions.get('general').onclick();seiyaSettingsOptions.get('models').onclick()");await tick();assert.equal(b.calls.length,modelCalls);assert.equal(b.get('setupBrain').value,'edited role, not submitted');assert.equal(b.run("seiyaDiagnosticsInputs.get('chat_warning_ms').value"),'3000');assert.equal(b.run('seiyaDiagnosticsDirty'),true);
 b.run('seiyaSettingsBack.onclick()');assert.equal(b.get('composer').hidden,false);assert.equal(b.get('history').textContent,'Existing conversation');assert.equal(b.get('prompt').value,'Draft while navigating');assert.equal(b.get('scroll').scrollTop,89);
 const shortcut=await b.get('seiyaModelsSettingsLink').fire('click');assert.equal(shortcut.prevented,true);assert.equal(b.run('seiyaSettingsView'),'models');assert.equal(b.navigations.length,0);
 b.run("seiyaShowDiagnostics({version:1,revision:1,limits:Object.fromEntries([...seiyaDiagnosticsInputs].map(([k,v])=>[k,Number(v.value)])),observation:{pressure:'normal'},no_inference_profile:true})");assert.equal(b.run('seiyaGeneralSettings.hidden'),true);assert.equal(b.run('seiyaSettingsView'),'models');b.run('seiyaSettingsBack.onclick()');
 b.context.busy=true;b.run("seiyaSettingsOptions.get('general').onclick()");assert.equal(b.run('seiyaSettingsView'),'chat');b.context.busy=false;
 b.run("seiyaSettingsOptions.get('general').onclick()");await b.run("seiyaSettingsMenu.fire('keydown',{key:'Escape'})");assert.equal(b.run('seiyaSettingsView'),'chat');assert.equal(b.run('seiyaSettingsRoot.attrs[\"aria-expanded\"]'),'false');
 b.run("seiyaSettingsOptions.get('models').onclick();openChat('another-chat')");assert.equal(b.context.lastChat,'another-chat');assert.equal(b.run('seiyaSettingsView'),'chat');assert.equal(b.get('prompt').value,'Draft while navigating');assert.equal(b.navigations.length,0);
 assert.ok(b.calls.every(c=>c.frame?.operation==='status'||c.options.method===undefined));
 for(const invalid of ['https://external.invalid/','http://127.0.0.1:9999/app/'+ 'b'.repeat(32)+'/',home+'?key=secret','http://127.0.0.1:8015/session/'+ 'a'.repeat(64)]){b=browser({base:invalid});await tick();assert.equal(b.run('seiyaSettingsOptions.get("models").disabled'),true);b.run("seiyaOpenSettings('models')");assert.equal(b.run('seiyaSettingsView'),'chat');assert.equal(b.timers.length,0);assert.ok(b.calls.every(c=>c.target==='/api/diagnostics/v1'));}
 b=browser({base:null,diagnostics:false});b.run("seiyaSettingsRoot.onclick();seiyaSettingsOptions.get('general').onclick()");assert.match(b.run('seiyaGeneralSettings.textContent'),/no está disponible/);assert.equal(b.calls.length,0);
 b.run('seiyaSettingsBack.onclick()');b.get('composer').hidden=true;b.run("seiyaSettingsOptions.get('general').onclick();seiyaSettingsBack.onclick()");assert.equal(b.get('composer').hidden,true);
 console.log('Settings submenu: in-chat model/general views, lazy static setup, authenticated base, draft/history/dirty preservation, keyboard/busy guards and zero start/save/replay OK (mocks)');
})().catch(e=>{console.error(e);process.exitCode=1;});
