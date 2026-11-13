// Reload/reconnect against synthetic persistent brokers only. No models/network.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const {randomUUID}=require('node:crypto');
const stored=new Map(),calls=[],brokers={images:new Map(),jobs:new Map()};let lose=false,offline=false,confirmed=true,denied=false,corrupt=false,holdSubmit=false,releaseSubmit,holdArtifact=false,releaseArtifact,holdList=false,releaseList;
class Element{
 constructor(tag='div'){this.tag=tag;this.value='';this.children=[];this.attrs={};this.style={};this.listeners={};this.text='';}
 append(...v){this.children.push(...v);for(const c of v)if(c&&typeof c==='object'){c.parent=this;c.parentNode=this;}}
 replaceChildren(...v){this.children=[];this.text='';this.append(...v);}setAttribute(k,v){this.attrs[k]=v;}focus(){}
 set textContent(v){this.text=String(v);this.children=[];}get textContent(){return this.text+this.children.map(c=>c.textContent||'').join('');}
 set innerHTML(_){throw Error('unsafe HTML');}remove(){if(this.parent)this.parent.children=this.parent.children.filter(c=>c!==this);}
 addEventListener(n,fn){(this.listeners[n]??=[]).push(fn);}async fire(n){const e={preventDefault(){},stopImmediatePropagation(){this.stopped=true;}};for(const fn of this.listeners[n]||[]){await fn(e);if(e.stopped)return;}if(this['on'+n])await this['on'+n](e);}
 click(){if(this.tag==='a')return;return this.fire('click');}
}
const ok=v=>({ok:true,json:async()=>v});
function owner(kind,id){if(!brokers[kind].has(id))brokers[kind].set(id,{session:randomUUID().replaceAll('-','')+randomUUID().replaceAll('-',''),jobs:[]});return brokers[kind].get(id);}
async function fetch(route,options){
 calls.push({route,body:options?.body?JSON.parse(options.body):null});
 if(route==='/health')return ok({jobs:{enabled:true,history_enabled:true,volatile:false},images:{configured:true,available:true,volatile:false}});
 if(route==='/api/chat')return new Promise(()=>{}); // hung transport fixture only
 const kind=route==='/api/images/v1'?'images':route==='/api/jobs'?'jobs':null;assert.ok(kind,'unexpected route '+route);const frame=JSON.parse(options.body);
 if(denied)return{ok:false,status:401,json:async()=>({error:{message:'fixture expired authentication'}})};
 if(frame.operation==='open')return ok({session:owner(kind,frame.conversation_id).session,volatile:false});
 const o=[...brokers[kind].values()].find(o=>o.session===frame.session);assert.ok(o,'unknown capability');
 if(frame.operation==='list'){if(offline)throw Error('fixture offline');if(corrupt)return ok({jobs:[{job_id:randomUUID(),phase:'__proto__',artifact_available:false}]});const data={jobs:o.jobs.map(j=>({...j})),volatile:false};if(holdList&&kind==='images')await new Promise(r=>releaseList=r);return ok(data);}
 if(frame.operation==='submit'){assert.equal(kind,'images');let j=o.jobs.find(j=>j.request_id===frame.request_id);if(!j){j={job_id:randomUUID(),request_id:frame.request_id,phase:'completed',artifact_available:true};o.jobs.push(j);}if(lose){lose=false;throw Error('response lost');}if(holdSubmit)await new Promise(r=>releaseSubmit=r);return ok(j);}
 if(frame.operation==='forget'){assert.ok(!o.jobs.some(j=>['running','generating','quarantined'].includes(j.phase)));o.jobs=[];return ok({forgotten:true});}
 if(frame.operation==='cancel'){o.jobs.find(j=>j.job_id===frame.job_id).phase='cancelling';return ok({});}
 if(frame.operation==='artifact'){
  if(kind==='jobs')return ok({text:'synthetic draft, not executed'});
  const blob=new Blob(['fixture PNG transport only'],{type:'image/png'});if(holdArtifact)await new Promise(r=>releaseArtifact=r);return{ok:true,headers:{get:k=>k==='content-type'?'image/png':k==='content-length'?String(blob.size):null},blob:async()=>blob};
 }throw Error('unexpected operation');
}
let blob=0;const revoked=[];class LocalURL extends URL{static createObjectURL(){return 'blob:history-'+(++blob);}static revokeObjectURL(u){revoked.push(u);}}
function browser(){
 const elements={},get=id=>elements[id]??=new Element(),controls=new Element();controls.append(get('send'));get('form').append(controls);
 const context={document:{getElementById:get,createElement:t=>new Element(t),createTextNode:t=>{const e=new Element();e.textContent=t;return e;},querySelectorAll:()=>[]},crypto:{randomUUID},Date,URL:LocalURL,Blob,TextEncoder,AbortController,setTimeout,clearTimeout,confirm:()=>confirmed,
  localStorage:{getItem:k=>stored.get(k)||null,setItem:(k,v)=>stored.set(k,v)},fetch,setInterval:()=>1,clearInterval:()=>{}};
 vm.createContext(context);
 let code=fs.readFileSync(path.join(__dirname,'../seiya/ui.html'),'utf8').split('<script>')[1].split('</script>')[0].replace('setInterval(refreshJobs,2000);','')
  .replace("fetch('/api/chat',{method:'POST',headers,body:JSON.stringify(payload)})",'seiyaChatFetch(headers,payload)').replace("fetch('/health',{headers})",'seiyaHealthFetch(headers)')
  .replace('jobsEnabled=data.jobs?.enabled===true;','jobsEnabled=data.jobs?.enabled===true;seiyaJobHistoryCapabilities(data.jobs);seiyaImageCapabilities(data.images);');
 for(const file of ['job_history_ui.js','image_jobs_ui.js'])code+='\n'+fs.readFileSync(path.join(__dirname,'../../crates/seiya-server/src',file),'utf8');
 vm.runInContext(code,context);return{context,get,run:s=>vm.runInContext(s,context)};
}
const settle=async()=>{await new Promise(r=>setImmediate(r));await new Promise(r=>setImmediate(r));};
const submits=()=>calls.filter(c=>c.body?.operation==='submit').length;
function button(root,text){if(root.tag==='button'&&root.textContent===text)return root;for(const c of root.children||[]){const found=button(c,text);if(found)return found;}}
(async()=>{
 let b=browser();await settle();const id=b.run('activeId');b.get('key').value='DO_NOT_PERSIST';
 const draft=owner('jobs',id);draft.jobs.push({job_id:randomUUID(),request_id:randomUUID(),phase:'completed',artifact_available:true});
 b.context.seiyaSetImageMode(true);b.get('prompt').value='synthetic image';await b.get('form').fire('submit');await settle();assert.equal(submits(),1);
 b=browser();await settle();assert.equal(b.run('activeId'),id);assert.equal(submits(),1);assert.match(b.run('seiyaImagePanel.textContent'),/PNG validado/);assert.match(b.run('jobsPanel.textContent'),/Borrador preparado/);
 assert.match(b.run('seiyaImagePanel.textContent'),/persistente/);assert.ok(!stored.get('seiya.chats.v1').includes('DO_NOT_PERSIST'));
 for(const o of [...brokers.images.values(),...brokers.jobs.values()])assert.ok(!stored.get('seiya.chats.v1').includes(o.session));
 // Lost response, then refresh: recovery may list but never resubmit.
 lose=true;offline=true;b.context.seiyaSetImageMode(true);b.get('prompt').value='ambiguous image';await b.get('form').fire('submit');assert.ok(b.run('activeChat().imagePending'));const sent=submits();
 offline=false;b=browser();await settle();assert.equal(submits(),sent);assert.equal(b.run('activeChat().imagePending'),undefined);
 confirmed=false;await b.get('deleteChat').fire('click');assert.equal(b.run('activeId'),id);assert.ok(owner('images',id).jobs.length);
 confirmed=true;await b.get('deleteChat').fire('click');assert.notEqual(b.run('activeId'),id);assert.equal(owner('images',id).jobs.length,0);assert.equal(owner('jobs',id).jobs.length,0);
 await settle();const newId=b.run('activeId');
 // Slow registration cannot erase a newly typed draft, or accept a double submit.
 holdSubmit=true;b.context.seiyaSetImageMode(true);b.get('prompt').value='slow registration';const sending=b.get('form').fire('submit');await settle();assert.equal(b.run('busy'),true);assert.equal(b.get('chats').disabled,true);const during=submits();await b.get('form').fire('submit');assert.equal(submits(),during);
 b.get('prompt').value='new draft while awaiting ACK';b.get('prompt').oninput();releaseSubmit();holdSubmit=false;await sending;assert.equal(b.get('prompt').value,'new draft while awaiting ACK');assert.equal(b.run('activeChat().draft'),'new draft while awaiting ACK');assert.equal(b.run('seiyaImageMode'),true);
 // Storage failure must precede any irreversible submission.
 const setter=b.context.localStorage.setItem;b.context.localStorage.setItem=()=>{throw Error('quota fixture');};const beforeStorage=submits();await b.get('form').fire('submit');assert.equal(submits(),beforeStorage);assert.match(b.get('status').textContent,/no se pudo guardar/i);b.context.localStorage.setItem=setter;
 // Expired auth pauses polling until an explicit connection check; never submits.
 denied=true;await b.context.seiyaRefreshImages();assert.equal(b.run('seiyaHistoryAuthBlocked'),true);const paused=calls.length;await b.context.refreshJobs();await b.context.seiyaRefreshImages();assert.equal(calls.length,paused);denied=false;await b.get('checkRuntime').fire('click');await settle();assert.equal(b.run('seiyaHistoryAuthBlocked'),false);assert.equal(submits(),beforeStorage);
 // A response for the previous conversation cannot repaint the new one.
 holdList=true;const listing=b.context.seiyaRefreshImages();await settle();b.context.clearChat();assert.notEqual(b.run('activeId'),newId);assert.equal(b.run('seiyaImagePanel.hidden'),true);holdList=false;releaseList();await listing;assert.equal(b.run('seiyaImagePanel.hidden'),true);b.context.openChat(newId);await settle();assert.equal(submits(),beforeStorage);
 // Invalid states cannot be interpreted as terminal or permit deletion.
 corrupt=true;await b.context.seiyaRefreshImages();assert.match(b.run('seiyaImagePanel.textContent'),/inválido/);const forgotten=calls.filter(c=>c.body?.operation==='forget').length;await b.get('deleteChat').fire('click');assert.equal(b.run('activeId'),newId);assert.equal(calls.filter(c=>c.body?.operation==='forget').length,forgotten);corrupt=false;
 // Cancel acknowledgement is not terminal confirmation.
 const jobs=owner('images',newId).jobs;jobs[0].phase='running';jobs[0].artifact_available=false;await b.context.seiyaRefreshImages();await b.get('deleteChat').fire('click');assert.equal(b.run('activeId'),newId);assert.equal(jobs[0].phase,'cancelling');assert.match(b.get('status').textContent,/Chat conservado/);await settle();jobs[0].phase='completed';jobs[0].artifact_available=true;await b.context.seiyaRefreshImages();
 // Concurrent view/download share one transfer; a late PNG after deletion is discarded.
 holdArtifact=true;const view=button(b.run('seiyaImagePanel'),'Ver imagen'),download=button(b.run('seiyaImagePanel'),'Descargar PNG');const viewing=view.click();await settle();const artifactCount=calls.filter(c=>c.body?.operation==='artifact').length;const downloading=download.click();await settle();assert.equal(calls.filter(c=>c.body?.operation==='artifact').length,artifactCount);const priorBlobs=blob;await b.get('deleteChat').fire('click');assert.notEqual(b.run('activeId'),newId);releaseArtifact();holdArtifact=false;await viewing;await downloading;assert.equal(blob,priorBlobs);
 // Deterministic timeout: no real delay and no retry, even if fetch ignores abort.
 let timeout,delay;b.context.setTimeout=(fn,ms)=>{timeout=fn;delay=ms;return 7;};b.context.clearTimeout=()=>{};const beforeTimeout=submits();const request=b.context.seiyaControlFetch(()=>new Promise(()=>{}));assert.equal(delay,15000);timeout();await assert.rejects(request,/Estado incierto/);assert.equal(submits(),beforeTimeout);
 const chatRequest=b.context.seiyaChatFetch({},{});assert.equal(delay,150000);timeout();await assert.rejects(chatRequest,/Estado incierto/);assert.equal(calls.filter(c=>c.route==='/api/chat').length,1);
 console.log('History/UI robustness: reload/no replay, draft preservation, storage quota, auth reconnection, malformed state, cancel confirmation, shared/late Blob and deterministic timeout OK (synthetic only)');
})().catch(e=>{console.error(e);process.exitCode=1;});
