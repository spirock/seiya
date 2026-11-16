'use strict';
const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs');
class Element {
 constructor(tag='div'){this.tag=tag;this.children=[];this.attrs={};this.style={};this.parentNode=null;this.hidden=false;this.text='';}
 append(...nodes){for(const node of nodes){node.remove();node.parentNode=this;this.children.push(node);}}
 remove(){if(this.parentNode)this.parentNode.children=this.parentNode.children.filter(c=>c!==this);this.parentNode=null;}
 insertBefore(node,other){node.remove();node.parentNode=this;const i=this.children.indexOf(other);this.children.splice(i<0?this.children.length:i,0,node);}
 setAttribute(k,v){this.attrs[k]=v;}getAttribute(k){return this.attrs[k]??null;}
 set textContent(v){this.text=String(v);this.children.forEach(c=>c.parentNode=null);this.children=[];}
 get textContent(){return this.text+this.children.map(c=>c.textContent).join('');}
 querySelectorAll(selector){
  const result=[];const matches=node=>selector==='.message'?node.className?.split(' ').includes('message'):selector==='[data-seiya-image-progress]'?node.getAttribute('data-seiya-image-progress')!==null:node.tag===selector;
  const walk=node=>{for(const child of node.children){if(matches(child))result.push(child);walk(child);}};walk(this);return result;
 }
 querySelector(selector){return this.querySelectorAll(selector)[0]||null;}
 get firstChild(){return this.children[0]||null;}
 get nextSibling(){if(!this.parentNode)return null;return this.parentNode.children[this.parentNode.children.indexOf(this)+1]||null;}
}
const code=fs.readFileSync('crates/seiya-server/src/chat_image_progress_ui.js','utf8');
function fixture(bind=true){
 const history=new Element(),panel=new Element(),chat={messages:[{role:'user',content:'crea una imagen'},{role:'assistant',content:'Solicitud de imagen registrada. Consulta el panel.'}]};
 if(bind)chat.imageJobMessages={'request-1':1};
 for(const role of ['user','assistant']){const node=new Element();node.className='message '+role;history.append(node);}
 const calls=[];const ctx={document:{createElement:tag=>new Element(tag)},$:id=>id==='history'?history:null,activeId:'chat',activeChat:()=>chat,seiyaImagePanel:panel,seiyaImageLabels:{preparing:'Comprobando permiso, recursos y archivos',running:'Ejecutando generador local',blocked:'Bloqueada por permisos o recursos',completed:'PNG validado'},seiyaRenderImages:(data)=>{
  panel.children.forEach(c=>c.parentNode=null);panel.children=[];panel.hidden=false;
  for(const [tag,text] of [['h2','Imágenes locales'],['p','Historial local persistente.']]){const node=new Element(tag);node.textContent=text;panel.append(node);}
  for(const job of data.jobs){const row=new Element();row.setAttribute('data-image-job-id',job.job_id);const status=new Element('p');status.textContent=job.job_id+' · '+ctx.seiyaImageLabels[job.phase];row.append(status);if(job.phase==='preparing'||job.phase==='running'){const cancel=new Element('button');cancel.textContent='Cancelar imagen';cancel.onclick=()=>calls.push('cancel');row.append(cancel);}panel.append(row);}
 }};
 vm.createContext(ctx);vm.runInContext(code,ctx);
 return {ctx,history,panel,chat,calls};
}
const job=phase=>({job_id:'job-1',request_id:'request-1',phase,artifact_available:false,progress_percent:null});
const f=fixture();
f.ctx.seiyaRenderImages({jobs:[job('preparing')]},'chat','session');
assert.equal(f.panel.hidden,true);assert.equal(f.panel.textContent,'');
let card=f.history.querySelector('[data-seiya-image-progress]');
assert.ok(card);assert.equal(card.querySelector('progress').getAttribute('value'),null);
assert.match(card.textContent,/ID de imagen: job-1/);assert.match(card.textContent,/Cancelar imagen/);
assert.ok(card.querySelector('small').nextSibling.tag==='button','ID above cancel');
f.ctx.seiyaRenderImages({jobs:[job('running')]},'chat','session');
assert.equal(f.history.querySelectorAll('[data-seiya-image-progress]').length,1);
f.ctx.seiyaRenderImages({jobs:[job('blocked')]},'chat','session');
card=f.history.querySelector('[data-seiya-image-progress]');
assert.ok(card);assert.equal(card.querySelector('progress'),null);assert.equal(card.querySelector('button'),null);
assert.equal(f.calls.length,0,'render never submits or cancels');
const legacy=fixture(false);legacy.ctx.seiyaRenderImages({jobs:[job('preparing')]},'chat','session');
assert.ok(legacy.history.querySelector('[data-seiya-image-progress]'),'single old job can attach without binding');
const unbound=fixture();unbound.chat.messages.push({role:'assistant',content:'Solicitud de imagen registrada. Más tarde.'});
unbound.chat.imageJobMessages={};
unbound.ctx.seiyaRenderImages({jobs:[job('blocked'),{...job('blocked'),job_id:'job-2',request_id:'request-2'}]},'chat','session');
assert.equal(unbound.panel.hidden,false,'ambiguous jobs remain visible, never guessed');
assert.doesNotMatch(unbound.panel.textContent,/Imágenes locales|Historial local persistente/);
console.log('Chat image progress: indeterminate bar, same-message status/ID/cancel, terminal state, old single-job and ambiguous fallback OK (DOM mocks)');
