// Synthetic DOM/network only; no runtime, external network or model invocation.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const {randomUUID}=require('node:crypto');
const downloads=[],revoked=[],calls=[],stored=new Map();let imageAvailable=false,chatCalls=0,loseSubmit=false,listOffline=false,rejectSubmit=false,current;
class Element{
 constructor(tag='div'){this.tag=tag;this.value='';this.children=[];this.attrs={};this.style={};this.listeners={};this._text='';}
 append(...items){this.children.push(...items);for(const c of items)if(c&&typeof c==='object'){c.parent=this;c.parentNode=this;}}
 setAttribute(k,v){this.attrs[k]=v;}replaceChildren(...v){this.children=[];this._text='';this.append(...v);}focus(){}
 set textContent(v){this._text=String(v);this.children=[];}get textContent(){return this._text+this.children.map(c=>c.textContent||'').join('');}
 set innerHTML(_){throw Error('unsafe innerHTML');}
 remove(){if(this.parent)this.parent.children=this.parent.children.filter(c=>c!==this);}
 addEventListener(name,fn,capture){(this.listeners[name]??=[]).push({fn,capture});}
 async fire(name){const e={preventDefault(){},stopImmediatePropagation(){this.stopped=true;}};for(const h of this.listeners[name]||[]){await h.fn(e);if(e.stopped)return;}if(this['on'+name])await this['on'+name](e);}
 click(){if(this.tag==='a'){downloads.push({name:this.download,url:this.href});return;}return this.fire('click');}
}
const els={},get=id=>els[id]??=new Element(),controls=new Element();get('form').append(controls);controls.append(get('send'));
const ownerJobs=new Map(),ownerConversations=new Map();let blobCount=0;
class LocalURL extends URL{static createObjectURL(){return 'blob:fixture-'+(++blobCount);}static revokeObjectURL(url){revoked.push(url);}}
function reply(value){return{ok:true,json:async()=>value};}
const ctx={document:{getElementById:get,createElement:t=>new Element(t),createTextNode:t=>{const e=new Element('#text');e.textContent=t;return e;},querySelectorAll:()=>[]},crypto:{randomUUID},Date,URL:LocalURL,Blob,TextEncoder,confirm:()=>true,
 localStorage:{getItem:k=>stored.get(k)||null,setItem:(k,v)=>stored.set(k,v)},setInterval:()=>1,clearInterval:()=>{},
 fetch:async(route,options)=>{
  calls.push({route,options});if(route==='/health')return reply({images:{available:imageAvailable},jobs:{enabled:false}});
  if(route==='/api/chat'){chatCalls++;return reply({choices:[{message:{content:'Respuesta de chat SIMULADA PARA TEST'}}],swarm:{elapsed_seconds:0}});}
  assert.equal(route,'/api/images/v1');const frame=JSON.parse(options.body);
  if(frame.operation==='session'){const session=randomUUID().replaceAll('-','')+randomUUID().replaceAll('-','');ownerJobs.set(session,[]);return reply({session});}
  if(frame.operation==='open'){if(!ownerConversations.has(frame.conversation_id)){const session=randomUUID().replaceAll('-','')+randomUUID().replaceAll('-','');ownerConversations.set(frame.conversation_id,session);ownerJobs.set(session,[]);}return reply({session:ownerConversations.get(frame.conversation_id)});}
  if(frame.operation==='forget'){ownerJobs.set(frame.session,[]);return reply({forgotten:true});}
  const jobs=ownerJobs.get(frame.session);assert.ok(jobs,'unknown synthetic capability');
  if(frame.operation==='submit'){
   if(rejectSubmit)return{ok:false,status:503,json:async()=>({error:{message:'fixture admission blocked'}})};
   let job=jobs.find(j=>j.request_id===frame.request_id);if(!job){job={job_id:randomUUID(),request_id:frame.request_id,phase:'preparing',artifact_available:false};jobs.push(job);}current=job;
   if(loseSubmit){loseSubmit=false;throw Error('synthetic response lost');}return reply({...job});
  }
  if(frame.operation==='list'){if(listOffline)throw Error('synthetic status offline');return reply({jobs:jobs.map(j=>({...j}))});}
  const job=jobs.find(j=>j.job_id===frame.job_id);assert.ok(job);
  if(frame.operation==='cancel'){job.phase='cancelling';return reply({...job});}
  if(frame.operation==='artifact'){
   assert.equal(job.phase,'completed');const blob=new Blob([Buffer.from('PNG bytes MOCK ONLY')],{type:'image/png'});
   return{ok:true,headers:{get:k=>k==='content-type'?'image/png':k==='content-length'?String(blob.size):null},blob:async()=>blob};
  }throw Error('unexpected command');
 }};
