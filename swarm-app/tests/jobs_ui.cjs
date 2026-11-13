// Synthetic DOM/network only: never talks to a model or service.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const {randomUUID}=require('node:crypto');
class Element{constructor(){this.value='';this.children=[];this.attrs={};}append(...v){this.children.push(...v);for(const c of v)if(typeof c==='object')c.parent=this;}setAttribute(k,v){this.attrs[k]=v;}replaceChildren(){this.children=[];}focus(){}remove(){if(this.parent)this.parent.children=this.parent.children.filter(c=>c!==this);}}
const els={},get=id=>els[id]??=new Element(),stored=new Map(),calls=[];
const capability='b'.repeat(64),jobId=randomUUID();let phase='generating',resolveChat;
const ctx={document:{getElementById:get,createElement:()=>new Element(),createTextNode:text=>({text}),querySelectorAll:()=>[]},crypto:{randomUUID},Date,URL,confirm:()=>true,
 localStorage:{getItem:k=>stored.get(k)||null,setItem:(k,v)=>stored.set(k,v)},setInterval:()=>1,clearInterval:()=>{},
 fetch:async(path,options)=>{calls.push({path,options});if(path==='/health')return{ok:true,json:async()=>({jobs:{enabled:true}})};
 if(path==='/api/jobs'){const frame=JSON.parse(options.body);let value;if(frame.operation==='session')value={session:capability};else if(frame.operation==='cancel'){phase='cancelled';value={phase};}else value={jobs:[{job_id:jobId,phase,artifact_available:false}],foreground_allowed:true,auxiliary_limit:2};return{ok:true,json:async()=>value};}
 return new Promise(resolve=>{resolveChat=resolve;});}};
vm.createContext(ctx);const script=fs.readFileSync('seiya/ui.html','utf8').split('<script>')[1].split('</script>')[0];vm.runInContext(script,ctx);
const turn=()=>new Promise(resolve=>setImmediate(resolve));
(async()=>{
 await turn();get('key').value='NEVER_STORE_KEY';get('prompt').value='Haz un programa Python';
 const pending=get('form').onsubmit({preventDefault(){}});await turn();
 const sent=JSON.parse(calls.find(c=>c.path==='/api/chat').options.body);assert.equal(sent.job_session,capability);assert.ok(sent.job_request_id);
 resolveChat({ok:true,json:async()=>({choices:[{message:{content:'Trabajo recibido'}}],swarm:{elapsed_seconds:0},jobs:{jobs:[{job_id:jobId,phase,artifact_available:false}],auxiliary_limit:2}})});await pending;
 assert.equal(vm.runInContext('busy',ctx),false);assert.equal(get('send').disabled,false);
 assert.equal(vm.runInContext('jobsPanel.hidden',ctx),false);
 assert.match(vm.runInContext('jobsPanel.children[2].children[0].textContent',ctx),/Generando borrador/);
 assert.ok(!stored.get('seiya.chats.v1').includes(capability));assert.ok(!stored.get('seiya.chats.v1').includes('NEVER_STORE_KEY'));
 const before=vm.runInContext('activeId',ctx);await get('deleteChat').onclick();
 assert.equal(vm.runInContext('activeId',ctx),before);assert.match(get('status').textContent,/No se eliminó/);
 await get('deleteChat').onclick();assert.notEqual(vm.runInContext('activeId',ctx),before);
 assert.equal(vm.runInContext('jobsPanel.hidden',ctx),true);
 console.log('Jobs UI: chat remains free, observed states, cancellation before deletion, session isolation and no persisted secrets OK');
})().catch(e=>{console.error(e);process.exitCode=1;});
