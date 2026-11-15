// Text chat contract, synthetic DOM/HTTP only; no models or external network.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const {randomUUID}=require('node:crypto');
class Element{
 constructor(){this.value='';this.children=[];this.attrs={};this.hidden=false;}
 append(...nodes){this.children.push(...nodes);for(const n of nodes)n.parent=this;}
 replaceChildren(...nodes){this.children=[];this.append(...nodes);}
 setAttribute(k,v){this.attrs[k]=v;}addEventListener(){}focus(){}
 set textContent(v){this.text=String(v);this.children=[];}get textContent(){return(this.text||'')+this.children.map(n=>n.textContent||'').join('');}
 set innerHTML(_){throw Error('unsafe HTML');}
 remove(){this.parent.children=this.parent.children.filter(n=>n!==this);}
}
const source=fs.readFileSync(path.join(__dirname,'../seiya/ui.html'),'utf8').split('<script>')[1].split('</script>')[0];
const success=()=>({choices:[{message:{role:'assistant',content:'Synthetic answer'}}],swarm:{elapsed_seconds:1}});
function browser(initial,images=false){
 const elements={},get=id=>elements[id]??=new Element(),stored=new Map(initial||[]),calls=[];
 let result=success(),status=200,nonJson=false,release;
 const context={document:{getElementById:get,createElement:()=>new Element(),createTextNode:t=>{const e=new Element();e.textContent=t;return e;},querySelectorAll:()=>[]},crypto:{randomUUID},Date,URL,TextEncoder,AbortController,setTimeout,clearTimeout,setInterval:()=>1,clearInterval:()=>{},confirm:()=>true,
 localStorage:{getItem:k=>stored.get(k)||null,setItem:(k,v)=>stored.set(k,v)},
 fetch:async(route,options)=>{
  if(route==='/health')return{ok:true,json:async()=>({})};
  assert.equal(route,'/api/chat');calls.push(JSON.parse(options.body));
  if(result==='network-fail')throw Error('synthetic connection lost');
  if(result==='hold')await new Promise(r=>release=r);
  return{ok:status===200,status,statusText:'Synthetic HTTP failure',json:async()=>{if(nonJson)throw Error('not JSON');return result;}};
 }};
 vm.createContext(context);
 let code=source.replace('setInterval(refreshJobs,2000);','').replace("fetch('/api/chat',{method:'POST',headers,body:JSON.stringify(payload)})",'seiyaChatFetch(headers,payload)').replace("fetch('/health',{headers})",'seiyaHealthFetch(headers)');
 for(const f of ['job_history_ui.js','chat_text_ui.js'])code+='\n'+fs.readFileSync(path.join(__dirname,'../../crates/seiya-server/src',f),'utf8');
 if(images){
  context.seiyaImageSession=async()=> 'synthetic-image-capability';context.seiyaImageLabels={queued:'queued'};context.seiyaRefreshImages=async()=>{};context.seiyaRenderImages=()=>{};
  code+='\n'+fs.readFileSync(path.join(__dirname,'../../crates/seiya-server/src/chat_images_ui.js'),'utf8');
 }
 code=code.replace("let data;try{data=await r.json();}catch{throw Error('Respuesta de chat inválida; estado incierto, no reenvíes sin comprobar.');}","let data;try{data=await r.json();}catch{if(!r.ok)throw seiyaHttpError(r,null);throw Error('Respuesta de chat inválida; estado incierto, no reenvíes sin comprobar.');}");
 vm.runInContext(code,context);
 return{context,get,stored,calls,run:s=>vm.runInContext(s,context),reply:(v,s=200,n=false)=>{result=v;status=s;nonJson=n;},release:()=>release(),send:async text=>{get('prompt').value=text;await get('form').onsubmit({preventDefault(){}});}};
}
(async()=>{
 let b=browser();await b.send('hello');assert.equal(b.calls.length,1);assert.equal(b.run('messages.length'),2);assert.match(b.get('history').textContent,/Synthetic answer/);
 b.reply({choices:[{message:{role:'assistant',content:'No timer'}}]});await b.send('again');assert.equal(b.run('messages.length'),4);assert.match(b.get('status').textContent,/— s/);assert.doesNotMatch(b.get('status').textContent,/Error/);
 for(const bad of [null,{}, {choices:[]},{choices:[{message:{role:'user',content:'wrong'}}]},{choices:[{message:{role:'assistant',content:' '}}]}, {...success(),web:{sources:{}}},{...success(),web:{reads:[null]}},{...success(),jobs:{jobs:[{}]}},{choices:[{message:{role:'assistant',content:'not executed',tool_calls:[{}]}}]},{choices:[{message:{role:'assistant',content:'invalid calls',tool_calls:{}}}]}]){
  b=browser();b.reply(bad);await b.send('keep me');assert.equal(b.run('messages.length'),0);assert.equal(b.get('history').textContent,'');assert.equal(b.get('prompt').value,'keep me');assert.match(b.get('status').textContent,/Error/);assert.equal(b.calls.length,1);
 }
 b=browser();b.reply(null,401,true);await b.send('private');assert.equal(b.run('seiyaHistoryAuthBlocked'),true);assert.match(b.get('status').textContent,/Acceso/);assert.equal(b.get('prompt').value,'private');
 for(const failure of ['network-fail',null]){b=browser();b.reply(failure,500,failure===null);await b.send('uncertain');assert.equal(b.calls.length,1);assert.equal(b.run('messages.length'),0);assert.equal(b.get('prompt').value,'uncertain');assert.match(b.get('status').textContent,/incierto/);}
 b=browser();b.reply({choices:[{message:{role:'assistant',content:'<script>not executable</script>'}}],swarm:{elapsed_seconds:-1}});await b.send('literal');assert.match(b.get('history').textContent,/<script>/);assert.match(b.get('status').textContent,/— s/);
 b=browser();b.reply('hold');const request=b.send('first');await new Promise(r=>setImmediate(r));assert.equal(b.run('busy'),true);await b.get('form').onsubmit({preventDefault(){}});assert.equal(b.calls.length,1);b.get('prompt').value='next draft';b.get('prompt').oninput();b.reply(success());b.release();await request;assert.equal(b.get('prompt').value,'next draft');assert.equal(b.run('activeChat().draft'),'next draft');
 b=browser();b.run("messages.push({role:'user',content:'unanswered'});saveChats()");b=browser(b.stored);assert.equal(b.calls.length,0);assert.equal(b.run('seiyaRecoverText.hidden'),false);
 b.get('prompt').value='different draft';b.run('seiyaRecoverText.onclick()');assert.equal(b.run('messages.length'),1);assert.equal(b.get('prompt').value,'different draft');
 b.get('prompt').value='';b.run('seiyaRecoverText.onclick()');assert.equal(b.calls.length,0);assert.equal(b.run('messages.length'),0);assert.equal(b.get('prompt').value,'unanswered');await b.get('form').onsubmit({preventDefault(){}});assert.equal(b.calls.length,1);assert.equal(b.calls[0].messages.length,1);
 b=browser();b.run("messages.push({role:'user',content:'preserved during deletion'});seiyaHistoryDeleting=true;seiyaRecoverText.onclick()");assert.equal(b.run('messages.length'),1);assert.equal(b.get('prompt').value,'');assert.equal(b.calls.length,0);
 b=browser();b.run("messages.push({role:'user',content:'saved'});saveChats()");b=browser(b.stored);b.context.localStorage.setItem=()=>{throw Error('quota');};b.run('seiyaRecoverText.onclick()');assert.equal(b.run('messages.length'),1);assert.equal(b.get('prompt').value,'');assert.equal(b.calls.length,0);
 let submitted=0;b.get('form').requestSubmit=()=>submitted++;for(const flags of [{shiftKey:true},{isComposing:true},{}])b.get('prompt').onkeydown({key:'Enter',preventDefault(){},...flags});assert.equal(submitted,1);
 // Actual extension ordering: invalid text must retain the chat/image request ID.
 b=browser(undefined,true);b.reply({});await b.send('synthetic image');const pending=b.run('activeChat().chatImagePending.id');assert.equal(b.calls[0].image_request_id,pending);
 b.reply({...success(),image_job:{job_id:randomUUID(),request_id:pending,phase:'queued',artifact_available:false}});await b.get('form').onsubmit({preventDefault(){}});assert.equal(b.calls.length,2);assert.equal(b.calls[1].image_request_id,pending);assert.equal(b.run('activeChat().chatImagePending'),undefined);assert.equal(b.run('messages.length'),2);
 console.log('Text chat: validated replies, no phantom messages, non-JSON auth, draft/double-submit, explicit recovery/no replay, quota and IME OK (mocks only)');
})().catch(e=>{console.error(e);process.exitCode=1;});