vm.createContext(ctx);
let code=fs.readFileSync(path.join(__dirname,'../seiya/ui.html'),'utf8').split('<script>')[1].split('</script>')[0];
code=code.replace('jobsEnabled=data.jobs?.enabled===true;','jobsEnabled=data.jobs?.enabled===true;seiyaImageCapabilities(data.images);');
code+='\n'+fs.readFileSync(path.join(__dirname,'../../crates/seiya-server/src/image_jobs_ui.js'),'utf8');vm.runInContext(code,ctx);
const tick=()=>new Promise(r=>setImmediate(r));const run=expr=>vm.runInContext(expr,ctx);
function find(root,text){if(root.tag==='button'&&root.textContent===text)return root;for(const c of root.children||[]){const found=find(c,text);if(found)return found;}}
function imageCalls(op){return calls.filter(c=>c.route==='/api/images/v1'&&JSON.parse(c.options.body).operation===op);}
async function start(text){imageAvailable=true;ctx.seiyaImageCapabilities({available:true});ctx.seiyaSetImageMode(true);get('prompt').value=text;await get('form').fire('submit');}
(async()=>{
 await tick();assert.equal(run('seiyaImageToggle.hidden'),true);assert.equal(imageCalls('submit').length,0);
 get('key').value='NEVER_STORE_IMAGE_KEY';await start('<script>data only</script>');assert.equal(chatCalls,0);assert.equal(imageCalls('submit').length,1);
 const sent=JSON.parse(imageCalls('submit')[0].options.body);assert.equal(sent.request.prompt,'<script>data only</script>');assert.equal(sent.request.width,512);assert.equal(sent.request.seed,42);assert.ok(sent.request_id);assert.equal(run('busy'),false);
 current.phase='running';await ctx.seiyaRefreshImages();assert.match(run('seiyaImagePanel.textContent'),/Ejecutando generador/);assert.doesNotMatch(run('seiyaImagePanel.textContent'),/\d+%/);
 get('prompt').value='Hola';await get('form').fire('submit');assert.equal(chatCalls,1);
 await find(run('seiyaImagePanel'),'Cancelar imagen').click();assert.match(run('seiyaImagePanel.textContent'),/esperando confirmación/);assert.equal(imageCalls('artifact').length,0);
 current.phase='cancelled';await ctx.seiyaRefreshImages();assert.match(run('seiyaImagePanel.textContent'),/Cancelada/);
 await start('synthetic completed image');current.phase='completed';current.artifact_available=true;await ctx.seiyaRefreshImages();
 await find(run('seiyaImagePanel'),'Ver imagen').click();assert.equal(blobCount,1);await find(run('seiyaImagePanel'),'Descargar PNG').click();assert.deepEqual(downloads,[{name:'seiya-image.png',url:'blob:fixture-1'}]);
 assert.equal(imageCalls('artifact')[0].options.headers.Authorization,'Bearer NEVER_STORE_IMAGE_KEY');const history=stored.get('seiya.chats.v1');assert.ok(!history.includes(sent.session));assert.ok(!history.includes('NEVER_STORE_IMAGE_KEY'));assert.ok(!history.includes('blob:fixture'));
 await start('image before deletion');current.phase='running';await ctx.seiyaRefreshImages();const before=run('activeId');await get('deleteChat').fire('click');assert.equal(run('activeId'),before);assert.match(get('status').textContent,/Chat conservado/);
 current.phase='cancelled';await get('deleteChat').fire('click');assert.notEqual(run('activeId'),before);assert.deepEqual(revoked,['blob:fixture-1']);
 ctx.seiyaImageCapabilities({available:true});ctx.seiyaSetImageMode(true);ctx.seiyaImageCapabilities({available:false});get('prompt').value='Do not silently send to chat';const priorChat=chatCalls,priorImages=imageCalls('submit').length;
 await get('form').fire('submit');assert.equal(chatCalls,priorChat);assert.equal(imageCalls('submit').length,priorImages);assert.match(get('status').textContent,/No se ha enviado nada/);
 imageAvailable=true;ctx.seiyaImageCapabilities({available:true});get('prompt').value='á'.repeat(1025);await get('form').fire('submit');assert.equal(imageCalls('submit').length,priorImages);assert.match(get('status').textContent,/2048 bytes/);
 loseSubmit=true;listOffline=true;get('prompt').value='idempotent manual retry';await get('form').fire('submit');const lost=JSON.parse(imageCalls('submit').at(-1).options.body);const count=imageCalls('submit').length;await tick();assert.equal(imageCalls('submit').length,count);
 listOffline=false;await get('form').fire('submit');assert.equal(JSON.parse(imageCalls('submit').at(-1).options.body).request_id,lost.request_id);assert.equal(ownerJobs.get(lost.session).length,1);
 rejectSubmit=true;await start('rejected request');assert.equal(run('seiyaImagePending.has(activeId)'),false);rejectSubmit=false;get('prompt').value='changed after confirmed rejection';await get('form').fire('submit');assert.equal(JSON.parse(imageCalls('submit').at(-1).options.body).request.prompt,'changed after confirmed rejection');
 console.log('Image UI: capabilities, isolated submit, observed states, cancellation, PNG Blob/download, deletion, revocation, UTF-8 bounds and manual idempotent retry OK (DOM/network mocks)');
})().catch(e=>{console.error(e);process.exitCode=1;});
