// Production script order and event dispatch; synthetic broker/PNG transport only.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const {randomUUID}=require('node:crypto');
class Element{
 constructor(tag='div'){this.tag=tag;this.value='';this.children=[];this.attrs={};this.listeners={};this.style={};}
 append(...nodes){this.children.push(...nodes);for(const n of nodes){n.parent=this;n.parentNode=this;}}
 replaceChildren(...nodes){this.children=[];this.text='';this.append(...nodes);}setAttribute(k,v){this.attrs[k]=v;}getAttribute(k){return this.attrs[k];}focus(){}
 set textContent(v){this.text=String(v);this.children=[];}get textContent(){return(this.text||'')+this.children.map(n=>n.textContent||'').join('');}
 set innerHTML(_){throw Error('unsafe HTML');}remove(){this.parent.children=this.parent.children.filter(n=>n!==this);}
 addEventListener(n,fn){(this.listeners[n]??=[]).push(fn);}
 async fire(n){const e={preventDefault(){},stopImmediatePropagation(){this.stopped=true;}};for(const fn of this.listeners[n]||[]){await fn(e);if(e.stopped)return;}if(this['on'+n])await this['on'+n](e);}
 click(){if(this.tag!=='a')return this.fire('click');}
}
const settle=async()=>{await new Promise(r=>setImmediate(r));await new Promise(r=>setImmediate(r));};
const ok=data=>({ok:true,status:200,json:async()=>data});
function browser(stored=new Map(),brokers=new Map()){
 const elements={},get=id=>elements[id]??=new Element(),controls=new Element();controls.append(get('send'));get('form').append(controls);
 const calls=[];let lost=true,quota=false,quotaAfterAck=false,override,deny=false,holdOnce=false,release;let generated=0,blobs=0;
 class LocalURL extends URL{static createObjectURL(){return'blob:synthetic-'+(++blobs);}static revokeObjectURL(){}}
 function owner(id){if(!brokers.has(id))brokers.set(id,{session:randomUUID().replaceAll('-','')+randomUUID().replaceAll('-',''),jobs:[]});return brokers.get(id);}
 const context={document:{getElementById:get,createElement:t=>new Element(t),createTextNode:t=>{const e=new Element();e.textContent=t;return e;},querySelectorAll:()=>[]},crypto:{randomUUID},Date,URL:LocalURL,Blob,TextEncoder,AbortController,setTimeout,clearTimeout,setInterval:()=>1,clearInterval:()=>{},confirm:()=>true,
 localStorage:{getItem:k=>stored.get(k)||null,setItem:(k,v)=>{if(quota)throw Error('synthetic quota');stored.set(k,v);}},
 fetch:async(route,options)=>{
  const frame=options?.body?JSON.parse(options.body):null;calls.push({route,frame});
  if(route==='/health')return ok({jobs:{enabled:false},images:{configured:true,available:true,volatile:false}});
  if(route==='/api/chat'){
   const o=[...brokers.values()].find(o=>o.session===frame.image_session);assert.ok(o);
   let job=o.jobs.find(j=>j.request_id===frame.image_request_id);
   if(!job){generated++;job={job_id:randomUUID(),request_id:frame.image_request_id,phase:'running',artifact_available:false};o.jobs.push(job);}
   if(lost){lost=false;throw Error('synthetic lost ACK');}
   if(quotaAfterAck)quota=true;
   return ok({choices:[{message:{role:'assistant',content:'Solicitud de imagen registrada'}}],swarm:{elapsed_seconds:0},image_job:override===undefined?job:override});
  }
  assert.equal(route,'/api/images/v1','unexpected route '+route);
  if(deny)return{ok:false,status:401,json:async()=>({error:{message:'synthetic auth rejection'}})};
  if(frame.operation==='open')return ok({session:owner(frame.conversation_id).session,volatile:false});
  const o=[...brokers.values()].find(o=>o.session===frame.session);assert.ok(o);
  if(frame.operation==='list'){const data={jobs:o.jobs.map(j=>({...j})),volatile:false};if(holdOnce){holdOnce=false;await new Promise(r=>release=r);}return ok(data);}
  if(frame.operation==='cancel'){o.jobs.find(j=>j.job_id===frame.job_id).phase='cancelling';return ok({});}
  if(frame.operation==='forget'){o.jobs=[];return ok({forgotten:true});}
  if(frame.operation==='artifact'){const blob=new Blob(['synthetic transport, server validation covered in Rust'],{type:'image/png'});return{ok:true,headers:{get:k=>k==='content-type'?'image/png':k==='content-length'?String(blob.size):null},blob:async()=>blob};}
  throw Error('recovery must not submit an image: '+frame.operation);
 }};
 vm.createContext(context);
 let code=fs.readFileSync(path.join(__dirname,'../seiya/ui.html'),'utf8').split('<script>')[1].split('</script>')[0]
  .replace('setInterval(refreshJobs,2000);','')
  .replace("fetch('/api/chat',{method:'POST',headers,body:JSON.stringify(payload)})",'seiyaChatFetch(headers,payload)')
  .replace("fetch('/health',{headers})",'seiyaHealthFetch(headers)')
  .replace('jobsEnabled=data.jobs?.enabled===true;','jobsEnabled=data.jobs?.enabled===true;seiyaJobHistoryCapabilities(data.jobs);seiyaImageCapabilities(data.images);');
 for(const f of ['job_history_ui.js','image_jobs_ui.js','chat_text_ui.js','chat_images_ui.js'])code+='\n'+fs.readFileSync(path.join(__dirname,'../../crates/seiya-server/src',f),'utf8');
 assert.equal(code.split('if(busy||seiyaHistoryDeleting)return;').length,2,'guard hook must replace exactly one shared handler');
 code=code.replace('if(busy||seiyaHistoryDeleting)return;','if(busy||seiyaHistoryDeleting)return;if(seiyaChatImageDeletionBlocked())return;');
 vm.runInContext(code,context);
 return{context,get,stored,brokers,calls,owner,run:s=>vm.runInContext(s,context),generated:()=>generated,blobs:()=>blobs,
  options:v=>{if('lost'in v)lost=v.lost;if('quota'in v)quota=v.quota;if('quotaAfterAck'in v)quotaAfterAck=v.quotaAfterAck;if('override'in v)override=v.override;if('deny'in v)deny=v.deny;if('holdOnce'in v)holdOnce=v.holdOnce;},release:()=>release(),send:async text=>{get('prompt').value=text;await get('form').fire('submit');await settle();}};
}
function button(root,text){if(root.tag==='button'&&root.textContent===text)return root;for(const child of root.children||[]){const found=button(child,text);if(found)return found;}}
const chats=b=>b.calls.filter(c=>c.route==='/api/chat').length;
const writes=b=>b.calls.filter(c=>['submit','forget'].includes(c.frame?.operation)).length;
(async()=>{
 let b=browser();await settle();await b.send('Generate a synthetic horse');const id=b.run('activeId'),pending=b.run('activeChat().chatImagePending.id');assert.equal(b.generated(),1);assert.equal(b.run('seiyaRecoverChatImage.hidden'),false);
 // Real registration order: the shared capture handler runs before the old guard.
 const beforeDelete=b.calls.length;await b.get('deleteChat').click();assert.equal(b.calls.length,beforeDelete);assert.equal(b.run('activeId'),id);assert.equal(writes(b),0);assert.match(b.get('status').textContent,/sin confirmar/);
 await b.run('seiyaRecoverChatImage.onclick()');assert.equal(chats(b),1);assert.equal(b.generated(),1);assert.equal(b.run('activeChat().chatImagePending'),undefined);assert.equal(b.get('prompt').value,'');assert.match(b.get('status').textContent,/recuperado sin reenviar/);
 // Running is not completed: no PNG fetch, forgetting or deletion.
 assert.equal(b.blobs(),0);await b.get('deleteChat').click();await settle();assert.equal(b.run('activeId'),id);assert.equal(writes(b),0);assert.equal(b.owner(id).jobs[0].phase,'cancelling');
 b.owner(id).jobs[0].phase='completed';b.owner(id).jobs[0].artifact_available=true;await b.context.seiyaRefreshImages();const view=button(b.run('seiyaImagePanel'),'Ver imagen');assert.ok(view);await view.click();assert.equal(b.blobs(),1);assert.equal(b.generated(),1);
 assert.ok(!b.stored.get('seiya.chats.v1').includes('blob:'));for(const o of b.brokers.values())assert.ok(!b.stored.get('seiya.chats.v1').includes(o.session));
 const stored=b.stored,brokers=b.brokers;b=browser(stored,brokers);await settle();assert.equal(chats(b),0);assert.equal(b.run('activeId'),id);assert.ok(button(b.run('seiyaImagePanel'),'Ver imagen'));
 // Reloading a lost ACK keeps the same ID and can recover without another chat.
 b=browser();await settle();await b.send('lost before reload');const ref=b.run('activeChat().chatImagePending.id');const old=b;b=browser(b.stored,b.brokers);await settle();assert.equal(b.run('activeChat().chatImagePending.id'),ref);b.get('prompt').value='new draft';b.get('prompt').oninput();await b.run('seiyaRecoverChatImage.onclick()');assert.equal(chats(b),0);assert.equal(old.generated(),1);assert.equal(b.get('prompt').value,'new draft');assert.equal(b.run('activeChat().draft'),'new draft');
 // Persistence failure rolls back; fresh-list absence/auth never clears pending.
 b=browser();await settle();await b.send('preserved');b.options({quota:true});await b.run('seiyaRecoverChatImage.onclick()');assert.ok(b.run('activeChat().chatImagePending'));assert.equal(b.get('prompt').value,'preserved');assert.match(b.get('status').textContent,/conservados/);b.options({quota:false,deny:true});await b.run('seiyaRecoverChatImage.onclick()');assert.ok(b.run('activeChat().chatImagePending'));assert.equal(b.run('seiyaHistoryAuthBlocked'),true);
 b.options({deny:false});b.owner(b.run('activeId')).jobs=[];await b.run('seiyaRecoverChatImage.onclick()');assert.ok(b.run('activeChat().chatImagePending'));assert.match(b.get('status').textContent,/no está confirmado/);assert.equal(chats(b),1);
 b=browser();await settle();await b.send('ambiguous');const o=b.owner(b.run('activeId'));o.jobs.push({...o.jobs[0],job_id:randomUUID()});await b.context.seiyaRefreshImages();assert.equal(b.run('seiyaRecoverChatImage.hidden'),true);await b.run('seiyaRecoverChatImage.onclick()');assert.ok(b.run('activeChat().chatImagePending'));assert.match(b.get('status').textContent,/ambiguo/);assert.equal(chats(b),1);
 b=browser();await settle();b.run('activeChat().chatImagePending=false');const beforeCorrupt=b.calls.length;await b.get('deleteChat').click();assert.equal(b.calls.length,beforeCorrupt);assert.equal(b.run('activeChat().chatImagePending'),false);
 // A late recovery response cannot mutate another conversation or its editor.
 b=browser();await settle();await b.send('late');const previousId=b.run('activeId');b.options({holdOnce:true});const recovering=b.run('seiyaRecoverChatImage.onclick()');await settle();b.context.clearChat();b.get('prompt').value='other conversation';b.release();await recovering;assert.notEqual(b.run('activeId'),previousId);assert.equal(b.get('prompt').value,'other conversation');assert.ok(b.run(`chats.find(c=>c.id===${JSON.stringify(previousId)}).chatImagePending`));
 // A false/null/malformed ACK cannot discard the durable request reference.
 for(const invalid of [false,null,'job',{}, {job_id:randomUUID(),request_id:pending,phase:'completed',artifact_available:false}]){b=browser();await settle();b.options({lost:false,override:invalid});await b.send('invalid ACK');assert.ok(b.run('activeChat().chatImagePending'));assert.equal(chats(b),1);}
 b=browser();await settle();b.options({lost:false,quotaAfterAck:true});await b.send('quota after ACK');assert.ok(b.run('activeChat().chatImagePending'));assert.equal(b.run('messages.length'),0);assert.equal(chats(b),1);
 console.log('Chat/image recovery: shared deletion guard, lost ACK/reload without resubmit, observed progress/PNG, draft/quota/auth, stale responses and strict ACK OK (synthetic only)');
})().catch(e=>{console.error(e);process.exitCode=1;});
